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
pub enum Route { Notes, Project, Weather }

/// UI state owned by the portable reducer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppState {
    /// Selected section.
    pub route: Route,
    /// Attached project folder.
    pub project_root: Option<PathBuf>,
    /// Loaded notes.
    pub notes: Vec<Note>,
    /// Status suitable for a native label.
    pub status: String,
}

/// Events from native controls, capabilities, and services.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// AppKit finished launching.
    Launched,
    /// The user selected a navigation section.
    Navigate(Route),
    /// The user asked to attach a project folder.
    ChooseProjectFolder,
    /// Folder panel completion; cancellation is intentionally a no-op.
    ProjectFolderChosen(Option<PathBuf>),
    /// Repository load finished.
    NotesLoaded(Result<Vec<Note>, String>),
}

/// Work delegated to the macOS host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Present the native folder panel.
    ChooseProjectFolder,
    /// Open the LiliaDB repository below this folder.
    LoadNotes(PathBuf),
}

impl ferro_sdk::Application for ProjectNotebook {
    type State = AppState;
    type Event = Event;
    type Effect = Effect;

    fn descriptor() -> ferro_sdk::ApplicationDescriptor {
        ferro_sdk::ApplicationDescriptor::new("Project Notebook", "dev.ferro.project-notebook")
    }

    fn initial_state() -> Self::State {
        AppState { route: Route::Notes, project_root: None, notes: Vec::new(), status: "Choose a project folder to begin".to_owned() }
    }

    fn reduce(state: &mut Self::State, event: Self::Event) -> Vec<Self::Effect> {
        match event {
            Event::Launched => Vec::new(),
            Event::Navigate(route) => { state.route = route; Vec::new() }
            Event::ChooseProjectFolder => vec![Effect::ChooseProjectFolder],
            Event::ProjectFolderChosen(Some(path)) => {
                state.status = format!("Opening {}", path.display());
                state.project_root = Some(path.clone());
                vec![Effect::LoadNotes(path)]
            }
            Event::ProjectFolderChosen(None) => Vec::new(),
            Event::NotesLoaded(Ok(notes)) => { state.notes = notes; state.status = format!("{} note(s) loaded", state.notes.len()); Vec::new() }
            Event::NotesLoaded(Err(error)) => { state.status = format!("Could not load notes: {error}"); Vec::new() }
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
        assert!(ProjectNotebook::reduce(&mut state, Event::ProjectFolderChosen(None)).is_empty());
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
}
