//! Thread-safe event injection primitives for async tasks and background workers.

use std::sync::{Arc, Mutex};

use crate::{FerroError, FerroErrorKind};

/// Thread-safe sink allowing background tasks to push events into a Ferro UI runtime.
pub trait EventSink<E>: Send + Sync {
    /// Enqueues `event` and schedules processing on the runtime's UI thread.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] with [`FerroErrorKind::Runtime`] when the runtime is no longer
    /// accepting events.
    fn send(&self, event: E) -> Result<(), FerroError>;
}

/// Cloneable, `Send + Sync` handle for dispatching application events from worker threads.
pub struct AppEventProxy<E> {
    sink: Arc<dyn EventSink<E>>,
}

impl<E> Clone for AppEventProxy<E> {
    fn clone(&self) -> Self {
        Self {
            sink: Arc::clone(&self.sink),
        }
    }
}

impl<E> AppEventProxy<E> {
    /// Wraps a runtime-supplied [`EventSink`].
    #[must_use]
    pub fn new(sink: Arc<dyn EventSink<E>>) -> Self {
        Self { sink }
    }

    /// Enqueues an event for delivery on the UI runtime thread.
    ///
    /// # Errors
    ///
    /// Returns [`FerroError`] when the owning runtime has terminated or rejected delivery.
    pub fn send(&self, event: E) -> Result<(), FerroError> {
        self.sink.send(event)
    }
}

impl<E: Send + Sync + 'static> AppEventProxy<E> {
    /// Creates an in-memory recording proxy useful in unit tests without a native UI runtime.
    #[must_use]
    pub fn memory() -> (Self, Arc<Mutex<Vec<E>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::new(MemoryEventSink {
            events: Arc::clone(&log),
        });
        (Self::new(sink), log)
    }
}

struct MemoryEventSink<E> {
    events: Arc<Mutex<Vec<E>>>,
}

impl<E: Send + Sync> EventSink<E> for MemoryEventSink<E> {
    fn send(&self, event: E) -> Result<(), FerroError> {
        let mut guard = self.events.lock().map_err(|_| {
            FerroError::new(FerroErrorKind::Runtime, "memory event sink lock poisoned")
        })?;
        guard.push(event);
        Ok(())
    }
}
