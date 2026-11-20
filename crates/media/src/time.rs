//! Monotonic time for the pipeline: the time value, the clock trait and its system and test implementations.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

/// How often a waiting clock re-checks its interrupt predicate.
///
/// Only the latency of noticing an interruption depends on this value, never
/// the outcome of a wait.
const POLL_SLICE: Duration = Duration::from_millis(2);

/// A point on one monotonic clock, in nanoseconds since that clock's origin.
///
/// Values from different clocks are not comparable; a pipeline shares one
/// [`Clock`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonoTime(u64);

impl MonoTime {
    /// The origin of a clock.
    pub const ZERO: Self = Self(0);

    /// A time `nanos` nanoseconds after the origin.
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// A time `millis` milliseconds after the origin (saturating).
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis.saturating_mul(1_000_000))
    }

    /// Nanoseconds since the origin.
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// This time shifted forward by `d` (saturating).
    pub fn saturating_add(self, d: Duration) -> Self {
        let nanos = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
        Self(self.0.saturating_add(nanos))
    }

    /// The time elapsed since `earlier`; zero when `earlier` is not earlier.
    pub fn saturating_since(self, earlier: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }
}

/// A source of [`MonoTime`] that tests can replace.
///
/// Pipeline code never reads the system time directly (CS-4.4.2): it takes a
/// `Clock`, so that timing rules are tested without sleeping.
pub trait Clock: Send + Sync {
    /// The current time.
    fn now(&self) -> MonoTime;

    /// Block the calling thread until [`Clock::now`] reaches `deadline`.
    ///
    /// `interrupted` is polled while waiting (for example to watch a
    /// cancellation token). Returns `true` when the deadline was reached and
    /// `false` when the wait was interrupted first; an interruption wins over
    /// a deadline that is also due.
    fn sleep_until(&self, deadline: MonoTime, interrupted: &dyn Fn() -> bool) -> bool;
}

impl<C: Clock + ?Sized> Clock for Arc<C> {
    fn now(&self) -> MonoTime {
        (**self).now()
    }

    fn sleep_until(&self, deadline: MonoTime, interrupted: &dyn Fn() -> bool) -> bool {
        (**self).sleep_until(deadline, interrupted)
    }
}

/// The real monotonic clock; its origin is the moment it was created.
#[derive(Clone, Copy, Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// A clock whose origin is now.
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> MonoTime {
        MonoTime(u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX))
    }

    fn sleep_until(&self, deadline: MonoTime, interrupted: &dyn Fn() -> bool) -> bool {
        loop {
            if interrupted() {
                return false;
            }
            let now = self.now();
            if now >= deadline {
                return true;
            }
            std::thread::sleep(deadline.saturating_since(now).min(POLL_SLICE));
        }
    }
}

#[derive(Debug, Default)]
struct ManualShared {
    now: Mutex<u64>,
    changed: Condvar,
}

/// A clock that only moves when a test calls [`ManualClock::advance`].
///
/// Clones share one time. Threads blocked in [`Clock::sleep_until`] wake as
/// soon as an advance reaches their deadline.
#[derive(Clone, Debug, Default)]
pub struct ManualClock {
    shared: Arc<ManualShared>,
}

impl ManualClock {
    /// A clock standing at [`MonoTime::ZERO`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Move the clock forward by `d` and wake every waiting thread.
    pub fn advance(&self, d: Duration) {
        let mut now = self.shared.now.lock();
        let nanos = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
        *now = now.saturating_add(nanos);
        drop(now);
        self.shared.changed.notify_all();
    }
}

impl Clock for ManualClock {
    fn now(&self) -> MonoTime {
        MonoTime(*self.shared.now.lock())
    }

    fn sleep_until(&self, deadline: MonoTime, interrupted: &dyn Fn() -> bool) -> bool {
        let mut now = self.shared.now.lock();
        loop {
            if interrupted() {
                return false;
            }
            if MonoTime(*now) >= deadline {
                return true;
            }
            // Woken by `advance`; the timeout only bounds how late an
            // interruption (which cannot notify this condvar) is noticed.
            let _ = self.shared.changed.wait_for(&mut now, POLL_SLICE);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use super::*;

    #[test]
    fn mono_time_arithmetic_saturates() {
        let t = MonoTime::from_millis(5);
        assert_eq!(t.as_nanos(), 5_000_000);
        assert_eq!(
            t.saturating_add(Duration::from_millis(3)),
            MonoTime::from_millis(8)
        );
        assert_eq!(
            MonoTime::from_millis(8).saturating_since(t),
            Duration::from_millis(3)
        );
        assert_eq!(t.saturating_since(MonoTime::from_millis(8)), Duration::ZERO);
        assert_eq!(
            MonoTime::from_nanos(u64::MAX)
                .saturating_add(Duration::from_secs(1))
                .as_nanos(),
            u64::MAX
        );
        assert!(MonoTime::ZERO < t);
    }

    #[test]
    fn manual_clock_moves_only_when_advanced_and_clones_share_time() {
        let clock = ManualClock::new();
        let other = clock.clone();
        assert_eq!(clock.now(), MonoTime::ZERO);
        clock.advance(Duration::from_millis(250));
        assert_eq!(other.now(), MonoTime::from_millis(250));
        other.advance(Duration::from_millis(50));
        assert_eq!(clock.now(), MonoTime::from_millis(300));
    }

    #[test]
    fn manual_sleep_returns_at_once_when_the_deadline_has_passed() {
        let clock = ManualClock::new();
        clock.advance(Duration::from_secs(1));
        assert!(clock.sleep_until(MonoTime::from_millis(500), &|| false));
    }

    #[test]
    fn manual_sleep_wakes_when_the_clock_reaches_the_deadline() {
        let clock = ManualClock::new();
        let waiter = {
            let clock = clock.clone();
            thread::spawn(move || clock.sleep_until(MonoTime::from_millis(100), &|| false))
        };
        clock.advance(Duration::from_millis(60));
        clock.advance(Duration::from_millis(60));
        assert!(waiter.join().expect("waiter thread"));
    }

    #[test]
    fn manual_sleep_is_interrupted_and_interruption_wins_over_a_due_deadline() {
        let clock = ManualClock::new();
        let stop = Arc::new(AtomicBool::new(false));
        let waiter = {
            let (clock, stop) = (clock.clone(), Arc::clone(&stop));
            thread::spawn(move || {
                clock.sleep_until(MonoTime::from_millis(100), &|| stop.load(Ordering::SeqCst))
            })
        };
        stop.store(true, Ordering::SeqCst);
        assert!(!waiter.join().expect("waiter thread"));
        assert!(!clock.sleep_until(MonoTime::ZERO, &|| true));
    }

    #[test]
    fn system_clock_is_monotonic_and_honours_past_deadlines() {
        let clock = SystemClock::new();
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
        assert!(clock.sleep_until(MonoTime::ZERO, &|| false));
        assert!(!clock.sleep_until(MonoTime::from_nanos(u64::MAX), &|| true));
    }

    #[test]
    fn a_shared_clock_behind_arc_dyn_works() {
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new());
        assert_eq!(clock.now(), MonoTime::ZERO);
        assert!(clock.sleep_until(MonoTime::ZERO, &|| false));
    }
}
