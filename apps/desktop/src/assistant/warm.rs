//! The assistant's engines stay loaded for a short while after a session ends, so that a second click within that time starts at once.
//!
//! The registry frees an engine when the last handle on it goes away, and a session's handles go
//! with the session. A [`WarmCache`] keeps the finished session's handles (and with them the
//! engines) for `assistant.keep_warm_s` seconds. A session that starts within the time takes them
//! back instead of loading again; call mode starting releases them at once (the call needs the
//! memory and loads its own), and so does any change of the settings the handles were built for
//! (the key). The clock is injected, so the tests need no waiting.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_media::{Clock, MonoTime};

/// One kept value, the settings it was built for and when it expires.
struct Entry<T> {
    key: String,
    value: T,
    until: MonoTime,
}

/// A single kept value.
pub(crate) struct WarmCache<T> {
    clock: Arc<dyn Clock>,
    entry: Mutex<Option<Entry<T>>>,
}

impl<T> WarmCache<T> {
    /// An empty cache on `clock`.
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            entry: Mutex::new(None),
        }
    }

    /// Keep `value`, built for `key`, for `keep`. A zero `keep` keeps nothing (the value drops now).
    pub(crate) fn keep(&self, key: String, value: T, keep: Duration) {
        if keep.is_zero() {
            *self.entry.lock() = None;
            return;
        }
        let until = self.clock.now().saturating_add(keep);
        // The previous value (if any) drops after the lock is released, outside the engine's own work.
        let old = self.entry.lock().replace(Entry { key, value, until });
        drop(old);
    }

    /// The kept value when it was built for `key` and has not expired; otherwise whatever is kept
    /// is dropped and `None` is returned.
    pub(crate) fn take(&self, key: &str) -> Option<T> {
        let entry = self.entry.lock().take()?;
        (entry.key == key && self.clock.now() < entry.until).then_some(entry.value)
    }

    /// Drop the kept value if its time is over. Called now and then.
    pub(crate) fn expire(&self) {
        let mut guard = self.entry.lock();
        if guard.as_ref().is_some_and(|e| self.clock.now() >= e.until) {
            let old = guard.take();
            drop(guard);
            drop(old);
        }
    }

    /// Drop the kept value now (call mode starts, memory is needed).
    pub(crate) fn release(&self) {
        let old = self.entry.lock().take();
        drop(old);
    }

    /// Whether something is kept.
    #[cfg(test)]
    pub(crate) fn is_warm(&self) -> bool {
        self.entry.lock().is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use prxvc_media::ManualClock;

    use super::*;

    /// Stands for the engines: counts how many are alive.
    struct Engine(Arc<AtomicUsize>);

    impl Engine {
        fn load(alive: &Arc<AtomicUsize>, loads: &AtomicUsize) -> Self {
            alive.fetch_add(1, Ordering::SeqCst);
            loads.fetch_add(1, Ordering::SeqCst);
            Self(Arc::clone(alive))
        }
    }

    impl Drop for Engine {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn rig() -> (
        ManualClock,
        WarmCache<Engine>,
        Arc<AtomicUsize>,
        AtomicUsize,
    ) {
        let clock = ManualClock::new();
        let cache = WarmCache::new(Arc::new(clock.clone()));
        (clock, cache, Arc::default(), AtomicUsize::new(0))
    }

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn a_second_session_within_the_time_takes_the_engines_back_without_loading() {
        let (clock, cache, alive, loads) = rig();
        cache.keep("k".into(), Engine::load(&alive, &loads), MINUTE);
        assert_eq!(
            alive.load(Ordering::SeqCst),
            1,
            "kept after the session ended"
        );
        clock.advance(Duration::from_secs(59));
        let engine = cache.take("k").expect("still warm");
        assert_eq!(loads.load(Ordering::SeqCst), 1, "no second load");
        assert_eq!(alive.load(Ordering::SeqCst), 1);
        drop(engine);
        assert_eq!(alive.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_engines_are_freed_when_the_time_is_over() {
        let (clock, cache, alive, loads) = rig();
        cache.keep("k".into(), Engine::load(&alive, &loads), MINUTE);
        clock.advance(Duration::from_secs(59));
        cache.expire();
        assert_eq!(alive.load(Ordering::SeqCst), 1);
        clock.advance(Duration::from_secs(2));
        cache.expire();
        assert_eq!(alive.load(Ordering::SeqCst), 0);
        assert!(cache.take("k").is_none());
    }

    #[test]
    fn a_late_take_finds_nothing_and_frees_what_was_kept() {
        let (clock, cache, alive, loads) = rig();
        cache.keep("k".into(), Engine::load(&alive, &loads), MINUTE);
        clock.advance(Duration::from_secs(61));
        assert!(cache.take("k").is_none());
        assert_eq!(alive.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn call_mode_starting_or_other_settings_release_them_at_once() {
        let (_clock, cache, alive, loads) = rig();
        cache.keep("k".into(), Engine::load(&alive, &loads), MINUTE);
        cache.release();
        assert_eq!(alive.load(Ordering::SeqCst), 0);
        cache.keep("k".into(), Engine::load(&alive, &loads), MINUTE);
        assert!(
            cache.take("another voice").is_none(),
            "built for other settings"
        );
        assert_eq!(alive.load(Ordering::SeqCst), 0, "and freed");
    }

    #[test]
    fn a_zero_time_keeps_nothing() {
        let (_clock, cache, alive, loads) = rig();
        cache.keep("k".into(), Engine::load(&alive, &loads), Duration::ZERO);
        assert_eq!(alive.load(Ordering::SeqCst), 0);
        assert!(!cache.is_warm());
    }

    #[test]
    fn keeping_a_newer_session_frees_the_older_one() {
        let (_clock, cache, alive, loads) = rig();
        cache.keep("k".into(), Engine::load(&alive, &loads), MINUTE);
        cache.keep("k".into(), Engine::load(&alive, &loads), MINUTE);
        assert_eq!(alive.load(Ordering::SeqCst), 1);
    }
}
