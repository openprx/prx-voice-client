//! Polling watcher of an application's call activity: reads a source on its own thread and emits debounced change events.
//!
//! Runs on a normal thread, never a real-time one (the Core Audio property
//! reads may take milliseconds). The decision logic is pure and clock-injected:
//! [`Debouncer`] holds a change back until it has persisted for the hold time,
//! and [`WatcherCore::tick`] is one poll. The thread only calls `tick` at the
//! poll interval. No coordinator lives here: events go to a bounded channel the
//! coordinator reads later (pipeline-design CT-8).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use thiserror::Error;

use super::judge::{CallJudgement, judge_call};
use super::model::AppAudioActivity;

/// Capacity of the event channel. Events are changes of state, so a consumer
/// that keeps up never fills it; a full channel drops the new event, counts it
/// and the consumer recovers the state from [`CallActivityWatcher::latest`].
const EVENT_CAPACITY: usize = 16;

/// Why the activity could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ActivityError {
    /// The audio system has no process objects (before macOS 14.4) or this is not macOS.
    #[error("reading an application's audio activity needs macOS 14.4 or later")]
    Unsupported,
    /// A Core Audio read failed.
    #[error("reading the audio process list failed (status {status})")]
    ReadFailed {
        /// The `OSStatus`.
        status: i32,
    },
}

/// Where activity comes from: Core Audio in production, a script in tests.
pub trait ActivitySource: Send {
    /// Read the application's activity now.
    ///
    /// # Errors
    /// [`ActivityError`] when the audio system cannot be read.
    fn read(&mut self) -> Result<AppAudioActivity, ActivityError>;
}

/// A change of the debounced judgement, or the loss of detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallActivityEvent {
    /// The judgement changed (the first event reports the initial state).
    Changed {
        /// The new judgement.
        judgement: CallJudgement,
        /// The record it was made from.
        activity: AppAudioActivity,
    },
    /// Detection is not possible; the coordinator shows "no detection" (Degraded).
    Unavailable {
        /// Why.
        error: ActivityError,
    },
}

/// Holds a change of value back until it has persisted for the hold time.
///
/// The first value is reported at once (the baseline); after that a different
/// value is reported when it has been seen continuously for `hold`. A value that
/// flips back before then is never reported.
#[derive(Debug, Clone)]
pub struct Debouncer<T> {
    hold: Duration,
    reported: Option<T>,
    candidate: Option<(T, Instant)>,
}

impl<T: Clone + PartialEq> Debouncer<T> {
    /// A debouncer with the given hold time.
    pub fn new(hold: Duration) -> Self {
        Self {
            hold,
            reported: None,
            candidate: None,
        }
    }

    /// Feed the value seen at `now`; returns it when it becomes the reported one.
    pub fn feed(&mut self, now: Instant, value: T) -> Option<T> {
        if self.reported.as_ref() == Some(&value) {
            self.candidate = None;
            return None;
        }
        let since = match &self.candidate {
            Some((c, since)) if *c == value => *since,
            _ => now,
        };
        if self.reported.is_none() || now.saturating_duration_since(since) >= self.hold {
            self.reported = Some(value.clone());
            self.candidate = None;
            return Some(value);
        }
        self.candidate = Some((value, since));
        None
    }
}

/// One poll of a source: read, judge, debounce.
pub struct WatcherCore {
    source: Box<dyn ActivitySource>,
    our_microphone_uid: String,
    debouncer: Debouncer<Result<CallJudgement, ActivityError>>,
}

impl WatcherCore {
    /// A core reading `source` and judging against `our_microphone_uid`.
    pub fn new(
        source: Box<dyn ActivitySource>,
        our_microphone_uid: impl Into<String>,
        hold: Duration,
    ) -> Self {
        Self {
            source,
            our_microphone_uid: our_microphone_uid.into(),
            debouncer: Debouncer::new(hold),
        }
    }

    /// Poll once at `now`; returns an event when the debounced state changed.
    pub fn tick(&mut self, now: Instant) -> Option<CallActivityEvent> {
        let (observed, activity) = match self.source.read() {
            Ok(activity) => (
                Ok(judge_call(&activity, &self.our_microphone_uid)),
                Some(activity),
            ),
            Err(error) => (Err(error), None),
        };
        match (self.debouncer.feed(now, observed)?, activity) {
            (Ok(judgement), Some(activity)) => Some(CallActivityEvent::Changed {
                judgement,
                activity,
            }),
            (Err(error), _) => Some(CallActivityEvent::Unavailable { error }),
            // A confirmed judgement always comes from a successful read.
            (Ok(_), None) => None,
        }
    }
}

/// Settings of the watcher thread.
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// UID of our virtual microphone.
    pub our_microphone_uid: String,
    /// Time between polls (pipeline-design PD-4.1.2: once per second).
    pub poll_interval: Duration,
    /// How long a change must persist before it is reported.
    pub hold: Duration,
}

impl WatcherConfig {
    /// Once-per-second polls and a hold of one second (two agreeing polls).
    pub fn new(our_microphone_uid: impl Into<String>) -> Self {
        Self {
            our_microphone_uid: our_microphone_uid.into(),
            poll_interval: Duration::from_secs(1),
            hold: Duration::from_secs(1),
        }
    }
}

/// The watcher thread. Stops and joins on drop.
pub struct CallActivityWatcher {
    events: Receiver<CallActivityEvent>,
    latest: Arc<Mutex<Option<CallActivityEvent>>>,
    dropped: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CallActivityWatcher {
    /// Start polling `source`.
    ///
    /// # Errors
    /// The operating-system error when the thread cannot be started.
    pub fn spawn(source: Box<dyn ActivitySource>, config: WatcherConfig) -> std::io::Result<Self> {
        let (tx, events) = sync_channel(EVENT_CAPACITY);
        let latest = Arc::new(Mutex::new(None));
        let dropped = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let core = WatcherCore::new(source, config.our_microphone_uid, config.hold);
        let shared = Shared {
            tx,
            latest: Arc::clone(&latest),
            dropped: Arc::clone(&dropped),
            stop: Arc::clone(&stop),
        };
        let interval = config.poll_interval;
        let thread = std::thread::Builder::new()
            .name("prxvc-call-activity".into())
            .spawn(move || run(core, &shared, interval))?;
        Ok(Self {
            events,
            latest,
            dropped,
            stop,
            thread: Some(thread),
        })
    }

    /// Change events since the last read.
    pub fn events(&self) -> &Receiver<CallActivityEvent> {
        &self.events
    }

    /// The most recent event, the source of truth when the channel overflowed.
    pub fn latest(&self) -> Option<CallActivityEvent> {
        self.latest.lock().clone()
    }

    /// Events dropped because the channel was full.
    pub fn dropped_events(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Drop for CallActivityWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join(); // a panicked watcher has nothing left to release
        }
    }
}

struct Shared {
    tx: SyncSender<CallActivityEvent>,
    latest: Arc<Mutex<Option<CallActivityEvent>>>,
    dropped: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

fn run(mut core: WatcherCore, shared: &Shared, interval: Duration) {
    while !shared.stop.load(Ordering::Acquire) {
        if let Some(event) = core.tick(Instant::now()) {
            *shared.latest.lock() = Some(event.clone());
            match shared.tx.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    shared.dropped.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
        std::thread::park_timeout(interval);
    }
}
