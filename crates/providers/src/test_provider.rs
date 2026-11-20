//! "Test provider": a minimal real request through a built provider, with latency and a
//! classified error, for the settings UI.
//!
//! Each function drives one built provider through the same port the pipeline uses, so what it
//! proves is what a call would get. Nothing is written anywhere and no audio is played: the audio
//! of a synthesis is discarded, a translation is only read. A request that does not finish in
//! `timeout` is cancelled through its token and reported as a timeout.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use prxvc_media::{ClauseId, Direction, Lang, MonoTime, SampleRate, SegmentId};
use prxvc_ports::{
    AsrContext, AsrError, CancellationToken, MtDelta, MtError, MtRequest, RefineAsr, SpeechSynth,
    StreamingAsr, Translator, TtsError, TtsItem, TtsRequest, VadProvider, VoiceSelection,
};

/// The sentence a translation test sends.
pub const TEST_SENTENCE: &str = "Hello, can you hear me?";
/// The clause a synthesis test speaks.
pub const TEST_CLAUSE: &str = "Hello, this is a test.";

/// How a test failed, in the terms of the connection-test result line (UX 4.8.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    /// The host cannot be reached (refused, unreachable, connect timeout).
    Unreachable,
    /// No answer within the timeout.
    Timeout,
    /// The key was rejected (401).
    Unauthorized,
    /// Not allowed (403).
    Forbidden,
    /// Model or address not found (404).
    NotFound,
    /// Rate limited (429).
    RateLimited,
    /// The server answered with another error status.
    Rejected,
    /// The provider answered with something that is not what the role produces.
    BadResponse,
    /// The engine is unusable (a model that cannot load, a stopped worker).
    Unavailable,
    /// The voice is not known to the engine.
    UnknownVoice,
}

/// A classified failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestError {
    /// The class.
    pub class: FailureClass,
    /// The provider's own message (never a secret).
    pub message: String,
}

/// The outcome of one test.
#[derive(Clone, Debug, PartialEq)]
pub struct TestReport {
    /// Whether the request did what the role needs.
    pub ok: bool,
    /// Time to the first output (first token, first audio chunk); `None` when none arrived or
    /// the role has no incremental output.
    pub first_response: Option<Duration>,
    /// Time to the end of the request.
    pub total: Duration,
    /// What was observed: the translation text, the seconds of audio, the recognised text.
    pub detail: String,
    /// The failure, when `ok` is false.
    pub error: Option<TestError>,
}

impl TestReport {
    fn success(first: Option<Duration>, started: Instant, detail: String) -> Self {
        Self {
            ok: true,
            first_response: first,
            total: started.elapsed(),
            detail,
            error: None,
        }
    }

    fn failure(first: Option<Duration>, started: Instant, class: FailureClass, message: String) -> Self {
        Self {
            ok: false,
            first_response: first,
            total: started.elapsed(),
            detail: String::new(),
            error: Some(TestError { class, message }),
        }
    }
}

/// Run `body` with a token that a watchdog cancels after `timeout`. The watchdog thread ends
/// before this returns.
fn with_deadline<R>(timeout: Duration, body: impl FnOnce(&CancellationToken) -> R) -> (R, bool) {
    let token = CancellationToken::new();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let watchdog = {
        let token = token.clone();
        thread::spawn(move || {
            if done_rx.recv_timeout(timeout).is_err_and(|e| e == mpsc::RecvTimeoutError::Timeout) {
                token.cancel();
                return true;
            }
            false
        })
    };
    let result = body(&token);
    let _ = done_tx.send(());
    let timed_out = watchdog.join().unwrap_or(false);
    (result, timed_out)
}

fn clause() -> ClauseId {
    ClauseId::new(SegmentId::first(Direction::Outgoing), 0)
}

fn classify_mt(e: &MtError) -> FailureClass {
    match e {
        MtError::Timeout | MtError::Cancelled => FailureClass::Timeout,
        MtError::Rejected { status: 401 } => FailureClass::Unauthorized,
        MtError::Rejected { status: 403 } => FailureClass::Forbidden,
        MtError::Rejected { status: 404 } => FailureClass::NotFound,
        MtError::Rejected { status: 429 } => FailureClass::RateLimited,
        MtError::Rejected { .. } => FailureClass::Rejected,
        // The adapter words connection failures as "cannot connect: ..." and "connect timeout".
        MtError::Unavailable { reason }
            if reason.starts_with("cannot connect") || reason.starts_with("connect timeout") =>
        {
            FailureClass::Unreachable
        }
        MtError::Unavailable { .. } => FailureClass::Unavailable,
        _ => FailureClass::BadResponse,
    }
}

/// Translate [`TEST_SENTENCE`] from `source` to `target` and read the whole stream.
pub fn test_translator(
    provider: &dyn Translator,
    source: Lang,
    target: Lang,
    timeout: Duration,
) -> TestReport {
    let started = Instant::now();
    let request = MtRequest {
        clause: clause(),
        source,
        target,
        text: TEST_SENTENCE.to_owned(),
        context: Vec::new(),
        glossary: Vec::new(),
    };
    let (outcome, timed_out) = with_deadline(timeout, |cancel| {
        let mut stream = provider.translate(request)?;
        let mut first = None;
        let mut text = String::new();
        while let Some(item) = stream.next(cancel) {
            match item {
                Ok(MtDelta::Text(t)) => {
                    first.get_or_insert_with(|| started.elapsed());
                    text.push_str(&t);
                }
                Ok(MtDelta::Done { .. }) => return Ok((first, text)),
                Err(e) => return Err(e),
            }
        }
        Err(MtError::Internal("the stream ended without a result".to_owned()))
    });
    match outcome {
        Ok((first, text)) if !text.trim().is_empty() => TestReport::success(first, started, text),
        Ok((first, _)) => TestReport::failure(
            first,
            started,
            FailureClass::BadResponse,
            "the translation is empty".to_owned(),
        ),
        Err(e) => {
            let class = if timed_out { FailureClass::Timeout } else { classify_mt(&e) };
            TestReport::failure(None, started, class, e.to_string())
        }
    }
}

