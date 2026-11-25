//! Native `AppKit` runtime for Ferro application hosts.
//!
//! Application state and reducers remain portable. This crate owns the macOS lifecycle,
//! multi-window management, command/menu routing, background event wake-up, and native OS panels.

mod async_bridge;
mod capabilities;
mod menus;
#[cfg(feature = "preview-webview")]
pub mod preview_webview;
pub mod status_bar;
pub mod view_host;
mod windows;

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use ferro_sdk::{
    AppEventProxy, Application, FerroError, FerroErrorKind, Lifecycle, MenuSection,
    WindowDescriptor, WindowId, WindowLifecycle,
};
use ferro_state::Store;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSMenu, NSMenuItem,
    NSWindow,
};
use objc2_foundation::{MainThreadMarker, NSNotification, NSObject, NSObjectProtocol};

use crate::async_bridge::{
    MacosEventQueue, clear_main_flusher, create_event_proxy, register_main_flusher,
};
use crate::capabilities::WindowController;
use crate::menus::build_menu_bar;
use crate::windows::{WindowEventSink, WindowRegistry};

pub use capabilities::{
    AlertKind, AppearanceMode, CommandId, Confirmation, FileDialogResult, MacosCapabilities,
    MacosCommand, MacosEventSender, OpenPanelMode, validate_external_url,
};

/// Native surface supplied by a macOS application host.
pub trait MacosHost<A: Application>: 'static {
    /// Returns the descriptor used to construct the primary application window.
    fn main_window_descriptor(&self) -> WindowDescriptor {
        WindowDescriptor::new(WindowId::main(), A::descriptor().name())
    }

    /// Builds durable `AppKit` views in the primary main window.
    fn build(
        &mut self,
        window: &NSWindow,
        main_thread: MainThreadMarker,
        sender: MacosEventSender<A::Event>,
    );

    /// Builds durable `AppKit` views inside a specific window (primary or secondary).
    fn build_window(
        &mut self,
        id: &WindowId,
        window: &NSWindow,
        main_thread: MainThreadMarker,
        sender: MacosEventSender<A::Event>,
        _proxy: AppEventProxy<A::Event>,
    ) {
        if id.is_main() {
            self.build(window, main_thread, sender);
        }
    }

    /// Updates existing native views from the current portable state.
    fn render(&mut self, state: &A::State);

    /// Updates a specific window's native views from the current portable state.
    fn render_window(&mut self, id: &WindowId, state: &A::State) {
        if id.is_main() {
            self.render(state);
        }
    }

    /// Returns legacy single-File-menu commands.
    fn commands(&self, _state: &A::State) -> Vec<MacosCommand<A::Event>> {
        Vec::new()
    }

    /// Returns full macOS menu-bar sections; falls back to [`Self::commands`] when empty.
    fn menu_sections(&self, _state: &A::State) -> Vec<MenuSection<A::Event>> {
        Vec::new()
    }

    /// Maps application lifecycle changes into optional portable events.
    fn lifecycle_event(&mut self, _lifecycle: Lifecycle) -> Option<A::Event> {
        None
    }

    /// Maps window-level lifecycle notifications into optional portable events.
    fn window_lifecycle_event(&mut self, _event: WindowLifecycle) -> Option<A::Event> {
        None
    }

    /// Executes an application effect with macOS-native capabilities.
    fn handle_effect(&mut self, effect: A::Effect, capabilities: MacosCapabilities<A::Event>);
}

trait RuntimeDriver {
    fn launched(&mut self, main_thread: MainThreadMarker);
    fn lifecycle(&mut self, lifecycle: Lifecycle);
    fn command(&mut self, index: usize);
    fn event(&mut self, event: Box<dyn Any>);
    fn drain_async(&mut self);
}

type DriverRef = Rc<RefCell<Box<dyn RuntimeDriver>>>;
type WeakDriverRef = Weak<RefCell<Box<dyn RuntimeDriver>>>;

struct DriverWindowSink<E> {
    driver: WeakDriverRef,
    _marker: std::marker::PhantomData<E>,
}

