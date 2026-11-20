//! Event log shared by the fakes so that tests can observe the life of every request.

use std::sync::Arc;

use parking_lot::Mutex;

/// What happened to a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// The request was accepted.
    Opened,
    /// A blocking call has fixed its due time and begun to wait (recognition
    /// `finalize` and refine calls). A test that advances the clock waits for
    /// this first, so the wait cannot start after the advance.
    Started,
    /// An item (delta, chunk or partial) was delivered.
    Item,
    /// The request finished normally.
    Completed,
    /// The request ended because its token was cancelled.
    Cancelled,
    /// The request ended with an injected failure.
    Failed,
    /// The stream was dropped before it finished.
    Dropped,
}

/// One entry of an [`EventLog`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeEvent {
    /// Names the request, for example a clause id such as `out#1.0`.
    pub request: String,
    /// What happened.
    pub kind: EventKind,
}

/// A shared, append-only log; clones see the same entries.
#[derive(Clone, Debug, Default)]
pub struct EventLog(Arc<Mutex<Vec<FakeEvent>>>);

impl EventLog {
    /// An empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an entry.
    pub fn record(&self, request: &str, kind: EventKind) {
        self.0.lock().push(FakeEvent {
            request: request.to_owned(),
            kind,
        });
    }

    /// Every entry, oldest first.
    pub fn events(&self) -> Vec<FakeEvent> {
        self.0.lock().clone()
    }

    /// The kinds recorded for one request, oldest first.
    pub fn kinds_for(&self, request: &str) -> Vec<EventKind> {
        self.0
            .lock()
            .iter()
            .filter(|e| e.request == request)
            .map(|e| e.kind)
            .collect()
    }

    /// Block until at least `n` entries of `kind` exist.
    ///
    /// Spins with `yield_now`; it ends as soon as the other threads have made
    /// the progress the test is waiting for and does not depend on wall time.
    pub fn wait_for(&self, kind: EventKind, n: usize) {
        while self.count(kind) < n {
            std::thread::yield_now();
        }
    }

    /// How many entries of `kind` were recorded, over all requests.
    pub fn count(&self, kind: EventKind) -> usize {
        self.0.lock().iter().filter(|e| e.kind == kind).count()
    }
}
