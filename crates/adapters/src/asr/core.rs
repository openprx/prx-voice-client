//! The shared core of the recogniser: one engine on one worker thread, the queue of work, and the
//! rules that decide what the engine decodes next.
//!
//! * Work is of two kinds. A **job** is a final or refine decode: FIFO, always ahead of partials, and
//!   it preempts a partial pass that is running (the partial is re-requested afterwards). A
//!   **partial pass** is a re-decode of the growing audio of one stream: at most one is wanted or
//!   running per stream, it takes a snapshot of the stream's *latest* audio when it starts (never
//!   audio from when it was requested), and streams with a pass wanted are served round robin, so one
//!   direction cannot starve the other.
//! * A pass that is wanted while the previous pass of the same stream has not finished is not queued:
//!   the stream's `want` flag is already set or its pass is running, and the request is counted as
//!   skipped.

use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use prxvc_media::{Clock, Lang, MonoTime};
use prxvc_ports::AsrError;

use super::adapter::AsrParams;
use super::seam::{EngineError, RecognizeEngine};

/// The result of one partial pass.
#[derive(Debug)]
pub(crate) struct PassResult {
    /// The stream's generation when the pass took its snapshot.
    pub(crate) generation: u64,
    /// Samples of audio the pass decoded.
    pub(crate) samples: usize,
    pub(crate) text: String,
}

/// What a stream shares with the worker.
#[derive(Debug)]
pub(crate) struct SlotShared {
    pub(crate) lang: Lang,
    /// The audio of the open segment.
    pub(crate) audio: Mutex<Vec<f32>>,
    /// The latest finished pass; a newer one replaces an older one that nobody read.
    pub(crate) result: Mutex<Option<Result<PassResult, AsrError>>>,
}

struct SlotEntry {
    shared: Arc<SlotShared>,
    want: bool,
    wanted_at: MonoTime,
    in_flight: bool,
    abort: Arc<AtomicBool>,
    generation: u64,
}

/// A final or refine decode.
pub(crate) struct Job {
    pub(crate) audio: Vec<f32>,
    pub(crate) lang: Lang,
    /// Set by the waiter to give the job up; checked before it starts and while it runs.
    pub(crate) abort: Arc<AtomicBool>,
    pub(crate) reply: mpsc::Sender<Result<String, AsrError>>,
}

struct Running {
    abort: Arc<AtomicBool>,
    is_partial: bool,
}

#[derive(Default)]
struct Sched {
    next_id: u64,
    slots: BTreeMap<u64, SlotEntry>,
    /// The stream served last, for round robin.
    last_served: u64,
    jobs: VecDeque<Job>,
    running: Option<Running>,
}

impl Sched {
    /// The next stream with a pass wanted: the first after the one served last, wrapping around.
    fn pick_partial(&self) -> Option<u64> {
        let wanting = |(_, e): &(&u64, &SlotEntry)| e.want && !e.in_flight;
        self.slots
            .range(self.last_served.saturating_add(1)..)
            .find(wanting)
            .or_else(|| self.slots.iter().find(wanting))
            .map(|(&id, _)| id)
    }
}

/// Counters of the recogniser's life, readable while it runs.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub(crate) partial_passes: AtomicU64,
    pub(crate) aborted_passes: AtomicU64,
    pub(crate) skipped_busy: AtomicU64,
    pub(crate) finals: AtomicU64,
    pub(crate) refines: AtomicU64,
    pub(crate) stable_repairs: AtomicU64,
    pub(crate) final_rewrites: AtomicU64,
    pub(crate) forced_cuts: AtomicU64,
    pub(crate) wait_ns_total: AtomicU64,
    pub(crate) wait_ns_max: AtomicU64,
    pub(crate) decode_ns_total: AtomicU64,
    pub(crate) decode_ns_max: AtomicU64,
}

impl Counters {
    fn record_wait(&self, d: Duration) {
        let ns = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
        self.wait_ns_total.fetch_add(ns, Ordering::Relaxed);
        self.wait_ns_max.fetch_max(ns, Ordering::Relaxed);
    }

    fn record_decode(&self, d: Duration) {
        let ns = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
        self.decode_ns_total.fetch_add(ns, Ordering::Relaxed);
        self.decode_ns_max.fetch_max(ns, Ordering::Relaxed);
    }
}

/// State shared by the provider, its streams and the worker.
pub(crate) struct Core {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) params: AsrParams,
    sched: Mutex<Sched>,
    wake: Condvar,
    pub(crate) counters: Counters,
    shutdown: AtomicBool,
    dead: AtomicBool,
}

enum Work {
    Job(Job),
    Partial {
        id: u64,
        shared: Arc<SlotShared>,
        abort: Arc<AtomicBool>,
        generation: u64,
        waited: Duration,
    },
}

fn unavailable() -> AsrError {
    AsrError::Unavailable {
        reason: "the recogniser engine has stopped".to_owned(),
    }
}