impl<E: 'static> WindowEventSink for DriverWindowSink<E> {
    fn on_window_lifecycle(&mut self, event: WindowLifecycle) {
        if let Some(driver) = self.driver.upgrade()
            && let Ok(mut guard) = driver.try_borrow_mut()
        {
            guard.event(Box::new(InternalMsg::<E>::Window(event)));
        }
    }
}

enum InternalMsg<E> {
    App(E),
    Window(WindowLifecycle),
}

struct Driver<A: Application, H: MacosHost<A>> {
    store: Store<A>,
    host: H,
    owner: WeakDriverRef,
    windows: Rc<RefCell<WindowRegistry>>,
    open_ids: Vec<WindowId>,
    sender: MacosEventSender<A::Event>,
    proxy: AppEventProxy<A::Event>,
    async_queue: std::sync::Arc<MacosEventQueue<A::Event>>,
    pending_events: Rc<RefCell<Vec<Box<dyn Any>>>>,
    command_events: Vec<A::Event>,
    router: Option<Retained<CommandRouter>>,
    active_menu: Option<Retained<NSMenu>>,
}

struct DriverWindowController<A: Application, H: MacosHost<A>> {
    driver: WeakDriverRef,
    main_thread: MainThreadMarker,
    _marker: std::marker::PhantomData<(A, H)>,
    registry: Rc<RefCell<WindowRegistry>>,
    sender: MacosEventSender<A::Event>,
    proxy: AppEventProxy<A::Event>,
    host_ptr: *mut H,
    open_ids: *mut Vec<WindowId>,
}

impl<A: Application + 'static, H: MacosHost<A>> WindowController for DriverWindowController<A, H> {
    fn open_window(&mut self, descriptor: &WindowDescriptor) -> Result<(), FerroError> {
        if self.registry.borrow().get(&descriptor.id).is_some() {
            self.registry.borrow().focus(&descriptor.id);
            return Ok(());
        }
        let sink: Rc<RefCell<Box<dyn WindowEventSink>>> =
            Rc::new(RefCell::new(Box::new(DriverWindowSink::<A::Event> {
                driver: self.driver.clone(),
                _marker: std::marker::PhantomData,
            })));
        self.registry
            .borrow_mut()
            .insert(self.main_thread, descriptor, sink);
        // SAFETY: `DriverWindowController` is only created inside `Driver::dispatch` and does not
        // outlive that scope. `host_ptr` and `open_ids` point into the exclusively borrowed `Driver`.
        unsafe {
            let registry = self.registry.borrow();
            let Some(ns_window) = registry.get(&descriptor.id) else {
                return Err(FerroError::new(
                    FerroErrorKind::Runtime,
                    "created window missing from registry",
                ));
            };
            (*self.open_ids).push(descriptor.id.clone());
            (*self.host_ptr).build_window(
                &descriptor.id,
                ns_window,
                self.main_thread,
                self.sender.clone(),
                self.proxy.clone(),
            );
        }
        Ok(())
    }

    fn close_window(&mut self, id: &WindowId) {
        self.registry.borrow_mut().remove(id);
        // SAFETY: Same scope invariant as `open_window`.
        unsafe {
            (*self.open_ids).retain(|item| item != id);
        }
    }

    fn focus_window(&mut self, id: &WindowId) {
        self.registry.borrow().focus(id);
    }

    fn set_always_on_top(&mut self, id: &WindowId, always_on_top: bool) {
        self.registry.borrow().set_always_on_top(id, always_on_top);
    }

    fn set_title(&mut self, id: &WindowId, title: &str) {
        self.registry.borrow().set_title(id, title);
    }
}

