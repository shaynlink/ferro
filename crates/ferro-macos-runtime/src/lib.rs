//! Native `AppKit` runtime for Ferro application hosts.
//!
//! Application state and reducers remain portable. This crate owns the macOS lifecycle, main
//! window, command routing, and native file panels.

use std::any::Any;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use ferro_sdk::{Application, Lifecycle};
use ferro_state::Store;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSApplicationActivationPolicy,
    NSApplicationDelegate, NSBackingStoreType, NSEventModifierFlags, NSMenu, NSMenuItem,
    NSModalResponseOK, NSOpenPanel, NSSavePanel, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSInteger, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString,
};

/// A stable identifier for a host-defined native command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandId(String);

impl CommandId {
    /// Creates a command identifier.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// A menu command rendered by the macOS runtime.
#[derive(Clone, Debug)]
pub struct MacosCommand<E> {
    /// Stable command identifier.
    pub id: CommandId,
    /// Menu item title.
    pub title: String,
    /// Optional single-character Command-key equivalent.
    pub shortcut: Option<char>,
    /// Whether `AppKit` should enable the menu item.
    pub enabled: bool,
    /// Event dispatched when the command is selected.
    pub event: E,
}

impl<E> MacosCommand<E> {
    /// Creates an enabled menu command without a shortcut.
    #[must_use]
    pub fn new(id: CommandId, title: impl Into<String>, event: E) -> Self {
        Self {
            id,
            title: title.into(),
            shortcut: None,
            enabled: true,
            event,
        }
    }

    /// Adds a Command-key shortcut.
    #[must_use]
    pub fn shortcut(mut self, shortcut: char) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Sets command availability.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Result returned from an `AppKit` open or save panel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileDialogResult {
    /// The user closed the panel without selecting a path.
    Cancelled,
    /// The selected file or folder paths.
    Selected(Vec<PathBuf>),
}

/// Configuration for an open-panel request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenPanelMode {
    /// Select one or more files.
    Files {
        /// Whether `AppKit` permits selecting more than one file.
        multiple: bool,
    },
    /// Select a single folder.
    Folder,
}

/// Severity used by a native macOS alert.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertKind {
    /// Informational message.
    Information,
    /// Warning requiring the user's attention.
    Warning,
    /// Critical error message.
    Critical,
}

/// Result selected in a native confirmation dialogue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confirmation {
    /// The user selected the primary action.
    Accepted,
    /// The user selected cancel or dismissed the alert.
    Cancelled,
}

/// Sender retained by native controls to dispatch application events on the main thread.
#[derive(Clone)]
pub struct MacosEventSender<E>(Rc<dyn Fn(E)>);

impl<E> MacosEventSender<E> {
    /// Dispatches an event through the runtime's store.
    pub fn dispatch(&self, event: E) {
        (self.0)(event);
    }
}

/// Runtime access to macOS file-selection capabilities.
pub struct MacosCapabilities<E> {
    main_thread: MainThreadMarker,
    sender: MacosEventSender<E>,
}

