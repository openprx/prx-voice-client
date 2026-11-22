//! `QwenAsr`: recognition by re-decoding, behind the streaming and refine ports.
//!
//! One engine (one loaded model) on one worker thread serves every stream and every refine call; see
//! `super::core` for how the work is ordered. Threading in short: callers (`feed`, `finalize`,
//! `transcribe`) never decode; they hand the worker a request and read results from shared state.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_media::{Clock, Lang, MonoTime, SampleRate, SystemClock};
use prxvc_ports::{
    AsrContext, AsrError, AsrHypothesis, AsrStream, CancellationToken, ProviderDescriptor,
    RefineAsr, SegmentEnd, StreamingAsr, StreamingKind,
};

use super::core::{Core, Job};
use super::seam::{EngineError, RecognizeEngine};
use super::stream::QwenStream;

/// The configured provider id of the Qwen3-ASR recogniser.
pub const PROVIDER_ID: &str = "qwen3-asr-0.6b-q8";

/// Audio the engine decodes once at load, per language, to warm its compute graphs.
const WARM_UP_AUDIO: Duration = Duration::from_secs(2);

/// Scheduling and stability parameters. The pipeline keys they correspond to are noted; there is no
/// `[pipeline]` key for the re-decode cadence, so the composition root passes the defaults or its own.
#[derive(Clone, Debug, PartialEq)]
pub struct AsrParams {
    /// Time between two partial passes of one stream, measured from the start of one request to
    /// the next (design PD-2.5.2, `segmented` mode: 400 to 500 ms).
    pub redecode_interval: Duration,
    /// Audio a segment needs before its first partial pass.
    pub first_decode_after: Duration,
    /// Consecutive hypotheses that must agree for text to be declared stable (at least two).
    pub stable_agreement: usize,
    /// Units (Chinese characters or words) at the end of a growing hypothesis that are never
    /// declared stable: the model has not yet heard what follows them.
    pub stable_tail_guard: usize,
    /// A segment is ended by the recogniser at this length (`pipeline.max_segment_s`).
    pub max_segment: Duration,
    /// A forced end is placed at the quietest point of this much audio before the limit.
    pub cut_search: Duration,
    /// A final or refine decode that has not answered after this long fails with `Timeout`.
    pub final_timeout: Duration,
}

impl Default for AsrParams {
    fn default() -> Self {
        Self {
            redecode_interval: Duration::from_millis(400),
            first_decode_after: Duration::from_millis(480),
            stable_agreement: 4,
            stable_tail_guard: 6,
            max_segment: Duration::from_secs(25),
            cut_search: Duration::from_secs(1),
            final_timeout: Duration::from_secs(20),
        }
    }
}

/// How `QwenAsr` is identified and tuned.
#[derive(Clone, Debug)]
pub struct QwenAsrConfig {
    /// The provider id (`providers.active.asr`).
    pub id: String,
    /// The model name for display and traces.
    pub model: String,
    /// Scheduling and stability parameters.
    pub params: AsrParams,
}

impl Default for QwenAsrConfig {
    fn default() -> Self {
        Self {
            id: PROVIDER_ID.to_owned(),
            model: "Qwen3-ASR-0.6B-Q8_0".to_owned(),
            params: AsrParams::default(),
        }
    }
}

/// `QwenAsr` could not start.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    /// The engine failed to load or to decode the warm-up audio.
    #[error("recogniser engine: {0}")]
    Engine(String),
    /// The worker thread could not be started.
    #[error("recogniser worker thread: {0}")]
    Thread(String),
}

impl From<EngineError> for LoadError {
    fn from(e: EngineError) -> Self {
        Self::Engine(e.to_string())
    }
}

/// A snapshot of the recogniser's counters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AsrStats {
    /// Partial passes that finished and were kept.
    pub partial_passes: u64,
    /// Partial passes abandoned because a final or refine decode preempted them.
    pub aborted_passes: u64,
    /// Passes that were due while the previous pass of the stream was still wanted or running.
    pub skipped_busy: u64,
    /// Final decodes requested.
    pub finals: u64,
    /// Refine decodes requested.
    pub refines: u64,
    /// Partial hypotheses that disagreed with the declared stable prefix and were repaired to keep
    /// it (counted per hypothesis: a disagreement that persists is counted each time).
    pub stable_repairs: u64,
    /// Finals (including forced cuts) whose text contradicts the prefix declared stable earlier;
    /// they are returned as the model decoded them, not repaired.
    pub final_rewrites: u64,
    /// Segments ended by the maximum length.
    pub forced_cuts: u64,
    /// Longest time a requested partial pass waited for the engine.
    pub max_pass_wait: Duration,
    /// Mean time a requested partial pass waited for the engine.
    pub mean_pass_wait: Duration,
    /// Longest decode (any kind).
    pub max_decode: Duration,
}

/// The recogniser: a provider of streaming recognition and of the refine pass.
pub struct QwenAsr {
    core: Arc<Core>,
    descriptor: ProviderDescriptor,
    worker: Mutex<Option<JoinHandle<()>>>,
    warm_up: Duration,
}