fn classify_tts(e: &TtsError) -> FailureClass {
    match e {
        TtsError::Timeout | TtsError::Cancelled => FailureClass::Timeout,
        TtsError::Rejected { status: 401 } => FailureClass::Unauthorized,
        TtsError::Rejected { status: 403 } => FailureClass::Forbidden,
        TtsError::Rejected { status: 404 } => FailureClass::NotFound,
        TtsError::Rejected { status: 429 } => FailureClass::RateLimited,
        TtsError::Rejected { .. } => FailureClass::Rejected,
        TtsError::UnknownVoice(_) => FailureClass::UnknownVoice,
        TtsError::Unavailable { .. } => FailureClass::Unavailable,
        _ => FailureClass::BadResponse,
    }
}

/// Speak [`TEST_CLAUSE`] with `voice` and discard the audio; ok when audio arrived and the
/// stream completed.
pub fn test_synth(
    provider: &dyn SpeechSynth,
    lang: Lang,
    voice: VoiceSelection,
    timeout: Duration,
) -> TestReport {
    let started = Instant::now();
    let request = TtsRequest {
        clause: clause(),
        text: TEST_CLAUSE.to_owned(),
        lang,
        voice,
    };
    let (outcome, timed_out) = with_deadline(timeout, |cancel| {
        let mut stream = provider.synthesize(request)?;
        let mut first = None;
        let mut samples = 0usize;
        while let Some(item) = stream.next(cancel) {
            match item? {
                TtsItem::Chunk(chunk) => {
                    first.get_or_insert_with(|| started.elapsed());
                    samples += chunk.samples.len();
                }
                TtsItem::Done { audio_secs } => return Ok((first, samples, audio_secs)),
            }
        }
        Err(TtsError::Internal("the stream ended without a result".to_owned()))
    });
    match outcome {
        Ok((first, samples, secs)) if samples > 0 => {
            TestReport::success(first, started, format!("{secs:.2} s of audio"))
        }
        Ok((first, ..)) => TestReport::failure(
            first,
            started,
            FailureClass::BadResponse,
            "the engine produced no audio".to_owned(),
        ),
        Err(e) => {
            let class = if timed_out { FailureClass::Timeout } else { classify_tts(&e) };
            TestReport::failure(None, started, class, e.to_string())
        }
    }
}

fn classify_asr(e: &AsrError) -> FailureClass {
    match e {
        AsrError::Timeout | AsrError::Cancelled => FailureClass::Timeout,
        AsrError::Unavailable { .. } => FailureClass::Unavailable,
        _ => FailureClass::BadResponse,
    }
}

const SILENCE_RATE: SampleRate = SampleRate::HZ_16000;
const SILENCE_FRAME: usize = 320;

/// Open a recognition stream for `lang`, feed one second of silence and finalise: ok when the
/// final text is empty.
pub fn test_recogniser(provider: &dyn StreamingAsr, lang: Lang, timeout: Duration) -> TestReport {
    let started = Instant::now();
    let (outcome, timed_out) = with_deadline(timeout, |cancel| {
        let mut stream = provider.open(lang, AsrContext::default())?;
        let frame = [0.0f32; SILENCE_FRAME];
        for i in 0..(SILENCE_RATE.hz() as usize / SILENCE_FRAME) {
            stream.feed(&frame, MonoTime::from_millis(i as u64 * 20))?;
        }
        stream.finalize(cancel)
    });
    silence_report(outcome.map(|h| h.text), timed_out, started)
}

/// Decode one second of silence through the refine pass: ok when the text is empty.
pub fn test_refiner(provider: &dyn RefineAsr, lang: Lang, timeout: Duration) -> TestReport {
    let started = Instant::now();
    let audio = vec![0.0f32; SILENCE_RATE.hz() as usize];
    let (outcome, timed_out) = with_deadline(timeout, |cancel| {
        provider.transcribe(&audio, SILENCE_RATE, lang, &AsrContext::default(), cancel)
    });
    silence_report(outcome.map(|h| h.text), timed_out, started)
}

fn silence_report(outcome: Result<String, AsrError>, timed_out: bool, started: Instant) -> TestReport {
    match outcome {
        Ok(text) if text.trim().is_empty() => TestReport::success(None, started, String::new()),
        Ok(text) => TestReport::failure(
            None,
            started,
            FailureClass::BadResponse,
            format!("silence was recognised as `{text}`"),
        ),
        Err(e) => {
            let class = if timed_out { FailureClass::Timeout } else { classify_asr(&e) };
            TestReport::failure(None, started, class, e.to_string())
        }
    }
}

/// Open an engine and score one frame of silence: ok when both succeed.
pub fn test_vad(provider: &dyn VadProvider) -> TestReport {
    let started = Instant::now();
    let result = provider.open().and_then(|mut engine| {
        let frame = vec![0.0f32; engine.frame_len()];
        engine.probability(&frame)
    });
    match result {
        Ok(p) if (0.0..=1.0).contains(&p) => {
            TestReport::success(None, started, format!("probability {p:.2} on silence"))
        }
        Ok(p) => TestReport::failure(
            None,
            started,
            FailureClass::BadResponse,
            format!("probability {p} is outside 0 to 1"),
        ),
        Err(e) => TestReport::failure(None, started, FailureClass::Unavailable, e.to_string()),
    }
}