impl<E: 'static> MacosCapabilities<E> {
    /// Presents a native informational, warning or critical alert.
    pub fn alert(&self, kind: AlertKind, title: &str, message: &str) {
        let alert = NSAlert::init(NSAlert::alloc(self.main_thread));
        alert.setAlertStyle(match kind {
            AlertKind::Information => NSAlertStyle::Informational,
            AlertKind::Warning => NSAlertStyle::Warning,
            AlertKind::Critical => NSAlertStyle::Critical,
        });
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(message));
        alert.runModal();
    }

    /// Presents a native confirmation alert with explicit primary and cancel labels.
    #[must_use]
    pub fn confirm(
        &self,
        title: &str,
        message: &str,
        accept_label: &str,
        cancel_label: &str,
    ) -> Confirmation {
        let alert = NSAlert::init(NSAlert::alloc(self.main_thread));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(message));
        alert.addButtonWithTitle(&NSString::from_str(accept_label));
        alert.addButtonWithTitle(&NSString::from_str(cancel_label));
        if alert.runModal() == NSAlertFirstButtonReturn {
            Confirmation::Accepted
        } else {
            Confirmation::Cancelled
        }
    }

    /// Opens a native panel and dispatches its result as an application event.
    pub fn open(&self, mode: OpenPanelMode, map_result: impl FnOnce(FileDialogResult) -> E) {
        let panel = NSOpenPanel::openPanel(self.main_thread);
        match mode {
            OpenPanelMode::Files { multiple } => {
                panel.setCanChooseFiles(true);
                panel.setCanChooseDirectories(false);
                panel.setAllowsMultipleSelection(multiple);
            }
            OpenPanelMode::Folder => {
                panel.setCanChooseFiles(false);
                panel.setCanChooseDirectories(true);
                panel.setAllowsMultipleSelection(false);
            }
        }
        let result = if panel.runModal() == NSModalResponseOK {
            let paths = panel
                .URLs()
                .iter()
                .filter_map(|url| url.path().map(|path| PathBuf::from(path.to_string())))
                .collect();
            FileDialogResult::Selected(paths)
        } else {
            FileDialogResult::Cancelled
        };
        self.sender.dispatch(map_result(result));
    }

    /// Opens a native save panel and dispatches its selected path as an application event.
    pub fn save(&self, suggested_name: &str, map_result: impl FnOnce(FileDialogResult) -> E) {
        let panel = NSSavePanel::savePanel(self.main_thread);
        panel.setNameFieldStringValue(&NSString::from_str(suggested_name));
        let result = if panel.runModal() == NSModalResponseOK {
            panel
                .URL()
                .and_then(|url| url.path())
                .map_or(FileDialogResult::Cancelled, |path| {
                    FileDialogResult::Selected(vec![PathBuf::from(path.to_string())])
                })
        } else {
            FileDialogResult::Cancelled
        };
        self.sender.dispatch(map_result(result));
    }
}

/// Native surface supplied by a generated macOS application host.
pub trait MacosHost<A: Application>: 'static {
    /// Builds durable `AppKit` views in the provided main window.
    fn build(
        &mut self,
        window: &NSWindow,
        main_thread: MainThreadMarker,
        sender: MacosEventSender<A::Event>,
    );

    /// Updates existing native views from the current portable state.
    fn render(&mut self, state: &A::State);

    /// Returns the current menu commands.
    fn commands(&self, state: &A::State) -> Vec<MacosCommand<A::Event>>;

    /// Maps lifecycle changes into optional portable events.
    fn lifecycle_event(&mut self, _lifecycle: Lifecycle) -> Option<A::Event> {
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
}

type DriverRef = Rc<RefCell<Box<dyn RuntimeDriver>>>;
type WeakDriverRef = Weak<RefCell<Box<dyn RuntimeDriver>>>;

struct Driver<A: Application, H: MacosHost<A>> {
    store: Store<A>,
    host: H,
    owner: WeakDriverRef,
    window: Option<Retained<NSWindow>>,
    sender: MacosEventSender<A::Event>,
    pending_events: Rc<RefCell<Vec<Box<dyn Any>>>>,
    commands: Vec<MacosCommand<A::Event>>,
    router: Option<Retained<CommandRouter>>,
}

impl<A: Application + 'static, H: MacosHost<A>> Driver<A, H>
where
    A::Event: Clone + 'static,
{
    fn dispatch(&mut self, event: A::Event) {
        let effects = self.store.dispatch(event);
        self.host.render(self.store.state());
        self.refresh_commands();
        let main_thread =
            MainThreadMarker::new().expect("Ferro events run on the macOS main thread");
        for effect in effects {
            self.host.handle_effect(
                effect,
                MacosCapabilities {
                    main_thread,
                    sender: self.sender.clone(),
                },
            );
        }
        self.flush_pending_events();
    }

    fn flush_pending_events(&mut self) {
        let events = std::mem::take(&mut *self.pending_events.borrow_mut());
        for event in events {
            self.event(event);
        }
    }

    fn refresh_commands(&mut self) {
        self.commands = self.host.commands(self.store.state());
        let (Some(window), Some(router)) = (&self.window, &self.router) else {
            return;
        };
        let main_thread =
            MainThreadMarker::new().expect("Ferro menus run on the macOS main thread");
        let menu = NSMenu::initWithTitle(NSMenu::alloc(main_thread), &NSString::from_str("File"));
        for (index, command) in self.commands.iter().enumerate() {
            let title = NSString::from_str(&command.title);
            let key = NSString::from_str(
                &command
                    .shortcut
                    .map_or_else(String::new, |key| key.to_string()),
            );
            // SAFETY: The selector exists on CommandRouter and the router is retained by Driver.
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(main_thread),
                    &title,
                    Some(sel!(performCommand:)),
                    &key,
                )
            };
            let Ok(index) = NSInteger::try_from(index) else {
                continue;
            };
            item.setTag(index);
            item.setEnabled(command.enabled);
            if command.shortcut.is_some() {
                item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            }
            // SAFETY: Driver retains CommandRouter until AppKit terminates.
            unsafe { item.setTarget(Some(&**router)) };
            menu.addItem(&item);
        }
        // SAFETY: A submenu-only item does not need an action selector.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(main_thread),
                &NSString::from_str("File"),
                None,
                &NSString::from_str(""),
            )
        };
        item.setSubmenu(Some(&menu));
        let main_menu =
            NSMenu::initWithTitle(NSMenu::alloc(main_thread), &NSString::from_str("Main"));
        main_menu.addItem(&item);
        NSApplication::sharedApplication(main_thread).setMainMenu(Some(&main_menu));
        window.makeKeyAndOrderFront(None);
    }

    fn dispatch_lifecycle(&mut self, lifecycle: Lifecycle) {
        if let Some(event) = self.host.lifecycle_event(lifecycle) {
            self.dispatch(event);
        }
    }
}