impl QwenAsr {
    /// Start the worker on `engine`, warm it up and return when it is ready. `clock` times the
    /// cadence and the timeouts (the system clock in production, a manual one in tests).
    pub fn start(
        engine: Box<dyn RecognizeEngine>,
        cfg: QwenAsrConfig,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, LoadError> {
        let core = Core::new(Arc::clone(&clock), cfg.params);
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker_core = Arc::clone(&core);
        let handle = std::thread::Builder::new()
            .name("prxvc-asr".to_owned())
            .spawn(move || {
                let mut engine = engine;
                let started = clock.now();
                let warm = vec![0.0f32; SampleRate::HZ_16000.samples_in(WARM_UP_AUDIO)];
                let ready = [Lang::Zh, Lang::En]
                    .into_iter()
                    .try_for_each(|lang| engine.decode(&warm, lang, &|| false).map(|_| ()));
                let warm_up = clock.now().saturating_since(started);
                let failed = ready.is_err();
                let _ = ready_tx.send(ready.map(|()| warm_up));
                if !failed {
                    worker_core.run(engine);
                }
            })
            .map_err(|e| LoadError::Thread(e.to_string()))?;
        let warm_up = match ready_rx.recv() {
            Ok(Ok(d)) => d,
            Ok(Err(e)) => {
                let _ = handle.join();
                return Err(e.into());
            }
            Err(_) => {
                let _ = handle.join();
                return Err(LoadError::Engine(
                    "the worker ended during warm-up".to_owned(),
                ));
            }
        };
        // Truthful capability: partials come from re-decoding the growing audio of the open
        // segment and every partial carries a stable prefix that later partials never retract.
        // It passes `require_live` and `require_partials` and refuses `require_native`.
        let descriptor = ProviderDescriptor::new(cfg.id, cfg.model, StreamingKind::Redecode)
            .with_stable_prefix(true)
            .with_languages([Lang::Zh, Lang::En])
            .with_sample_rates([SampleRate::HZ_16000]);
        Ok(Self {
            core,
            descriptor,
            worker: Mutex::new(Some(handle)),
            warm_up,
        })
    }

    /// Time the warm-up decodes took.
    pub fn warm_up_duration(&self) -> Duration {
        self.warm_up
    }

    /// The counters, read at once.
    pub fn stats(&self) -> AsrStats {
        let c = &self.core.counters;
        let get = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);
        let passes = get(&c.partial_passes) + get(&c.aborted_passes);
        let mean = get(&c.wait_ns_total).checked_div(passes).unwrap_or(0);
        AsrStats {
            partial_passes: get(&c.partial_passes),
            aborted_passes: get(&c.aborted_passes),
            skipped_busy: get(&c.skipped_busy),
            finals: get(&c.finals),
            refines: get(&c.refines),
            stable_repairs: get(&c.stable_repairs),
            final_rewrites: get(&c.final_rewrites),
            forced_cuts: get(&c.forced_cuts),
            max_pass_wait: Duration::from_nanos(get(&c.wait_ns_max)),
            mean_pass_wait: Duration::from_nanos(mean),
            max_decode: Duration::from_nanos(get(&c.decode_ns_max)),
        }
    }

    /// Stop the worker: abort the decode in flight, fail what waits, free the engine. Called by
    /// `Drop`; streams opened earlier then fail with `Unavailable`.
    pub fn shut_down(&self) {
        self.core.shut_down();
        if let Some(handle) = self.worker.lock().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for QwenAsr {
    fn drop(&mut self) {
        self.shut_down();
    }
}

impl StreamingAsr for QwenAsr {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    fn open(&self, lang: Lang, ctx: AsrContext) -> Result<Box<dyn AsrStream>, AsrError> {
        if !self.descriptor.supports_lang(lang) {
            return Err(AsrError::UnsupportedLanguage(lang));
        }
        if self.core.is_dead() {
            return Err(AsrError::Unavailable {
                reason: "the recogniser engine has stopped".to_owned(),
            });
        }
        Ok(Box::new(QwenStream::open(&self.core, lang, ctx)))
    }
}

impl RefineAsr for QwenAsr {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    fn transcribe(
        &self,
        audio: &[f32],
        rate: SampleRate,
        lang: Lang,
        _ctx: &AsrContext,
        cancel: &CancellationToken,
    ) -> Result<AsrHypothesis, AsrError> {
        if rate != SampleRate::HZ_16000 {
            return Err(AsrError::UnsupportedRate(rate));
        }
        if !self.descriptor.supports_lang(lang) {
            return Err(AsrError::UnsupportedLanguage(lang));
        }
        if cancel.is_cancelled() {
            return Err(AsrError::Cancelled);
        }
        if audio.iter().any(|s| !s.is_finite()) {
            return Err(AsrError::Internal(
                "audio holds a non-finite sample".to_owned(),
            ));
        }
        let text = if audio.is_empty() {
            String::new()
        } else {
            let (reply, rx) = mpsc::channel();
            let abort = Arc::new(std::sync::atomic::AtomicBool::new(false));
            self.core.submit(Job {
                audio: audio.to_vec(),
                lang,
                abort: Arc::clone(&abort),
                reply,
            })?;
            self.core.wait_reply(
                &rx,
                &abort,
                &|| cancel.is_cancelled(),
                self.core.params.final_timeout,
            )?
        };
        self.core.counters.refines.fetch_add(1, Ordering::Relaxed);
        Ok(AsrHypothesis {
            stable_prefix_len: text.len(),
            text,
            revision: 1,
            is_final: true,
            ended_by: Some(SegmentEnd::Requested),
            contradicts_stable: false,
            audio_end: MonoTime::ZERO.saturating_add(rate.duration_of(audio.len())),
        })
    }
}

impl QwenAsr {
    /// Start on the system clock.
    pub fn start_system(
        engine: Box<dyn RecognizeEngine>,
        cfg: QwenAsrConfig,
    ) -> Result<Self, LoadError> {
        Self::start(engine, cfg, Arc::new(SystemClock::new()))
    }
}
