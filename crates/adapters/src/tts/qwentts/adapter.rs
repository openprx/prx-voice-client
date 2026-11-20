//! `QwenTts`: the `qwentts.cpp` engine behind the synthesis port.
//!
//! # Threads
//!
//! The engine lives for the life of the adapter. Synthesis calls are blocking, so a pool of worker
//! threads (one per batch slot, two by default) runs them; `synthesize` only queues a job and
//! returns the request's stream, so a caller never waits on GPU work. Two requests at once (one
//! per call direction) occupy two workers and are batched by the runtime into shared decode
//! steps. More requests than workers wait in a bounded job queue (a full queue is an
//! `Unavailable` error, not a wait). The runtime's chunk and cancel callbacks run on its own
//! compute thread and touch only the request's [`AttemptSink`](super::request::AttemptSink):
//! atomics and a bounded queue, never caller code. Each request has its own flags, so cancelling
//! one request (or dropping its stream) stops that call at its next poll and leaves the other
//! untouched. Back-pressure is described in the `request` module.
//!
//! # Shutdown
//!
//! Dropping the adapter flags every request as closing (in-flight calls stop at their next
//! poll), closes the job queue, fails queued jobs with `Unavailable`, joins the workers and only
//! then releases its reference to the engine, which frees the native context. Streams hold no
//! reference to the engine, so none can outlive it.

use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex, RwLock};
use prxvc_media::{Lang, PcmChunk, SampleRate};
use prxvc_ports::{
    CancellationToken, PreparedVoice, ProviderDescriptor, SpeechSynth, StreamingKind, TtsError,
    TtsRequest, TtsStream, VoiceReference, VoiceSelection,
};

use super::cap::{self, RunawayRetry};
use super::request::{AttemptSink, Counters, Outcome, QwenStream, RequestFlags, queue_capacity};
use super::seam::{CallEnd, EngineError, SynthCall, SynthEngine};
use super::voice::{
    CacheKey, ENGINE_RATE, VoiceCache, embedding_of, handle_of, prepared_voice, reference_hash,
    reference_to_engine_rate,
};

/// The provider id of the adapter.
pub const PROVIDER_ID: &str = "qwentts";

/// Settings of the adapter.
#[derive(Clone, Debug)]
pub struct QwenTtsConfig {
    /// The model's name for display and traces, for example the catalogue id.
    pub model_label: String,
    /// Hash of the model files the engine was loaded with ([`hash_model_files`](super::hash_model_files)
    /// or the catalogue's verified checksums); keys prepared voices.
    pub model_hash: String,
    /// Worker threads, which is the number of requests served at once. Must not exceed the
    /// engine's batch size. Two: one per call direction.
    pub workers: usize,
    /// Jobs that may wait for a free worker.
    pub queue_depth: usize,
    /// Whether a runaway is retried with another seed. Default: no; the runaway is reported.
    pub runaway_retry: RunawayRetry,
    /// Seed of the first attempt; `None` lets the runtime choose a random one.
    pub seed: Option<i64>,
    /// Replaces the cap computed from the text, in codec frames. For tests and diagnostics (a
    /// deliberately tiny cap proves the runaway path); production leaves it `None`.
    pub cap_frames_override: Option<u32>,
}

impl QwenTtsConfig {
    /// The default settings for a model.
    pub fn new(model_label: impl Into<String>, model_hash: impl Into<String>) -> Self {
        Self {
            model_label: model_label.into(),
            model_hash: model_hash.into(),
            workers: 2,
            queue_depth: 8,
            runaway_retry: RunawayRetry::Off,
            seed: None,
            cap_frames_override: None,
        }
    }
}

/// Loading the adapter failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    /// The engine could not be loaded.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// The settings cannot work.
    #[error("invalid configuration: {0}")]
    Config(&'static str),
    /// The warm-up synthesis failed, so the engine is not ready.
    #[error("warm-up synthesis failed: {0}")]
    WarmUp(TtsError),
}