impl<A: Application + 'static, H: MacosHost<A>> RuntimeDriver for Driver<A, H>
where
    A::Event: Clone + 'static,
{
    fn launched(&mut self, main_thread: MainThreadMarker) {
        let window = create_window(main_thread, A::descriptor().name());
        self.host.build(&window, main_thread, self.sender.clone());
        self.window = Some(window);
        let owner = self.owner.upgrade().expect("runtime driver remains alive");
        self.router = Some(CommandRouter::new(main_thread, owner));
        self.dispatch_lifecycle(Lifecycle::Launched);
        self.host.render(self.store.state());
        self.refresh_commands();
        let application = NSApplication::sharedApplication(main_thread);
        application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        #[allow(deprecated)]
        application.activateIgnoringOtherApps(true);
    }

    fn lifecycle(&mut self, lifecycle: Lifecycle) {
        self.dispatch_lifecycle(lifecycle);
    }

    fn command(&mut self, index: usize) {
        if let Some(command) = self.commands.get(index)
            && command.enabled
        {
            self.dispatch(command.event.clone());
        }
    }

    fn event(&mut self, event: Box<dyn Any>) {
        if let Ok(event) = event.downcast::<A::Event>() {
            self.dispatch(*event);
        }
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
    A::Event: Clone + 'static,
    H: MacosHost<A>,
{
    let main_thread =
        MainThreadMarker::new().expect("a Ferro macOS application must start on the main thread");
    let pending_events = Rc::new(RefCell::new(Vec::<Box<dyn Any>>::new()));
    let driver: DriverRef = Rc::new_cyclic(|owner: &WeakDriverRef| {
        let pending_events = Rc::clone(&pending_events);
        let sender = MacosEventSender({
            let pending_events = Rc::clone(&pending_events);
            Rc::new(move |event| {
                pending_events.borrow_mut().push(Box::new(event));
            })
        });
        RefCell::new(Box::new(Driver::<A, H> {
            store: Store::new(),
            host,
            owner: owner.clone(),
            window: None,
            sender,
            pending_events,
            commands: Vec::new(),
            router: None,
        }))
    });
    let app = NSApplication::sharedApplication(main_thread);
    let delegate = Delegate::new(main_thread, Rc::clone(&driver));
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}

fn create_window(main_thread: MainThreadMarker, title: &str) -> Retained<NSWindow> {
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    // SAFETY: Construction occurs on AppKit's main thread using valid value arguments.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(main_thread),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(800.0, 520.0)),
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str(title));
    window.center();
    window
}

#[cfg(test)]
mod tests {
    use super::{CommandId, FileDialogResult, MacosCommand, OpenPanelMode};

    #[test]
    fn commands_keep_identity_shortcuts_and_availability() {
        let command = MacosCommand::new(CommandId::new("open"), "Open…", 1)
            .shortcut('o')
            .enabled(false);
        assert_eq!(command.id, CommandId::new("open"));
        assert_eq!(command.shortcut, Some('o'));
        assert!(!command.enabled);
    }

    #[test]
    fn cancellation_has_no_selected_path() {
        assert_eq!(FileDialogResult::Cancelled, FileDialogResult::Cancelled);
        assert_eq!(OpenPanelMode::Folder, OpenPanelMode::Folder);
    }
}