impl<A: Application + 'static, H: MacosHost<A>> Driver<A, H>
where
    A::Event: Clone + Send + Sync + 'static,
{
    fn dispatch(&mut self, event: A::Event) {
        let effects = self.store.dispatch(event);
        self.render_all_windows();
        self.refresh_commands();
        let main_thread =
            MainThreadMarker::new().expect("Ferro events run on the macOS main thread");

        for effect in effects {
            let controller = Rc::new(RefCell::new(DriverWindowController::<A, H> {
                driver: self.owner.clone(),
                main_thread,
                _marker: std::marker::PhantomData,
                registry: Rc::clone(&self.windows),
                sender: self.sender.clone(),
                proxy: self.proxy.clone(),
                host_ptr: &raw mut self.host,
                open_ids: &raw mut self.open_ids,
            }));
            self.host.handle_effect(
                effect,
                MacosCapabilities {
                    main_thread,
                    sender: self.sender.clone(),
                    windows: controller,
                },
            );
        }
        self.render_all_windows();
        self.flush_pending_events();
    }

    fn render_all_windows(&mut self) {
        let state = self.store.state();
        for id in &self.open_ids {
            self.host.render_window(id, state);
        }
    }

    fn flush_pending_events(&mut self) {
        loop {
            let queued = std::mem::take(&mut *self.pending_events.borrow_mut());
            let async_events = self.async_queue.drain();
            if queued.is_empty() && async_events.is_empty() {
                break;
            }
            for item in queued {
                self.event(item);
            }
            for item in async_events {
                self.dispatch(item);
            }
        }
    }

    fn refresh_commands(&mut self) {
        let Some(router) = &self.router else {
            return;
        };
        let main_thread =
            MainThreadMarker::new().expect("Ferro menus run on the macOS main thread");
        let sections = self.host.menu_sections(self.store.state());
        let legacy = if sections.is_empty() {
            self.host.commands(self.store.state())
        } else {
            Vec::new()
        };
        let (menu, events) = build_menu_bar(
            main_thread,
            A::descriptor().name(),
            &sections,
            &legacy,
            router,
        );
        self.command_events = events;
        NSApplication::sharedApplication(main_thread).setMainMenu(Some(&menu));
        self.active_menu = Some(menu);
    }

    fn dispatch_lifecycle(&mut self, lifecycle: Lifecycle) {
        if let Some(event) = self.host.lifecycle_event(lifecycle) {
            self.dispatch(event);
        }
    }
}

impl<A: Application + 'static, H: MacosHost<A>> RuntimeDriver for Driver<A, H>
where
    A::Event: Clone + Send + Sync + 'static,
{
    fn launched(&mut self, main_thread: MainThreadMarker) {
        let descriptor = self.host.main_window_descriptor();
        let sink: Rc<RefCell<Box<dyn WindowEventSink>>> =
            Rc::new(RefCell::new(Box::new(DriverWindowSink::<A::Event> {
                driver: self.owner.clone(),
                _marker: std::marker::PhantomData,
            })));
        {
            let mut registry = self.windows.borrow_mut();
            let window = registry.insert(main_thread, &descriptor, sink);
            self.host.build_window(
                &descriptor.id,
                window,
                main_thread,
                self.sender.clone(),
                self.proxy.clone(),
            );
        }
        self.open_ids.push(descriptor.id);
        let owner = self.owner.upgrade().expect("runtime driver remains alive");
        self.router = Some(CommandRouter::new(main_thread, owner));
        self.dispatch_lifecycle(Lifecycle::Launched);
        self.render_all_windows();
        self.refresh_commands();
        let application = NSApplication::sharedApplication(main_thread);
        application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        #[allow(deprecated)]
        application.activateIgnoringOtherApps(true);
    }

    fn lifecycle(&mut self, lifecycle: Lifecycle) {
        if lifecycle == Lifecycle::Terminating {
            self.async_queue.shutdown();
            clear_main_flusher();
        }
        self.dispatch_lifecycle(lifecycle);
    }

    fn command(&mut self, index: usize) {
        if let Some(event) = self.command_events.get(index).cloned() {
            self.dispatch(event);
        }
    }

    fn event(&mut self, event: Box<dyn Any>) {
        if let Ok(internal) = event.downcast::<InternalMsg<A::Event>>() {
            match *internal {
                InternalMsg::App(app_event) => self.dispatch(app_event),
                InternalMsg::Window(win_event) => {
                    if let WindowLifecycle::Closed(ref id) = win_event {
                        self.windows.borrow_mut().forget(id);
                        self.open_ids.retain(|item| item != id);
                    }
                    if let Some(mapped) = self.host.window_lifecycle_event(win_event) {
                        self.dispatch(mapped);
                    }
                }
            }
        }
    }

    fn drain_async(&mut self) {
        self.flush_pending_events();
    }
}

