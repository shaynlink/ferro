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
    title_field: OnceCell<Retained<NSTextField>>,
    body_field: OnceCell<Retained<NSTextField>>,
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
        let title_field = editable_field(main_thread, &router, sel!(editTitle:), 32.0, 132.0, "Untitled note");
        let body_field = editable_field(main_thread, &router, sel!(editBody:), 32.0, 92.0, "");
        let new_note = native_button(main_thread, "New Note", &router, sel!(newNote:));
        new_note.setFrame(NSRect::new(NSPoint::new(470.0, 132.0), NSSize::new(110.0, 28.0)));
        let save_note = native_button(main_thread, "Save Note", &router, sel!(saveNote:));
        save_note.setFrame(NSRect::new(NSPoint::new(590.0, 132.0), NSSize::new(110.0, 28.0)));
        content.addSubview(&title);
        content.addSubview(&status);
        content.addSubview(&detail);
        content.addSubview(&open);
        content.addSubview(&notes);
        content.addSubview(&project);
        content.addSubview(&weather);
        content.addSubview(&title_field);
        content.addSubview(&body_field);
        content.addSubview(&new_note);
        content.addSubview(&save_note);
        self.status.set(status).expect("host builds once");
        self.detail.set(detail).expect("host builds once");
        self.title_field.set(title_field).expect("host builds once");
        self.body_field.set(body_field).expect("host builds once");
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
        if let Some(note) = &state.draft {
            if let Some(field) = self.title_field.get()
                && field.stringValue().to_string() != note.title {
                field.setStringValue(&NSString::from_str(&note.title));
            }
            if let Some(field) = self.body_field.get()
                && field.stringValue().to_string() != note.body {
                field.setStringValue(&NSString::from_str(&note.body));
            }
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
            Effect::SaveNote(note) => {
                let result = self.repository.borrow_mut().as_mut()
                    .ok_or_else(|| "Choose a project folder before saving".to_owned())
                    .and_then(|repository| repository.upsert(&note).map(|()| note).map_err(|error| error.to_string()));
                self.sender.get().expect("host is built before effects").dispatch(Event::NoteSaved(result));
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
        #[unsafe(method(newNote:))]
        fn new_note(&self, _: &AnyObject) { self.ivars().sender.dispatch(Event::NewNote); }
        #[unsafe(method(saveNote:))]
        fn save_note(&self, _: &AnyObject) { self.ivars().sender.dispatch(Event::SaveDraft); }
        #[unsafe(method(editTitle:))]
        fn edit_title(&self, field: &NSTextField) { self.ivars().sender.dispatch(Event::UpdateDraftTitle(field.stringValue().to_string())); }
        #[unsafe(method(editBody:))]
        fn edit_body(&self, field: &NSTextField) { self.ivars().sender.dispatch(Event::UpdateDraftBody(field.stringValue().to_string())); }
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

fn editable_field(main_thread: MainThreadMarker, target: &ButtonRouter, action: objc2::runtime::Sel, x: f64, y: f64, value: &str) -> Retained<NSTextField> {
    let field = NSTextField::initWithFrame(NSTextField::alloc(main_thread), NSRect::new(NSPoint::new(x, y), NSSize::new(420.0, 28.0)));
    field.setStringValue(&NSString::from_str(value));
    // SAFETY: `ButtonRouter` remains retained by `NotebookHost` for the field lifetime.
    unsafe { field.setTarget(Some(&**target)); field.setAction(Some(action)); }
    field
}
