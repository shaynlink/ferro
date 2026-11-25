//! Shared presentation state, deliberately separate from platform widgets.

use ferro_domain::Workspace;
use ferro_sdk::Application;

/// Single-threaded application state backed by a portable [`Application`] reducer.
///
/// Native runtimes own a store on their UI thread and render after every successful dispatch.
#[derive(Debug)]
pub struct Store<A: Application> {
    state: A::State,
}

impl<A: Application> Store<A> {
    /// Creates a store with the application's initial state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: A::initial_state(),
        }
    }

    /// Returns the current state snapshot.
    #[must_use]
    pub fn state(&self) -> &A::State {
        &self.state
    }

    /// Applies an event and returns ordered effects for the active target runtime.
    pub fn dispatch(&mut self, event: A::Event) -> Vec<A::Effect> {
        A::reduce(&mut self.state, event)
    }
}

impl<A: Application> Default for Store<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// A user intent emitted by a platform-native view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppIntent {
    /// The user selected a known workspace.
    OpenWorkspace(Workspace),
    /// The user asked to choose a new workspace from a native file dialog.
    ChooseWorkspace,
}

/// State common to the native desktop shells.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DesktopState {
    /// The workspaces currently shown by the shell.
    pub recent_workspaces: Vec<Workspace>,
}

#[cfg(test)]
mod tests {
    use ferro_sdk::{Application, ApplicationDescriptor};

    use super::Store;

    struct Counter;

    impl Application for Counter {
        type State = usize;
        type Event = usize;
        type Effect = &'static str;

        fn descriptor() -> ApplicationDescriptor {
            ApplicationDescriptor::new("Counter", "dev.ferro.counter")
        }

        fn initial_state() -> Self::State {
            1
        }

        fn reduce(state: &mut Self::State, event: Self::Event) -> Vec<Self::Effect> {
            *state += event;
            vec!["persist", "render"]
        }
    }

    #[test]
    fn dispatches_in_order_and_keeps_the_shared_state() {
        let mut store = Store::<Counter>::new();
        assert_eq!(*store.state(), 1);
        assert_eq!(store.dispatch(2), ["persist", "render"]);
        assert_eq!(*store.state(), 3);
    }
}
