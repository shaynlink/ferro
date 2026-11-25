//! Multi-window management and native `NSWindow` lifecycle routing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use ferro_sdk::{WindowDescriptor, WindowId, WindowLifecycle, WindowStyle};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSFloatingWindowLevel, NSNormalWindowLevel, NSWindow, NSWindowDelegate,
    NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

use crate::view_host::configure_traffic_lights;

pub(crate) trait WindowEventSink {
    fn on_window_lifecycle(&mut self, event: WindowLifecycle);
}

struct WindowDelegateIvars {
    window_id: WindowId,
    sink: Rc<RefCell<Box<dyn WindowEventSink>>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = WindowDelegateIvars]
    struct ManagedWindowDelegate;

    unsafe impl NSObjectProtocol for ManagedWindowDelegate {}

    unsafe impl NSWindowDelegate for ManagedWindowDelegate {
        #[unsafe(method(windowDidBecomeKey:))]
        fn did_become_key(&self, _notification: &NSNotification) {
            let id = self.ivars().window_id.clone();
            if let Ok(mut sink) = self.ivars().sink.try_borrow_mut() {
                sink.on_window_lifecycle(WindowLifecycle::Focused(id));
            }
        }

        #[unsafe(method(windowWillClose:))]
        fn will_close(&self, _notification: &NSNotification) {
            let id = self.ivars().window_id.clone();
            if let Ok(mut sink) = self.ivars().sink.try_borrow_mut() {
                sink.on_window_lifecycle(WindowLifecycle::Closed(id));
            }
        }

        #[unsafe(method(windowDidResize:))]
        fn did_resize(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else {
                return;
            };
            let Ok(window) = object.downcast::<NSWindow>() else {
                return;
            };
            let frame = window
                .contentView()
                .map_or_else(|| window.frame(), |view| view.frame());
            let id = self.ivars().window_id.clone();
            if let Ok(mut sink) = self.ivars().sink.try_borrow_mut() {
                sink.on_window_lifecycle(WindowLifecycle::Resized {
                    id,
                    width: frame.size.width,
                    height: frame.size.height,
                });
            }
        }
    }
);

impl ManagedWindowDelegate {
    fn new(
        main_thread: MainThreadMarker,
        window_id: WindowId,
        sink: Rc<RefCell<Box<dyn WindowEventSink>>>,
    ) -> Retained<Self> {
        let this = Self::alloc(main_thread).set_ivars(WindowDelegateIvars { window_id, sink });
        // SAFETY: NSObject's init has its declared Objective-C signature.
        unsafe { msg_send![super(this), init] }
    }
}

pub(crate) struct ManagedWindow {
    pub(crate) window: Retained<NSWindow>,
    _delegate: Retained<ManagedWindowDelegate>,
}

pub(crate) struct WindowRegistry {
    windows: HashMap<WindowId, ManagedWindow>,
}

impl WindowRegistry {
    pub(crate) fn new() -> Self {
        Self {
            windows: HashMap::new(),
        }
    }

    pub(crate) fn get(&self, id: &WindowId) -> Option<&NSWindow> {
        self.windows.get(id).map(|entry| &*entry.window)
    }

    pub(crate) fn insert(
        &mut self,
        main_thread: MainThreadMarker,
        descriptor: &WindowDescriptor,
        sink: Rc<RefCell<Box<dyn WindowEventSink>>>,
    ) -> &NSWindow {
        let window = create_ns_window(main_thread, descriptor);
        let delegate = ManagedWindowDelegate::new(main_thread, descriptor.id.clone(), sink);
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        if let WindowStyle::Overlay {
            traffic_light_offset: Some((x, y)),
        } = descriptor.style
        {
            configure_traffic_lights(&window, x, y);
        }
        window.makeKeyAndOrderFront(None);
        self.windows.insert(
            descriptor.id.clone(),
            ManagedWindow {
                window,
                _delegate: delegate,
            },
        );
        &self.windows[&descriptor.id].window
    }

    pub(crate) fn remove(&mut self, id: &WindowId) {
        if let Some(managed) = self.windows.remove(id) {
            managed.window.setDelegate(None);
            managed.window.close();
        }
    }

    pub(crate) fn forget(&mut self, id: &WindowId) {
        if let Some(managed) = self.windows.remove(id) {
            managed.window.setDelegate(None);
        }
    }

    pub(crate) fn focus(&self, id: &WindowId) {
        if let Some(window) = self.get(id) {
            window.makeKeyAndOrderFront(None);
        }
    }

    pub(crate) fn set_always_on_top(&self, id: &WindowId, always_on_top: bool) {
        if let Some(window) = self.get(id) {
            let level = if always_on_top {
                NSFloatingWindowLevel
            } else {
                NSNormalWindowLevel
            };
            window.setLevel(level);
        }
    }

    pub(crate) fn set_title(&self, id: &WindowId, title: &str) {
        if let Some(window) = self.get(id) {
            window.setTitle(&NSString::from_str(title));
        }
    }
}

pub(crate) fn create_ns_window(
    main_thread: MainThreadMarker,
    descriptor: &WindowDescriptor,
) -> Retained<NSWindow> {
    let mut style =
        NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
    if descriptor.resizable {
        style |= NSWindowStyleMask::Resizable;
    }
    match descriptor.style {
        WindowStyle::Standard => {}
        WindowStyle::Overlay { .. } => {
            style |= NSWindowStyleMask::FullSizeContentView;
        }
        WindowStyle::HudPanel { non_activating } => {
            style |= NSWindowStyleMask::FullSizeContentView;
            if non_activating {
                style |= NSWindowStyleMask::NonactivatingPanel;
            }
        }
    }

    // SAFETY: Construction occurs on AppKit's main thread using valid value arguments.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(main_thread),
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(descriptor.width, descriptor.height),
            ),
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str(&descriptor.title));

    if let (Some(min_w), Some(min_h)) = (descriptor.min_width, descriptor.min_height) {
        window.setContentMinSize(NSSize::new(min_w, min_h));
    }

    if matches!(
        descriptor.style,
        WindowStyle::Overlay { .. } | WindowStyle::HudPanel { .. }
    ) {
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setMovableByWindowBackground(true);
    }

    if descriptor.always_on_top {
        window.setLevel(NSFloatingWindowLevel);
    }

    if let (Some(x), Some(y)) = (descriptor.x, descriptor.y) {
        window.setFrameTopLeftPoint(NSPoint::new(x, y));
    } else {
        window.center();
    }

    window
}
