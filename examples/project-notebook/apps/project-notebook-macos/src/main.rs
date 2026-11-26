//! Native `AppKit` host for the Project Notebook tutorial.

use std::cell::{OnceCell, RefCell};

use ferro_contracts::DocumentRepository;
use ferro_macos_runtime::{
    CommandId, FileDialogResult, MacosCapabilities, MacosCommand, MacosEventSender, MacosHost,
    OpenPanelMode,
};
use ferro_storage::LiliaDocumentRepository;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadOnly, MainThreadMarker, define_class, msg_send, sel};
use objc2::rc::Retained;
use objc2_app_kit::{NSButton, NSFont, NSTextField, NSWindow};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use project_notebook_core::{AppState, Effect, Event, Note, ProjectNotebook, Route};

#[derive(Default)]
struct NotebookHost {
    status: OnceCell<Retained<NSTextField>>,
    detail: OnceCell<Retained<NSTextField>>,
    sender: OnceCell<MacosEventSender<Event>>,
    button_router: OnceCell<Retained<ButtonRouter>>,
    repository: RefCell<Option<LiliaDocumentRepository<Note>>>,
}

impl MacosHost<ProjectNotebook> for NotebookHost {
    fn build(&mut self, window: &NSWindow, main_thread: MainThreadMarker, sender: MacosEventSender<Event>) {
        let content = window.contentView().expect("runtime windows have content");
        let title = NSTextField::labelWithString(&NSString::from_str("Project Notebook"), main_thread);
        title.setFrame(NSRect::new(NSPoint::new(32.0, 390.0), NSSize::new(736.0, 36.0)));
        title.setFont(Some(&NSFont::systemFontOfSize(28.0)));
        let status = NSTextField::labelWithString(&NSString::from_str("Starting…"), main_thread);
        status.setFrame(NSRect::new(NSPoint::new(32.0, 340.0), NSSize::new(736.0, 24.0)));
        let detail = NSTextField::labelWithString(&NSString::from_str("Pick a section or attach a folder to start."), main_thread);
        detail.setFrame(NSRect::new(NSPoint::new(32.0, 304.0), NSSize::new(736.0, 24.0)));
        let router = ButtonRouter::new(main_thread, sender.clone());
        let open = native_button(main_thread, "Open Project Folder…", &router, sel!(openProject:));
        open.setFrame(NSRect::new(NSPoint::new(32.0, 244.0), NSSize::new(190.0, 32.0)));
        let notes = native_button(main_thread, "Notes", &router, sel!(showNotes:));
        notes.setFrame(NSRect::new(NSPoint::new(32.0, 192.0), NSSize::new(100.0, 30.0)));
        let project = native_button(main_thread, "Project", &router, sel!(showProject:));
        project.setFrame(NSRect::new(NSPoint::new(142.0, 192.0), NSSize::new(100.0, 30.0)));
        let weather = native_button(main_thread, "Weather", &router, sel!(showWeather:));
        weather.setFrame(NSRect::new(NSPoint::new(252.0, 192.0), NSSize::new(100.0, 30.0)));
        content.addSubview(&title);
        content.addSubview(&status);
        content.addSubview(&detail);
        content.addSubview(&open);
        content.addSubview(&notes);
        content.addSubview(&project);
        content.addSubview(&weather);
        self.status.set(status).expect("host builds once");
        self.detail.set(detail).expect("host builds once");
        assert!(self.sender.set(sender).is_ok(), "host builds once");
        assert!(self.button_router.set(router).is_ok(), "host builds once");
    }

    fn render(&mut self, state: &AppState) {
        if let Some(status) = self.status.get() { status.setStringValue(&NSString::from_str(&state.status)); }
        if let Some(detail) = self.detail.get() {
            let route = match state.route { Route::Notes => "Notes", Route::Project => "Project", Route::Weather => "Weather" };
            let project = state.project_root.as_ref().map_or_else(|| "No project folder".to_owned(), |path| path.display().to_string());
            detail.setStringValue(&NSString::from_str(&format!("{route} · {project}")));
        }
    }

    fn commands(&self, _state: &AppState) -> Vec<MacosCommand<Event>> {
        vec![
            MacosCommand::new(CommandId::new("project.open"), "Open Project Folder…", Event::ChooseProjectFolder).shortcut('o'),
            MacosCommand::new(CommandId::new("view.notes"), "Show Notes", Event::Navigate(Route::Notes)),
            MacosCommand::new(CommandId::new("view.weather"), "Show Weather", Event::Navigate(Route::Weather)),
        ]
    }

    fn handle_effect(&mut self, effect: Effect, capabilities: MacosCapabilities<Event>) {
        match effect {
            Effect::ChooseProjectFolder => capabilities.open(OpenPanelMode::Folder, |result| match result {
                FileDialogResult::Selected(paths) => Event::ProjectFolderChosen(paths.into_iter().next()),
                FileDialogResult::Cancelled => Event::ProjectFolderChosen(None),
            }),
            Effect::LoadNotes(root) => {
                let path = root.join(".ferro-notebook").join("data.lilia");
                let result = LiliaDocumentRepository::<Note>::open(path, "notes").and_then(|repository| {
                    let notes = repository.list()?;
                    *self.repository.borrow_mut() = Some(repository);
                    Ok(notes)
                }).map_err(|error| error.to_string());
                let sender = self.sender.get().expect("host is built before effects");
                sender.dispatch(Event::NotesLoaded(result));
            }
        }
    }
}

fn main() { ferro_macos_runtime::run::<ProjectNotebook, _>(NotebookHost::default()); }

struct ButtonRouterIvars { sender: MacosEventSender<Event> }

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ButtonRouterIvars]
    struct ButtonRouter;

    unsafe impl NSObjectProtocol for ButtonRouter {}

    impl ButtonRouter {
        #[unsafe(method(openProject:))]
        fn open_project(&self, _: &AnyObject) { self.ivars().sender.dispatch(Event::ChooseProjectFolder); }
        #[unsafe(method(showNotes:))]
        fn show_notes(&self, _: &AnyObject) { self.ivars().sender.dispatch(Event::Navigate(Route::Notes)); }
        #[unsafe(method(showProject:))]
        fn show_project(&self, _: &AnyObject) { self.ivars().sender.dispatch(Event::Navigate(Route::Project)); }
        #[unsafe(method(showWeather:))]
        fn show_weather(&self, _: &AnyObject) { self.ivars().sender.dispatch(Event::Navigate(Route::Weather)); }
    }
);

impl ButtonRouter {
    fn new(main_thread: MainThreadMarker, sender: MacosEventSender<Event>) -> Retained<Self> {
        let this = Self::alloc(main_thread).set_ivars(ButtonRouterIvars { sender });
        // SAFETY: NSObject's `init` has its declared Objective-C signature.
        unsafe { msg_send![super(this), init] }
    }
}

fn native_button(main_thread: MainThreadMarker, title: &str, target: &ButtonRouter, action: objc2::runtime::Sel) -> Retained<NSButton> {
    // SAFETY: `ButtonRouter` remains retained by `NotebookHost` and selectors above match its methods.
    unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str(title), Some(&**target), Some(action), main_thread) }
}
