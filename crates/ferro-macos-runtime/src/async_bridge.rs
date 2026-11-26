//! Thread-safe event injection bridge between worker threads and the `AppKit` main run loop.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dispatch2::DispatchQueue;
use ferro_sdk::{AppEventProxy, EventSink, FerroError, FerroErrorKind};

thread_local! {
    static MAIN_FLUSHER: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

pub(crate) fn register_main_flusher(flusher: impl FnMut() + 'static) {
    MAIN_FLUSHER.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(flusher));
    });
}

pub(crate) fn clear_main_flusher() {
    MAIN_FLUSHER.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

pub(crate) fn invoke_main_flusher() {
    MAIN_FLUSHER.with(|cell| {
        if let Ok(mut guard) = cell.try_borrow_mut()
            && let Some(flusher) = guard.as_mut()
        {
            flusher();
        }
    });
}

/// Shared queue backing [`AppEventProxy`] on macOS.
pub(crate) struct MacosEventQueue<E> {
    events: Mutex<VecDeque<E>>,
    alive: AtomicBool,
    wakeup_scheduled: AtomicBool,
}

impl<E> MacosEventQueue<E> {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(VecDeque::new()),
            alive: AtomicBool::new(true),
            wakeup_scheduled: AtomicBool::new(false),
        })
    }

    pub(crate) fn shutdown(&self) {
        self.alive.store(false, Ordering::Release);
    }

    pub(crate) fn drain(&self) -> Vec<E> {
        self.wakeup_scheduled.store(false, Ordering::Release);
        self.events
            .lock()
            .map_or_default(|mut queue| queue.drain(..).collect())
    }

    fn push(&self, event: E) -> Result<bool, FerroError> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(FerroError::new(
                FerroErrorKind::Runtime,
                "macOS runtime has terminated",
            ));
        }
        let mut queue = self.events.lock().map_err(|_| {
            FerroError::new(FerroErrorKind::Runtime, "macOS event queue lock poisoned")
        })?;
        queue.push_back(event);
        Ok(!self.wakeup_scheduled.swap(true, Ordering::AcqRel))
    }
}

struct MacosEventSink<E> {
    queue: Arc<MacosEventQueue<E>>,
}

impl<E: Send + Sync + 'static> EventSink<E> for MacosEventSink<E> {
    fn send(&self, event: E) -> Result<(), FerroError> {
        let should_wake = self.queue.push(event)?;
        if should_wake {
            DispatchQueue::main().exec_async(|| {
                invoke_main_flusher();
            });
        }
        Ok(())
    }
}

pub(crate) fn create_event_proxy<E: Send + Sync + 'static>(
    queue: &Arc<MacosEventQueue<E>>,
) -> AppEventProxy<E> {
    AppEventProxy::new(Arc::new(MacosEventSink {
        queue: Arc::clone(queue),
    }))
}

#[cfg(test)]
mod tests {
    use super::MacosEventQueue;

    #[test]
    fn coalesces_wakeups_until_drained() {
        let queue = MacosEventQueue::new();
        assert_eq!(queue.push(1), Ok(true));
        assert_eq!(queue.push(2), Ok(false));
        assert_eq!(queue.drain(), vec![1, 2]);
        assert_eq!(queue.push(3), Ok(true));
        assert_eq!(queue.drain(), vec![3]);
    }

    #[test]
    fn rejects_events_after_shutdown() {
        let queue = MacosEventQueue::<i32>::new();
        queue.shutdown();
        assert!(queue.push(10).is_err());
    }
}
