//! Shared engines: construction on first use, reference counting, retirement after a failure.
//!
//! An engine (the TTS engine, the recognition engine, the HTTP translation client) is expensive,
//! so both directions of a call share one. The registry never owns an engine strongly: it keeps a
//! [`std::sync::Weak`] reference per key, and every provider handle holds a [`Lease`]. The engine
//! is built when the first lease is requested, shared by later requests, and dropped (freeing its
//! GPU memory) when the last lease goes away: leaving call mode is dropping the handles.
//!
//! A lease can report that its engine failed. The failed engine is retired: leases already out
//! keep using it until they are dropped, and the next request builds a fresh one. Nothing here
//! decides to switch to another provider; that belongs to the pipeline.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use crate::problem::BuildError;

struct Shared<T> {
    engine: T,
    failed: AtomicBool,
}

/// A counted reference to a shared engine.
pub struct Lease<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Clone for Lease<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> Lease<T> {
    /// The engine.
    pub fn engine(&self) -> &T {
        &self.shared.engine
    }

    /// Record that the engine failed: the next request for its key builds a new one.
    pub fn report_failure(&self) {
        self.shared.failed.store(true, Ordering::Release);
    }

    /// Whether the engine was reported as failed.
    pub fn is_retired(&self) -> bool {
        self.shared.failed.load(Ordering::Acquire)
    }

    /// Whether two leases are on the same engine.
    pub fn same_engine(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
}

type Cell<T> = Arc<Mutex<Weak<Shared<T>>>>;

/// The engines of one kind, by key.
pub(crate) struct Engines<K, T> {
    cells: Mutex<HashMap<K, Cell<T>>>,
}

impl<K: Eq + Hash, T> Default for Engines<K, T> {
    fn default() -> Self {
        Self {
            cells: Mutex::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash, T> Engines<K, T> {
    /// A lease on the live engine of `key`, building it with `build` when there is none or it
    /// was retired. Requests for one key wait for each other's build; other keys do not.
    pub fn acquire(
        &self,
        key: K,
        build: impl FnOnce() -> Result<T, BuildError>,
    ) -> Result<Lease<T>, BuildError> {
        let cell = Arc::clone(self.cells.lock().entry(key).or_default());
        let mut slot = cell.lock();
        if let Some(shared) = slot.upgrade()
            && !shared.failed.load(Ordering::Acquire)
        {
            return Ok(Lease { shared });
        }
        let shared = Arc::new(Shared {
            engine: build()?,
            failed: AtomicBool::new(false),
        });
        *slot = Arc::downgrade(&shared);
        Ok(Lease { shared })
    }

    /// How many engines are alive (leased and not yet dropped), retired ones included. An engine
    /// that is still being built does not count.
    pub fn live(&self) -> usize {
        self.cells
            .lock()
            .values()
            .filter(|cell| cell.try_lock().is_some_and(|slot| slot.strong_count() > 0))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct Probe(Arc<AtomicUsize>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn build(alive: &Arc<AtomicUsize>, built: &Arc<AtomicUsize>) -> Result<Probe, BuildError> {
        alive.fetch_add(1, Ordering::SeqCst);
        built.fetch_add(1, Ordering::SeqCst);
        Ok(Probe(Arc::clone(alive)))
    }

    #[test]
    fn leases_share_one_engine_and_the_last_drop_frees_it() {
        let (alive, built) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let engines: Engines<&str, Probe> = Engines::default();
        let a = engines.acquire("k", || build(&alive, &built)).unwrap();
        let b = engines.acquire("k", || build(&alive, &built)).unwrap();
        assert!(a.same_engine(&b));
        assert_eq!(built.load(Ordering::SeqCst), 1);
        assert_eq!(engines.live(), 1);
        drop(a);
        assert_eq!(alive.load(Ordering::SeqCst), 1, "still leased by b");
        drop(b);
        assert_eq!(alive.load(Ordering::SeqCst), 0, "freed");
        assert_eq!(engines.live(), 0);
        let _c = engines.acquire("k", || build(&alive, &built)).unwrap();
        assert_eq!(built.load(Ordering::SeqCst), 2, "rebuilt after being freed");
    }

    #[test]
    fn a_failed_engine_is_retired_and_rebuilt_while_old_leases_keep_working() {
        let (alive, built) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let engines: Engines<&str, Probe> = Engines::default();
        let old = engines.acquire("k", || build(&alive, &built)).unwrap();
        old.report_failure();
        assert!(old.is_retired());
        let fresh = engines.acquire("k", || build(&alive, &built)).unwrap();
        assert!(!fresh.same_engine(&old));
        assert_eq!(alive.load(Ordering::SeqCst), 2);
        drop(old);
        assert_eq!(alive.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_build_leaves_nothing_behind_and_is_retried() {
        let engines: Engines<&str, Probe> = Engines::default();
        let err = engines.acquire("k", || {
            Err(BuildError::Start {
                provider: prxvc_ports::ProviderId::new("p"),
                detail: "boom".into(),
            })
        });
        assert!(err.is_err());
        assert_eq!(engines.live(), 0);
        let (alive, built) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        assert!(engines.acquire("k", || build(&alive, &built)).is_ok());
    }

    #[test]
    fn different_keys_are_different_engines() {
        let (alive, built) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let engines: Engines<&str, Probe> = Engines::default();
        let a = engines.acquire("a", || build(&alive, &built)).unwrap();
        let b = engines.acquire("b", || build(&alive, &built)).unwrap();
        assert!(!a.same_engine(&b));
        assert_eq!(engines.live(), 2);
    }
}
