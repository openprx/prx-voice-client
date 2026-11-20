//! A scripted engine for the adapter's tests: no model, no GPU, no sleeps.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::Mutex;
use prxvc_media::Lang;

use super::seam::{CallControl, CallEnd, EngineError, SynthCall, SynthEngine};

/// The text the adapter's warm-up synthesises; the engine answers it itself.
const WARM_UP_PREFIX: &str = "Hello, this is a short warm-up";

/// What the engine saw of one call.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Recorded {
    pub text: String,
    pub lang: Lang,
    pub seed: i64,
    pub max_new_tokens: u32,
    pub speaker: Vec<f32>,
}

pub(super) type Script =
    Box<dyn Fn(&SynthCall<'_>, &dyn CallControl) -> Result<CallEnd, EngineError> + Send + Sync>;

/// Observation handles that stay valid after the engine moved into the adapter.
#[derive(Clone, Default)]
pub(super) struct Probe {
    pub calls: Arc<Mutex<Vec<Recorded>>>,
    pub warm_up_calls: Arc<AtomicUsize>,
    pub extract_calls: Arc<AtomicUsize>,
    pub in_flight: Arc<AtomicUsize>,
    pub dropped: Arc<AtomicBool>,
    pub in_flight_when_dropped: Arc<AtomicUsize>,
}

pub(super) struct TestEngine {
    script: Script,
    probe: Probe,
    fail_warm_up: bool,
    /// The first this many warm-up calls never end their sentence.
    runaway_warm_ups: AtomicUsize,
    embedding_dim: usize,
}

impl TestEngine {
    pub(super) fn new(script: Script) -> (Self, Probe) {
        let probe = Probe::default();
        let engine = Self {
            script,
            probe: probe.clone(),
            fail_warm_up: false,
            runaway_warm_ups: AtomicUsize::new(0),
            embedding_dim: 8,
        };
        (engine, probe)
    }

    /// The first `n` warm-up calls produce audio until the cap stops them.
    pub(super) fn running_away_in_warm_up(self, n: usize) -> Self {
        self.runaway_warm_ups.store(n, Ordering::SeqCst);
        self
    }

    pub(super) fn failing_warm_up(mut self) -> Self {
        self.fail_warm_up = true;
        self
    }
}

impl SynthEngine for TestEngine {
    fn extract_voice(&self, samples_24k: &[f32]) -> Result<Vec<f32>, EngineError> {
        self.probe.extract_calls.fetch_add(1, Ordering::SeqCst);
        let mean = samples_24k.iter().sum::<f32>() / samples_24k.len().max(1) as f32;
        Ok(vec![mean + 1.0; self.embedding_dim])
    }

    fn synthesize(
        &self,
        call: &SynthCall<'_>,
        ctl: &dyn CallControl,
    ) -> Result<CallEnd, EngineError> {
        if call.text.starts_with(WARM_UP_PREFIX) {
            self.probe.warm_up_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_warm_up {
                return Err(EngineError::Call {
                    status: -3,
                    message: "warm-up failed".to_owned(),
                });
            }
            if self
                .runaway_warm_ups
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
            {
                for _ in 0..1000 {
                    if ctl.should_abort() {
                        return Ok(CallEnd::Aborted);
                    }
                    ctl.on_chunk(&[0.1; 1920]);
                }
            }
            ctl.on_chunk(&[0.1; 1920]);
            return Ok(CallEnd::Completed);
        }
        self.probe.calls.lock().push(Recorded {
            text: call.text.to_owned(),
            lang: call.lang,
            seed: call.seed,
            max_new_tokens: call.max_new_tokens,
            speaker: call.speaker.to_vec(),
        });
        self.probe.in_flight.fetch_add(1, Ordering::SeqCst);
        let result = (self.script)(call, ctl);
        self.probe.in_flight.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

impl Drop for TestEngine {
    fn drop(&mut self) {
        self.probe.in_flight_when_dropped.store(
            self.probe.in_flight.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        self.probe.dropped.store(true, Ordering::SeqCst);
    }
}

/// Emit `n` chunks of `samples` samples; chunk `i` is filled with `i + 1`.
pub(super) fn emit(ctl: &dyn CallControl, n: usize, samples: usize) {
    for i in 0..n {
        ctl.on_chunk(&vec![(i + 1) as f32; samples]);
    }
}

/// Wait, without sleeping, until `open` is set or the call is told to stop. `true` when stopped.
pub(super) fn wait_or_abort(ctl: &dyn CallControl, open: &AtomicBool) -> bool {
    while !open.load(Ordering::SeqCst) {
        if ctl.should_abort() {
            return true;
        }
        std::thread::yield_now();
    }
    false
}