/// Counters of the adapter since it started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TtsStats {
    /// Requests accepted (the warm-up's included).
    pub requests: u64,
    /// Requests that ended with `Done`.
    pub completed: u64,
    /// Requests cancelled or abandoned by their consumer.
    pub cancelled: u64,
    /// Requests that ended with an error other than a cancellation.
    pub failed: u64,
    /// Attempts the runaway guard stopped.
    pub runaways: u64,
    /// Retries started after a runaway.
    pub retries: u64,
    /// Chunk queues that overflowed (requests failed, no audio dropped silently).
    pub overruns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum VoiceKey {
    Preset(String),
    Cloned(String),
}

impl VoiceKey {
    fn of(selection: &VoiceSelection) -> Self {
        match selection {
            VoiceSelection::Preset { name } => Self::Preset(name.clone()),
            VoiceSelection::Cloned { profile_id } => Self::Cloned(profile_id.clone()),
        }
    }

    fn profile_id(&self) -> String {
        match self {
            Self::Preset(name) => format!("preset:{name}"),
            Self::Cloned(id) => id.clone(),
        }
    }
}

struct Job {
    text: String,
    lang: Lang,
    speaker: Arc<[f32]>,
    cap_frames: u32,
    flags: Arc<RequestFlags>,
    tx: SyncSender<PcmChunk>,
    outcome: Arc<Mutex<Option<Outcome>>>,
    clause: String,
}

struct QueueState {
    jobs: VecDeque<Job>,
    closed: bool,
}

/// The pending jobs; the lock is held only to push or pop (CS-4.5.2).
struct JobQueue {
    state: Mutex<QueueState>,
    ready: Condvar,
    depth: usize,
}

impl JobQueue {
    fn push(&self, job: Job) -> Result<(), TtsError> {
        let mut state = self.state.lock();
        if state.closed {
            return Err(unavailable("the engine is shutting down"));
        }
        if state.jobs.len() >= self.depth {
            return Err(unavailable("the synthesis queue is full"));
        }
        state.jobs.push_back(job);
        drop(state);
        self.ready.notify_one();
        Ok(())
    }

    /// The next job, waiting for one; `None` once the queue is closed and empty.
    fn pop(&self) -> Option<Job> {
        let mut state = self.state.lock();
        loop {
            if let Some(job) = state.jobs.pop_front() {
                return Some(job);
            }
            if state.closed {
                return None;
            }
            self.ready.wait(&mut state);
        }
    }

    fn close(&self) -> Vec<Job> {
        let mut state = self.state.lock();
        state.closed = true;
        let rest = state.jobs.drain(..).collect();
        drop(state);
        self.ready.notify_all();
        rest
    }
}

fn unavailable(reason: &str) -> TtsError {
    TtsError::Unavailable {
        reason: reason.to_owned(),
    }
}

struct Inner {
    engine: Arc<dyn SynthEngine>,
    cfg: QwenTtsConfig,
    voices: RwLock<HashMap<VoiceKey, Arc<[f32]>>>,
    queue: JobQueue,
    counters: Counters,
    closing: AtomicBool,
    seeds: AtomicU64,
}

/// The `qwentts.cpp` synthesiser.
pub struct QwenTts {
    inner: Arc<Inner>,
    workers: Vec<JoinHandle<()>>,
    warm_up: Duration,
}

