//! Portable state and effects for the Project Notebook integration example.

use std::path::PathBuf;

use ferro_contracts::Document;
use serde::{Deserialize, Serialize};

/// Shared application entry point.
pub struct ProjectNotebook;

/// A locally persisted note.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Note {
    /// Stable document identifier.
    pub id: String,
    /// User-facing title.
    pub title: String,
    /// Note body.
    pub body: String,
}

impl Document for Note {
    fn id(&self) -> &str { &self.id }
}

/// Navigation destination rendered by the native host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Route {
    /// Browse and edit local notes.
    Notes,
    /// Inspect or attach the local project folder.
    Project,
    /// Inspect the project's weather service.
    Weather,
}

/// UI state owned by the portable reducer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppState {
    /// Selected section.
    pub route: Route,
    /// Attached project folder.
    pub project_root: Option<PathBuf>,
    /// Loaded notes.
    pub notes: Vec<Note>,
    /// Note currently edited by the native form.
    pub draft: Option<Note>,
    /// Monotonic local identifier used for newly created notes.
    pub next_note_id: u64,
    /// Status suitable for a native label.
    pub status: String,
}

/// Events from native controls, capabilities, and services.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// `AppKit` finished launching.
    Launched,
    /// The user selected a navigation section.
    Navigate(Route),
    /// The user asked to attach a project folder.
    ChooseProjectFolder,
    /// Folder panel completion; cancellation is intentionally a no-op.
    ProjectFolderChosen(Option<PathBuf>),
    /// Repository load finished.
    NotesLoaded(Result<Vec<Note>, String>),
    /// Start a new editable note.
    NewNote,
    /// Replace the draft title from a native text field.
    UpdateDraftTitle(String),
    /// Replace the draft body from a native text field.
    UpdateDraftBody(String),
    /// Persist the current draft.
    SaveDraft,
    /// Persistence completion from the native host.
    NoteSaved(Result<Note, String>),
}

/// Work delegated to the macOS host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Present the native folder panel.
    ChooseProjectFolder,
    /// Open the `LiliaDB` repository below this folder.
    LoadNotes(PathBuf),
    /// Persist a note through the selected project's document repository.
    SaveNote(Note),
}

impl ferro_sdk::Application for ProjectNotebook {
    type State = AppState;
    type Event = Event;
    type Effect = Effect;

    fn descriptor() -> ferro_sdk::ApplicationDescriptor {
        ferro_sdk::ApplicationDescriptor::new("Project Notebook", "dev.ferro.project-notebook")
    }

    fn initial_state() -> Self::State {
        AppState { route: Route::Notes, project_root: None, notes: Vec::new(), draft: None, next_note_id: 1, status: "Choose a project folder to begin".to_owned() }
    }

    fn reduce(state: &mut Self::State, event: Self::Event) -> Vec<Self::Effect> {
        match event {
            Event::Launched | Event::ProjectFolderChosen(None) => Vec::new(),
            Event::Navigate(route) => { state.route = route; Vec::new() }
            Event::ChooseProjectFolder => vec![Effect::ChooseProjectFolder],
            Event::ProjectFolderChosen(Some(path)) => {
                state.status = format!("Opening {}", path.display());
                state.project_root = Some(path.clone());
                vec![Effect::LoadNotes(path)]
            }
            Event::NotesLoaded(Ok(notes)) => { state.notes = notes; state.status = format!("{} note(s) loaded", state.notes.len()); Vec::new() }
            Event::NotesLoaded(Err(error)) => { state.status = format!("Could not load notes: {error}"); Vec::new() }
            Event::NewNote => {
                state.draft = Some(Note { id: format!("note-{}", state.next_note_id), title: "Untitled note".to_owned(), body: String::new() });
                state.next_note_id += 1;
                "Editing a new note".clone_into(&mut state.status);
                Vec::new()
            }
            Event::UpdateDraftTitle(title) => { if let Some(note) = &mut state.draft { note.title = title; } Vec::new() }
            Event::UpdateDraftBody(body) => { if let Some(note) = &mut state.draft { note.body = body; } Vec::new() }
            Event::SaveDraft => match (&state.project_root, &state.draft) {
                (Some(_), Some(note)) => vec![Effect::SaveNote(note.clone())],
                (None, _) => { "Choose a project folder before saving".clone_into(&mut state.status); Vec::new() }
                (_, None) => { "Create a note before saving".clone_into(&mut state.status); Vec::new() }
            },
            Event::NoteSaved(Ok(note)) => {
                if let Some(existing) = state.notes.iter_mut().find(|item| item.id == note.id) { *existing = note.clone(); } else { state.notes.push(note.clone()); }
                state.draft = Some(note);
                "Note saved locally".clone_into(&mut state.status);
                Vec::new()
            }
            Event::NoteSaved(Err(error)) => { state.status = format!("Could not save note: {error}"); Vec::new() }
        }
    }
}

#[cfg(test)]
mod tests {
    use ferro_sdk::Application;
    use super::{Effect, Event, ProjectNotebook, Route};

    #[test]
    fn cancellation_leaves_project_state_unchanged() {
        let mut state = ProjectNotebook::initial_state();
        assert_eq!(
            ProjectNotebook::reduce(&mut state, Event::ProjectFolderChosen(None)),
            Vec::<Effect>::new()
        );
        assert!(state.project_root.is_none());
    }

    #[test]
    fn selected_folder_loads_notes_and_navigation_is_portable() {
        let mut state = ProjectNotebook::initial_state();
        let path = std::path::PathBuf::from("/tmp/notebook");
        assert_eq!(ProjectNotebook::reduce(&mut state, Event::ProjectFolderChosen(Some(path.clone()))), vec![Effect::LoadNotes(path)]);
        ProjectNotebook::reduce(&mut state, Event::Navigate(Route::Weather));
        assert_eq!(state.route, Route::Weather);
    }

    #[test]
    fn draft_changes_remain_pure_until_save_is_requested() {
        let mut state = ProjectNotebook::initial_state();
        ProjectNotebook::reduce(&mut state, Event::NewNote);
        ProjectNotebook::reduce(&mut state, Event::UpdateDraftTitle("Plan".to_owned()));
        assert_eq!(state.draft.as_ref().map(|note| note.title.as_str()), Some("Plan"));
        assert_eq!(ProjectNotebook::reduce(&mut state, Event::SaveDraft), Vec::<Effect>::new());
        assert!(state.status.contains("Choose a project folder"));
    }
}