struct DelegateIvars {
    driver: DriverRef,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            self.ivars().driver.borrow_mut().launched(self.mtm());
        }

        #[unsafe(method(applicationDidBecomeActive:))]
        fn did_become_active(&self, _notification: &NSNotification) {
            self.ivars()
                .driver
                .borrow_mut()
                .lifecycle(Lifecycle::Activated);
        }

        #[unsafe(method(applicationDidResignActive:))]
        fn did_resign_active(&self, _notification: &NSNotification) {
            self.ivars()
                .driver
                .borrow_mut()
                .lifecycle(Lifecycle::Deactivated);
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _notification: &NSNotification) {
            self.ivars()
                .driver
                .borrow_mut()
                .lifecycle(Lifecycle::Terminating);
        }
    }
);

impl Delegate {
    fn new(main_thread: MainThreadMarker, driver: DriverRef) -> Retained<Self> {
        let this = Self::alloc(main_thread).set_ivars(DelegateIvars { driver });
        // SAFETY: NSObject's init has its declared Objective-C signature.
        unsafe { msg_send![super(this), init] }
    }
}

struct RouterIvars {
    driver: DriverRef,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = RouterIvars]
    struct CommandRouter;

    unsafe impl NSObjectProtocol for CommandRouter {}

    impl CommandRouter {
        #[unsafe(method(performCommand:))]
        fn perform_command(&self, sender: &NSMenuItem) {
            if let Ok(index) = usize::try_from(sender.tag()) {
                self.ivars().driver.borrow_mut().command(index);
            }
        }
    }
);

impl CommandRouter {
    fn new(main_thread: MainThreadMarker, driver: DriverRef) -> Retained<Self> {
        let this = Self::alloc(main_thread).set_ivars(RouterIvars { driver });
        // SAFETY: NSObject's init has its declared Objective-C signature.
        unsafe { msg_send![super(this), init] }
    }
}

/// Starts an `AppKit` application using a portable reducer and a macOS-specific host.
///
/// # Panics
///
/// Panics when called outside macOS's main thread.
pub fn run<A, H>(host: H)
where
    A: Application + 'static,
    A::Event: Clone + Send + Sync + 'static,
    H: MacosHost<A>,
{
    let main_thread =
        MainThreadMarker::new().expect("a Ferro macOS application must start on the main thread");
    let pending_events = Rc::new(RefCell::new(Vec::<Box<dyn Any>>::new()));
    let async_queue = MacosEventQueue::new();
    let proxy = create_event_proxy(&async_queue);

    let driver: DriverRef = Rc::new_cyclic(|owner: &WeakDriverRef| {
        let pending_events_for_sender = Rc::clone(&pending_events);
        let owner_for_sender = owner.clone();
        let sender = MacosEventSender(Rc::new(move |event| {
            if let Some(driver) = owner_for_sender.upgrade()
                && let Ok(mut guard) = driver.try_borrow_mut()
            {
                guard.event(Box::new(InternalMsg::App(event)));
                return;
            }
            pending_events_for_sender
                .borrow_mut()
                .push(Box::new(InternalMsg::App(event)));
        }));
        RefCell::new(Box::new(Driver::<A, H> {
            store: Store::new(),
            host,
            owner: owner.clone(),
            windows: Rc::new(RefCell::new(WindowRegistry::new())),
            open_ids: Vec::new(),
            sender,
            proxy,
            async_queue: std::sync::Arc::clone(&async_queue),
            pending_events,
            command_events: Vec::new(),
            router: None,
            active_menu: None,
        }))
    });

    let driver_for_async = Rc::downgrade(&driver);
    register_main_flusher(move || {
        if let Some(driver) = driver_for_async.upgrade()
            && let Ok(mut guard) = driver.try_borrow_mut()
        {
            guard.drain_async();
        }
    });

    let app = NSApplication::sharedApplication(main_thread);
    let delegate = Delegate::new(main_thread, Rc::clone(&driver));
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