impl QwenTts {
    /// Start the worker threads, run the warm-up synthesis and return the adapter, which is ready.
    ///
    /// Blocks for the warm-up (about a second once the model is loaded); call it from a loading
    /// thread, not from the UI or a pipeline thread. The warm-up is a throw-away synthesis of a
    /// short sentence with a synthetic voice, then, with two workers, two at once so the batched
    /// path is warm as well; its audio is discarded. The first synthesis in a fresh process takes
    /// 300 to 830 ms to its first chunk without it.
    pub fn start(engine: Arc<dyn SynthEngine>, cfg: QwenTtsConfig) -> Result<Self, LoadError> {
        if cfg.workers == 0 {
            return Err(LoadError::Config("at least one worker is needed"));
        }
        if cfg.model_hash.is_empty() {
            return Err(LoadError::Config("the model hash is empty"));
        }
        let inner = Arc::new(Inner {
            engine,
            queue: JobQueue {
                state: Mutex::new(QueueState {
                    jobs: VecDeque::new(),
                    closed: false,
                }),
                ready: Condvar::new(),
                depth: cfg.queue_depth,
            },
            cfg,
            voices: RwLock::new(HashMap::new()),
            counters: Counters::default(),
            closing: AtomicBool::new(false),
            seeds: AtomicU64::new(0x9E37_79B9_7F4A_7C15),
        });
        let mut tts = Self {
            workers: Vec::new(),
            warm_up: Duration::ZERO,
            inner,
        };
        for index in 0..tts.inner.cfg.workers {
            let inner = Arc::clone(&tts.inner);
            let handle = thread::Builder::new()
                .name(format!("qwentts-{index}"))
                .spawn(move || worker_loop(&inner))
                .map_err(|e| EngineError::Load(format!("cannot start a worker thread: {e}")))?;
            tts.workers.push(handle);
        }
        // On failure `tts` drops here, which stops the workers and frees the engine.
        tts.warm_up = tts.run_warm_up().map_err(LoadError::WarmUp)?;
        Ok(tts)
    }

    /// How long the warm-up took.
    pub fn warm_up_duration(&self) -> Duration {
        self.warm_up
    }

    /// The hash of the model files this adapter was configured with.
    pub fn model_hash(&self) -> &str {
        &self.inner.cfg.model_hash
    }

    /// Counters since start.
    pub fn stats(&self) -> TtsStats {
        let c = &self.inner.counters;
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        TtsStats {
            requests: get(&c.requests),
            completed: get(&c.completed),
            cancelled: get(&c.cancelled),
            failed: get(&c.failed),
            runaways: get(&c.runaways),
            retries: get(&c.retries),
            overruns: get(&c.overruns),
        }
    }

    /// Register a voice prepared earlier (read from the cache) under `selection`.
    pub fn install_prepared(
        &self,
        selection: &VoiceSelection,
        prepared: &PreparedVoice,
    ) -> Result<(), TtsError> {
        let spk = embedding_of(prepared, &self.inner.cfg.model_hash)?;
        self.install(selection, spk);
        Ok(())
    }

    /// Prepare the voice of `reference` as a cloned voice, using the cache of its profile: a hit
    /// skips the engine (about 0.4 s). Returns the prepared voice and whether the cache served it.
    /// A cache that cannot be written is logged and does not fail the preparation.
    pub fn prepare_voice_cached(
        &self,
        cache: &VoiceCache,
        reference: &VoiceReference,
    ) -> Result<(PreparedVoice, bool), TtsError> {
        let selection = VoiceSelection::Cloned {
            profile_id: reference.profile_id.clone(),
        };
        self.prepare_cached(cache, reference, &selection)
    }

    /// Register a preset voice: the stored reference prompt it speaks with. `reference` is a
    /// bundled or catalogue-provided recording; `cache` (the preset's own directory) keeps the
    /// prepared form so the preparation is not repeated at every start.
    pub fn register_preset(
        &self,
        name: &str,
        reference: &VoiceReference,
        cache: Option<&VoiceCache>,
    ) -> Result<PreparedVoice, TtsError> {
        let selection = VoiceSelection::Preset {
            name: name.to_owned(),
        };
        match cache {
            Some(cache) => self
                .prepare_cached(cache, reference, &selection)
                .map(|(v, _)| v),
            None => {
                let spk = self.extract(reference)?;
                let prepared = prepared_voice(
                    &VoiceKey::of(&selection).profile_id(),
                    &self.inner.cfg.model_hash,
                    &spk,
                );
                self.install(&selection, spk);
                Ok(prepared)
            }
        }
    }