impl Core {
    pub(crate) fn new(clock: Arc<dyn Clock>, params: AsrParams) -> Arc<Self> {
        Arc::new(Self {
            clock,
            params,
            sched: Mutex::new(Sched::default()),
            wake: Condvar::new(),
            counters: Counters::default(),
            shutdown: AtomicBool::new(false),
            dead: AtomicBool::new(false),
        })
    }

    /// Whether the worker has stopped (shutdown or an engine failure).
    pub(crate) fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire)
    }

    pub(crate) fn shut_down(&self) {
        self.shutdown.store(true, Ordering::Release);
        let _guard = self.sched.lock();
        self.wake.notify_all();
    }

    /// Register a stream: its id and what it shares with the worker.
    pub(crate) fn register(&self, lang: Lang) -> (u64, Arc<SlotShared>) {
        let shared = Arc::new(SlotShared {
            lang,
            audio: Mutex::new(Vec::new()),
            result: Mutex::new(None),
        });
        let mut s = self.sched.lock();
        s.next_id += 1;
        let id = s.next_id;
        s.slots.insert(
            id,
            SlotEntry {
                shared: Arc::clone(&shared),
                want: false,
                wanted_at: MonoTime::ZERO,
                in_flight: false,
                abort: Arc::new(AtomicBool::new(false)),
                generation: 0,
            },
        );
        (id, shared)
    }

    /// Remove a stream and abandon its running pass.
    pub(crate) fn unregister(&self, id: u64) {
        let mut s = self.sched.lock();
        if let Some(entry) = s.slots.remove(&id) {
            entry.abort.store(true, Ordering::Release);
        }
    }

    /// Forget the stream's pending and running work and discard its results: the start of a new
    /// segment. Returns the new generation; a pass that took its snapshot earlier is dropped.
    pub(crate) fn reset_slot(&self, id: u64) -> u64 {
        let mut s = self.sched.lock();
        let Some(entry) = s.slots.get_mut(&id) else {
            return 0;
        };
        entry.generation += 1;
        entry.want = false;
        entry.abort.store(true, Ordering::Release);
        *entry.shared.result.lock() = None;
        entry.generation
    }

    /// Ask for a partial pass of the stream. Returns false, counting a skipped pass, when the
    /// previous one has not finished: passes are never queued behind each other.
    pub(crate) fn request_partial(&self, id: u64) -> bool {
        let now = self.clock.now();
        let mut s = self.sched.lock();
        let Some(entry) = s.slots.get_mut(&id) else {
            return false;
        };
        if entry.want || entry.in_flight {
            self.counters.skipped_busy.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        entry.want = true;
        entry.wanted_at = now;
        self.wake.notify_one();
        true
    }

    /// Queue a final or refine decode ahead of every partial pass; a running partial pass is
    /// preempted.
    pub(crate) fn submit(&self, job: Job) -> Result<(), AsrError> {
        if self.is_dead() {
            return Err(unavailable());
        }
        let mut s = self.sched.lock();
        s.jobs.push_back(job);
        if let Some(r) = &s.running
            && r.is_partial
        {
            r.abort.store(true, Ordering::Release);
        }
        self.wake.notify_one();
        Ok(())
    }

    fn next_work(&self) -> Option<Work> {
        let mut s = self.sched.lock();
        loop {
            if self.shutdown.load(Ordering::Acquire) {
                return None;
            }
            if let Some(job) = s.jobs.pop_front() {
                s.running = Some(Running {
                    abort: Arc::clone(&job.abort),
                    is_partial: false,
                });
                return Some(Work::Job(job));
            }
            if let Some(id) = s.pick_partial() {
                let now = self.clock.now();
                s.last_served = id;
                let abort = Arc::new(AtomicBool::new(false));
                let entry = s.slots.get_mut(&id)?;
                entry.want = false;
                entry.in_flight = true;
                entry.abort = Arc::clone(&abort);
                let work = Work::Partial {
                    id,
                    shared: Arc::clone(&entry.shared),
                    abort: Arc::clone(&abort),
                    generation: entry.generation,
                    waited: now.saturating_since(entry.wanted_at),
                };
                s.running = Some(Running {
                    abort,
                    is_partial: true,
                });
                return Some(work);
            }
            self.wake.wait(&mut s);
        }
    }

    fn decode(
        &self,
        engine: &mut dyn RecognizeEngine,
        audio: &[f32],
        lang: Lang,
        abort: &AtomicBool,
    ) -> Result<String, AsrError> {
        let stop = || abort.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire);
        let started = self.clock.now();
        let out = catch_unwind(AssertUnwindSafe(|| engine.decode(audio, lang, &stop)));
        self.counters
            .record_decode(self.clock.now().saturating_since(started));
        match out {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(EngineError::Aborted)) => Err(AsrError::Cancelled),
            Ok(Err(EngineError::Failed(e))) => Err(AsrError::Internal(e)),
            Err(_) => {
                self.dead.store(true, Ordering::Release);
                Err(AsrError::Internal("the engine panicked".to_owned()))
            }
        }
    }

    fn run_job(&self, engine: &mut dyn RecognizeEngine, job: Job) {
        let result = if job.abort.load(Ordering::Acquire) {
            Err(AsrError::Cancelled)
        } else {
            self.decode(engine, &job.audio, job.lang, &job.abort)
        };
        self.sched.lock().running = None;
        // The waiter may have given up and dropped the receiver.
        let _ = job.reply.send(result);
    }

    fn run_partial(
        &self,
        engine: &mut dyn RecognizeEngine,
        (id, shared, abort, generation, waited): (
            u64,
            Arc<SlotShared>,
            Arc<AtomicBool>,
            u64,
            Duration,
        ),
    ) {
        self.counters.record_wait(waited);
        let snapshot = shared.audio.lock().clone();
        let samples = snapshot.len();
        let out = if abort.load(Ordering::Acquire) {
            Err(AsrError::Cancelled)
        } else {
            self.decode(engine, &snapshot, shared.lang, &abort)
        };
        let mut s = self.sched.lock();
        s.running = None;
        let Some(entry) = s.slots.get_mut(&id) else {
            return;
        };
        entry.in_flight = false;
        if entry.generation != generation {
            return;
        }
        match out {
            Ok(text) => {
                self.counters.partial_passes.fetch_add(1, Ordering::Relaxed);
                *shared.result.lock() = Some(Ok(PassResult {
                    generation,
                    samples,
                    text,
                }));
            }
            Err(AsrError::Cancelled) => {
                // Preempted by a job: decode the latest audio again when the engine is free.
                self.counters.aborted_passes.fetch_add(1, Ordering::Relaxed);
                entry.want = true;
                entry.wanted_at = self.clock.now();
            }
            Err(e) => *shared.result.lock() = Some(Err(e)),
        }
    }

    /// The worker thread's loop.
    pub(crate) fn run(self: &Arc<Self>, mut engine: Box<dyn RecognizeEngine>) {
        while let Some(work) = self.next_work() {
            match work {
                Work::Job(job) => self.run_job(engine.as_mut(), job),
                Work::Partial {
                    id,
                    shared,
                    abort,
                    generation,
                    waited,
                } => self.run_partial(engine.as_mut(), (id, shared, abort, generation, waited)),
            }
            if self.dead.load(Ordering::Acquire) {
                break;
            }
        }
        self.dead.store(true, Ordering::Release);
        self.fail_all();
        // The engine (the native model) is freed here, on the worker, with nothing in flight.
        drop(engine);
    }

    /// The worker has stopped: fail what is waiting.
    fn fail_all(&self) {
        let mut s = self.sched.lock();
        for job in s.jobs.drain(..) {
            let _ = job.reply.send(Err(unavailable()));
        }
        for entry in s.slots.values() {
            *entry.shared.result.lock() = Some(Err(unavailable()));
        }
    }

    /// Wait for a job's reply, giving the job up when `cancelled` is true or `timeout` has passed on
    /// the injected clock.
    pub(crate) fn wait_reply(
        &self,
        rx: &mpsc::Receiver<Result<String, AsrError>>,
        abort: &AtomicBool,
        cancelled: &dyn Fn() -> bool,
        timeout: Duration,
    ) -> Result<String, AsrError> {
        let deadline = self.clock.now().saturating_add(timeout);
        loop {
            match rx.recv_timeout(Duration::from_millis(2)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(unavailable()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if cancelled() {
                abort.store(true, Ordering::Release);
                return Err(AsrError::Cancelled);
            }
            if self.clock.now() >= deadline {
                abort.store(true, Ordering::Release);
                return Err(AsrError::Timeout);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(want: bool, in_flight: bool) -> SlotEntry {
        SlotEntry {
            shared: Arc::new(SlotShared {
                lang: Lang::En,
                audio: Mutex::new(Vec::new()),
                result: Mutex::new(None),
            }),
            want,
            wanted_at: MonoTime::ZERO,
            in_flight,
            abort: Arc::new(AtomicBool::new(false)),
            generation: 0,
        }
    }

    fn sched(slots: &[(u64, bool, bool)], last_served: u64) -> Sched {
        let mut s = Sched {
            last_served,
            ..Sched::default()
        };
        for &(id, want, in_flight) in slots {
            s.slots.insert(id, entry(want, in_flight));
        }
        s
    }

    #[test]
    fn the_stream_served_last_goes_behind_the_other() {
        let both = [(1, true, false), (2, true, false)];
        assert_eq!(sched(&both, 1).pick_partial(), Some(2));
        assert_eq!(sched(&both, 2).pick_partial(), Some(1));
        assert_eq!(sched(&both, 0).pick_partial(), Some(1));
    }

    #[test]
    fn a_lone_waiting_stream_wraps_around_and_idle_ones_are_skipped() {
        assert_eq!(
            sched(&[(1, true, false), (2, false, false)], 1).pick_partial(),
            Some(1)
        );
        assert_eq!(
            sched(&[(1, false, false), (2, false, false)], 0).pick_partial(),
            None
        );
        // A stream whose pass is running is not picked again.
        assert_eq!(
            sched(&[(1, true, true), (2, false, false)], 0).pick_partial(),
            None
        );
        assert_eq!(
            sched(&[(1, true, true), (2, true, false), (3, true, false)], 2).pick_partial(),
            Some(3)
        );
    }
}
