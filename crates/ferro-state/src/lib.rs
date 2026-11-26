//! Shared presentation state, deliberately separate from platform widgets.

use std::fmt;
use std::time::{Duration, Instant};

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

    /// Creates a store seeded with an explicit state (for example restored after a hot reload).
    #[must_use]
    pub const fn with_state(state: A::State) -> Self {
        Self { state }
    }

    /// Returns the current state snapshot.
    #[must_use]
    pub const fn state(&self) -> &A::State {
        &self.state
    }

    /// Applies an event and returns ordered effects for the active target runtime.
    pub fn dispatch(&mut self, event: A::Event) -> Vec<A::Effect> {
        A::reduce(&mut self.state, event)
    }

    /// Applies an event while measuring reducer execution time.
    pub fn dispatch_timed(&mut self, event: A::Event) -> (Vec<A::Effect>, Duration) {
        let start = Instant::now();
        let effects = A::reduce(&mut self.state, event);
        (effects, start.elapsed())
    }
}

impl<A: Application> Default for Store<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Recorded transition produced by a [`TimeTravelStore`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateTransition<S> {
    /// Zero-based sequential transition index (`0` is the initial state).
    pub sequence: u64,
    /// Human-readable event representation.
    pub event: String,
    /// Human-readable ordered effects produced by this transition.
    pub effects: Vec<String>,
    /// Elapsed reducer execution time in microseconds.
    pub duration_us: u64,
    /// State snapshot immediately after applying the event.
    pub state: S,
}

/// Time-travel capable wrapper around [`Store`] for debugging and deterministic tests.
#[derive(Debug)]
pub struct TimeTravelStore<A: Application> {
    store: Store<A>,
    history: Vec<StateTransition<A::State>>,
    cursor: usize,
    max_history: usize,
    next_sequence: u64,
}

impl<A: Application> TimeTravelStore<A>
where
    A::State: Clone,
    A::Event: fmt::Debug,
    A::Effect: fmt::Debug,
{
    /// Creates a time-travel store retaining up to `max_history` snapshots (minimum 1).
    #[must_use]
    pub fn new(max_history: usize) -> Self {
        let store = Store::<A>::new();
        let initial = StateTransition {
            sequence: 0,
            event: "<initial>".to_owned(),
            effects: Vec::new(),
            duration_us: 0,
            state: store.state().clone(),
        };
        Self {
            store,
            history: vec![initial],
            cursor: 0,
            max_history: max_history.max(1),
            next_sequence: 1,
        }
    }

    /// Returns the currently active state snapshot.
    #[must_use]
    pub const fn state(&self) -> &A::State {
        self.store.state()
    }

    /// Returns all recorded transitions in chronological order.
    #[must_use]
    pub fn history(&self) -> &[StateTransition<A::State>] {
        &self.history
    }

    /// Returns the index in [`Self::history`] representing the currently active state.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Dispatches an event, truncating any forward redo branch if currently rewound.
    pub fn dispatch(&mut self, event: A::Event) -> Vec<A::Effect> {
        if self.cursor + 1 < self.history.len() {
            self.history.truncate(self.cursor + 1);
        }
        let event_label = format!("{event:?}");
        let (effects, elapsed) = self.store.dispatch_timed(event);
        let effect_labels = effects.iter().map(|effect| format!("{effect:?}")).collect();
        let duration_us = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        let transition = StateTransition {
            sequence: self.next_sequence,
            event: event_label,
            effects: effect_labels,
            duration_us,
            state: self.store.state().clone(),
        };
        self.next_sequence += 1;
        self.history.push(transition);
        if self.history.len() > self.max_history {
            let overflow = self.history.len() - self.max_history;
            self.history.drain(..overflow);
        }
        self.cursor = self.history.len().saturating_sub(1);
        effects
    }

    /// Restores the state at `index` within [`Self::history`] without destroying forward history.
    pub fn jump_to(&mut self, index: usize) -> bool {
        if let Some(entry) = self.history.get(index) {
            self.store = Store::with_state(entry.state.clone());
            self.cursor = index;
            true
        } else {
            false
        }
    }

    /// Steps backward by one transition if possible.
    pub fn step_back(&mut self) -> bool {
        if self.cursor == 0 {
            false
        } else {
            self.jump_to(self.cursor - 1)
        }
    }

    /// Steps forward by one transition if possible.
    pub fn step_forward(&mut self) -> bool {
        self.jump_to(self.cursor + 1)
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

    use super::{Store, TimeTravelStore};

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

    #[test]
    fn time_travel_store_records_rewinds_and_branches() {
        let mut store = TimeTravelStore::<Counter>::new(16);
        assert_eq!(*store.state(), 1);
        assert_eq!(store.dispatch(2), ["persist", "render"]);
        assert_eq!(store.dispatch(5), ["persist", "render"]);
        assert_eq!(*store.state(), 8);
        assert_eq!(store.history().len(), 3);

        assert!(store.step_back());
        assert_eq!(*store.state(), 3);
        assert!(store.step_forward());
        assert_eq!(*store.state(), 8);

        assert!(store.jump_to(0));
        assert_eq!(*store.state(), 1);
        store.dispatch(10);
        assert_eq!(*store.state(), 11);
        assert_eq!(store.history().len(), 2);
    }
}