    fn prepare_cached(
        &self,
        cache: &VoiceCache,
        reference: &VoiceReference,
        selection: &VoiceSelection,
    ) -> Result<(PreparedVoice, bool), TtsError> {
        let model_hash = &self.inner.cfg.model_hash;
        let id = VoiceKey::of(selection).profile_id();
        let key = CacheKey {
            model_hash: model_hash.clone(),
            reference_sha256: reference_hash(reference),
        };
        match cache.lookup(&key) {
            Ok(Some(spk)) => {
                let prepared = prepared_voice(&id, model_hash, &spk);
                self.install(selection, spk);
                return Ok((prepared, true));
            }
            Ok(None) => {}
            // A cache that cannot be read costs one preparation; the voice still works.
            Err(e) => tracing::warn!(error = %e, "voice cache lookup failed; preparing the voice"),
        }
        let spk = self.extract(reference)?;
        if let Err(e) = cache.store(&key, &spk) {
            tracing::warn!(error = %e, "voice cache could not be written");
        }
        let prepared = prepared_voice(&id, model_hash, &spk);
        self.install(selection, spk);
        Ok((prepared, false))
    }

    fn extract(&self, reference: &VoiceReference) -> Result<Vec<f32>, TtsError> {
        let samples = reference_to_engine_rate(reference)?;
        // Runs on the caller's thread: preparation is an explicit, rare step (enrolment, start-up).
        let spk = self
            .inner
            .engine
            .extract_voice(&samples)
            .map_err(|e| TtsError::Internal(format!("voice preparation failed: {e}")))?;
        if spk.is_empty() || spk.iter().any(|v| !v.is_finite()) {
            return Err(TtsError::Internal(
                "voice preparation returned an invalid embedding".to_owned(),
            ));
        }
        Ok(spk)
    }

    fn install(&self, selection: &VoiceSelection, spk: Vec<f32>) {
        self.inner
            .voices
            .write()
            .insert(VoiceKey::of(selection), spk.into());
    }

    fn run_warm_up(&self) -> Result<Duration, TtsError> {
        let started = Instant::now();
        let reference = VoiceReference {
            profile_id: "warm-up".to_owned(),
            samples: warm_up_reference(),
            rate: ENGINE_RATE,
            transcript: None,
            lang: Lang::En,
        };
        let spk: Arc<[f32]> = self.extract(&reference)?.into();
        let parallel = self.inner.cfg.workers.min(2);
        for round in [1, parallel] {
            let streams = (0..round)
                .map(|_| {
                    let voice = VoiceSelection::Preset {
                        name: "warm-up".to_owned(),
                    };
                    let text = "Hello, this is a short warm-up sentence.";
                    let cap = cap::cap_frames(text, Lang::En);
                    self.submit_for(text, Lang::En, &spk, voice, "warm-up".to_owned(), cap)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let never = CancellationToken::new();
            for mut stream in streams {
                // The audio is thrown away; only a failure matters.
                while let Some(item) = stream.next(&never) {
                    item?;
                }
            }
            if parallel == 1 {
                break;
            }
        }
        Ok(started.elapsed())
    }

    fn submit_for(
        &self,
        text: &str,
        lang: Lang,
        speaker: &Arc<[f32]>,
        voice: VoiceSelection,
        clause: String,
        cap_frames: u32,
    ) -> Result<QwenStream, TtsError> {
        let (tx, rx) = sync_channel(queue_capacity(cap_frames));
        let flags = Arc::new(RequestFlags::default());
        let outcome = Arc::new(Mutex::new(None));
        let job = Job {
            text: text.to_owned(),
            lang,
            speaker: Arc::clone(speaker),
            cap_frames,
            flags: Arc::clone(&flags),
            tx,
            outcome: Arc::clone(&outcome),
            clause,
        };
        self.inner.queue.push(job)?;
        self.inner.counters.requests.fetch_add(1, Ordering::Relaxed);
        Ok(QwenStream::new(rx, outcome, flags, voice))
    }
}

/// A short voiced signal for the warm-up's voice: harmonics of a gliding 120 Hz fundamental with
/// a syllable-rate envelope. It only has to be valid input for the speaker encoder.
fn warm_up_reference() -> Vec<f32> {
    let rate = f64::from(ENGINE_RATE.hz());
    let mut phase = 0.0f64;
    (0..(2 * ENGINE_RATE.hz()) as usize)
        .map(|i| {
            let t = i as f64 / rate;
            let f0 = 120.0 + 15.0 * (2.0 * std::f64::consts::PI * 0.7 * t).sin();
            phase += 2.0 * std::f64::consts::PI * f0 / rate;
            let envelope = 0.55 + 0.45 * (2.0 * std::f64::consts::PI * 4.0 * t).sin();
            let voiced: f64 = (1..=12)
                .map(|h| (phase * f64::from(h)).sin() / f64::from(h))
                .sum();
            (0.2 * envelope * voiced) as f32
        })
        .collect()
}

impl SpeechSynth for QwenTts {
    fn descriptor(&self) -> ProviderDescriptor {
        // Streaming audio out; text arrives one complete clause per call (the runtime has no
        // incremental text input), nothing leaves the machine.
        ProviderDescriptor::new(
            PROVIDER_ID,
            self.inner.cfg.model_label.clone(),
            StreamingKind::Native,
        )
        .with_languages([Lang::Zh, Lang::En])
        .with_sample_rates([SampleRate::HZ_24000])
    }

    fn prepare_voice(&self, reference: &VoiceReference) -> Result<PreparedVoice, TtsError> {
        let spk = self.extract(reference)?;
        let prepared = prepared_voice(&reference.profile_id, &self.inner.cfg.model_hash, &spk);
        self.install(
            &VoiceSelection::Cloned {
                profile_id: reference.profile_id.clone(),
            },
            spk,
        );
        Ok(prepared)
    }

    fn release_voice(&self, handle: &str) -> Result<(), TtsError> {
        let model_hash = &self.inner.cfg.model_hash;
        let mut voices = self.inner.voices.write();
        let key = voices
            .keys()
            .find(|k| handle_of(model_hash, &k.profile_id()) == handle)
            .cloned();
        match key {
            Some(k) => {
                voices.remove(&k);
                Ok(())
            }
            None => Err(TtsError::UnknownVoice(handle.to_owned())),
        }
    }

    fn synthesize(&self, req: TtsRequest) -> Result<Box<dyn TtsStream>, TtsError> {
        if req.text.trim().is_empty() {
            return Err(TtsError::Rejected { status: 400 });
        }
        if req.text.contains('\0') || req.text.chars().count() > cap::MAX_TEXT_CHARS {
            return Err(TtsError::Rejected { status: 400 });
        }
        let speaker = self
            .inner
            .voices
            .read()
            .get(&VoiceKey::of(&req.voice))
            .cloned()
            .ok_or_else(|| {
                TtsError::UnknownVoice(match &req.voice {
                    VoiceSelection::Preset { name } => name.clone(),
                    VoiceSelection::Cloned { profile_id } => profile_id.clone(),
                })
            })?;
        let cap_frames = self
            .inner
            .cfg
            .cap_frames_override
            .unwrap_or_else(|| cap::cap_frames(&req.text, req.lang));
        let clause = req.clause.to_string();
        let stream =
            self.submit_for(&req.text, req.lang, &speaker, req.voice, clause, cap_frames)?;
        Ok(Box::new(stream))
    }
}

impl Drop for QwenTts {
    fn drop(&mut self) {
        self.inner.closing.store(true, Ordering::Release);
        for job in self.inner.queue.close() {
            finish(
                &self.inner,
                &job,
                Err(unavailable("the engine is shutting down")),
            );
        }
        for worker in self.workers.drain(..) {
            if worker.join().is_err() {
                tracing::error!("a synthesis worker panicked");
            }
        }
        // `inner`, and with it the last reference to the engine, drops after the workers ended.
    }
}

fn worker_loop(inner: &Inner) {
    while let Some(job) = inner.queue.pop() {
        run_job(inner, job);
    }
}

/// Store the terminal outcome. The job's sender drops with the job, which wakes the consumer after
/// every queued chunk.
fn finish(inner: &Inner, job: &Job, outcome: Outcome) {
    let counter = match &outcome {
        Ok(_) => &inner.counters.completed,
        Err(TtsError::Cancelled) => &inner.counters.cancelled,
        Err(_) => &inner.counters.failed,
    };
    counter.fetch_add(1, Ordering::Relaxed);
    *job.outcome.lock() = Some(outcome);
}

enum Verdict {
    Done(f32),
    Cancelled,
    Failed(TtsError),
    Runaway(f32),
}

fn run_job(inner: &Inner, job: Job) {
    if job.flags.cancelled.load(Ordering::Acquire) {
        finish(inner, &job, Err(TtsError::Cancelled));
        return;
    }
    let retries = inner.cfg.runaway_retry.extra_attempts();
    let mut attempt = 0;
    let outcome = loop {
        let seed = if attempt == 0 {
            inner.cfg.seed.unwrap_or(-1)
        } else {
            retry_seed(inner)
        };
        let verdict = attempt_once(inner, &job, seed, retries > 0);
        match verdict {
            Verdict::Done(secs) => break Ok(secs),
            Verdict::Cancelled => break Err(TtsError::Cancelled),
            Verdict::Failed(e) => break Err(e),
            Verdict::Runaway(secs) => {
                inner.counters.runaways.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(clause = %job.clause, produced_secs = secs, attempt, "synthesis ran away");
                if attempt < retries {
                    attempt += 1;
                    inner.counters.retries.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                break Err(TtsError::Runaway {
                    produced_secs: secs,
                });
            }
        }
    };
    finish(inner, &job, outcome);
}

/// A seed for a retry: well mixed and different from the previous attempt's.
fn retry_seed(inner: &Inner) -> i64 {
    let mut z = inner
        .seeds
        .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    i64::try_from(z >> 1).unwrap_or(1)
}

fn attempt_once(inner: &Inner, job: &Job, seed: i64, hold: bool) -> Verdict {
    let sink = AttemptSink::new(
        &job.flags,
        &inner.closing,
        &inner.counters,
        &job.tx,
        job.cap_frames,
        hold,
    );
    let call = SynthCall {
        text: &job.text,
        lang: job.lang,
        speaker: &job.speaker,
        seed,
        max_new_tokens: job.cap_frames,
    };
    let result = catch_unwind(AssertUnwindSafe(|| inner.engine.synthesize(&call, &sink)));
    let end = match result {
        Err(_) => {
            return Verdict::Failed(TtsError::Internal("the engine panicked".to_owned()));
        }
        Ok(Err(e)) => return Verdict::Failed(TtsError::Internal(e.to_string())),
        Ok(Ok(end)) => end,
    };
    if sink.closing() {
        return Verdict::Failed(unavailable("the engine is shutting down"));
    }
    if sink.cancelled() {
        return Verdict::Cancelled;
    }
    if sink.corrupt() {
        return Verdict::Failed(TtsError::Internal(
            "the engine produced audio that is not finite".to_owned(),
        ));
    }
    if sink.stalled() {
        return Verdict::Failed(unavailable(
            "the audio consumer fell behind: the chunk queue overflowed",
        ));
    }
    // Reaching the cap ends the call as a success for the runtime; here it is a failure.
    if sink.runaway() || cap::is_runaway(sink.produced_samples(), job.cap_frames) {
        return Verdict::Runaway(sink.produced_secs());
    }
    if end == CallEnd::Aborted {
        return Verdict::Failed(TtsError::Internal(
            "the engine stopped the call without being asked".to_owned(),
        ));
    }
    if sink.produced_samples() == 0 {
        return Verdict::Failed(TtsError::Internal(
            "the engine produced no audio".to_owned(),
        ));
    }
    if !sink.release_held() {
        return if sink.cancelled() {
            Verdict::Cancelled
        } else {
            Verdict::Failed(unavailable(
                "the audio consumer fell behind: the chunk queue overflowed",
            ))
        };
    }
    Verdict::Done(sink.produced_secs())
}
