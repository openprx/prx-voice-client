//! One utterance through the assistant's loop: a session of its own on providers that are already loaded.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use prxvc_config::AppConfig;
use prxvc_media::{Clock, MonoTime, PcmChunk, SampleRate, SystemClock};
use prxvc_ports::{ConversationId, TraceEvent, TraceSink, TraceStage};
use prxvc_session::assistant::{
    AssistantDeps, AssistantEvent, OpenGate, Phase, ReplyOutput, start,
};

use crate::Item;
use crate::kit::Prepared;
use crate::params::session_params;
use crate::report::Row;
use crate::sinks::write_wav;
use crate::source::{PacedFileSource, Recording};

/// Silence before and after the speech, as the call harness feeds it.
const LEAD: Duration = Duration::from_millis(300);
const TAIL: Duration = Duration::from_secs(2);
/// A run that does not finish within this is reported as failed.
const GIVE_UP: Duration = Duration::from_secs(90);

#[derive(Default)]
struct Trace(Mutex<Vec<TraceEvent>>);

impl TraceSink for Trace {
    fn wants_text(&self) -> bool {
        false
    }
    fn record(&self, ev: TraceEvent) {
        self.0.lock().push(ev);
    }
}

/// A silent output: it keeps the audio, remembers when the first of it arrived and is drained as
/// soon as the reply's audio is complete.
struct SilentOutput {
    clock: Arc<SystemClock>,
    first: Arc<Mutex<Option<MonoTime>>>,
    samples: Arc<Mutex<Vec<f32>>>,
    rate: Arc<Mutex<SampleRate>>,
    ended: Arc<AtomicBool>,
}

impl ReplyOutput for SilentOutput {
    fn play(&mut self, chunk: &PcmChunk) -> Result<(), String> {
        self.first.lock().get_or_insert_with(|| self.clock.now());
        *self.rate.lock() = chunk.rate;
        self.samples.lock().extend_from_slice(&chunk.samples);
        self.ended.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn end_stream(&mut self) {
        self.ended.store(true, Ordering::SeqCst);
    }
    fn flush(&mut self) {
        self.ended.store(true, Ordering::SeqCst);
    }
    fn is_drained(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }
}

pub(crate) fn run_utterance(
    config: &AppConfig,
    prepared: &Prepared,
    clock: &Arc<SystemClock>,
    item: &Item,
    rec: &Recording,
    out: &Path,
) -> Result<Row, String> {
    let trace = Arc::new(Trace::default());
    let first = Arc::new(Mutex::new(None));
    let samples = Arc::new(Mutex::new(Vec::new()));
    let rate = Arc::new(Mutex::new(SampleRate::HZ_24000));
    let source = PacedFileSource::new(rec, LEAD, TAIL, Arc::clone(clock));
    let began = Arc::clone(&source.began);
    let params = session_params(
        config,
        prepared.lang,
        prepared.voice.clone(),
        ConversationId::new(format!("harness-{}", item.id)),
    );
    let deps = AssistantDeps {
        source: Box::new(source),
        vad: Arc::clone(&prepared.vad),
        asr: Arc::clone(&prepared.asr),
        dialogue: Arc::clone(&prepared.dialogue),
        tts: Arc::clone(&prepared.tts),
        output: Box::new(SilentOutput {
            clock: Arc::clone(clock),
            first: Arc::clone(&first),
            samples: Arc::clone(&samples),
            rate: Arc::clone(&rate),
            ended: Arc::new(AtomicBool::new(true)),
        }),
        gate: Box::new(OpenGate),
        clock: Arc::clone(clock) as Arc<dyn Clock>,
        trace: trace.clone(),
    };
    let handle = start(deps, params).map_err(|e| e.to_string())?;
    let mut recognised = String::new();
    let mut reply = String::new();
    let mut final_at: Option<MonoTime> = None;
    let mut failure = None;
    let mut note = None;
    let mut heard = false;
    let deadline = Instant::now() + GIVE_UP;
    loop {
        if Instant::now() > deadline {
            failure = Some("gave up waiting for the reply".to_owned());
            break;
        }
        let Some(event) = handle.next_event(Duration::from_millis(50)) else {
            continue;
        };
        match event {
            AssistantEvent::Phase(Phase::UserSpeaking) => heard = true,
            // Speech that ended with nothing but filler makes no turn: that is the result.
            AssistantEvent::Phase(Phase::Listening) if heard && final_at.is_none() => {
                note = Some("no turn: nothing but filler was heard".to_owned());
                break;
            }
            AssistantEvent::UserTurn { text } => {
                final_at = Some(clock.now());
                recognised = text;
            }
            AssistantEvent::ReplyDelta { text } => reply.push_str(&text),
            AssistantEvent::ReplyDone => break,
            AssistantEvent::Fault(fault) => {
                failure = Some(fault.to_string());
                break;
            }
            AssistantEvent::Ended => break,
            _ => {}
        }
    }
    handle.stop();
    handle.join(|_| {});

    // The end of the speech: the last window of the recording that is not quiet, after the lead silence.
    let origin = began.lock().unwrap_or(MonoTime::ZERO);
    let speech_end = origin.saturating_add(LEAD + Duration::from_secs_f64(speech_seconds(rec)));
    let token_at = trace
        .0
        .lock()
        .iter()
        .find(|e| e.stage == TraceStage::MtFirstToken)
        .map(|e| e.at);
    let audio = samples.lock().clone();
    let reply_rate = *rate.lock();
    if !audio.is_empty() {
        write_wav(
            &out.join(format!("{}.reply.wav", item.id)),
            &audio,
            reply_rate,
        )?;
    }
    let to = |t: Option<MonoTime>| t.map(|t| t.saturating_since(speech_end));
    Ok(Row {
        id: item.id.clone(),
        reference: item.reference.clone(),
        recognised,
        reply,
        speech_s: rec.seconds(),
        to_final: to(final_at),
        to_first_token: to(token_at),
        to_first_audio: to(*first.lock()),
        reply_audio_s: reply_rate.duration_of(audio.len()).as_secs_f64(),
        failure: failure.or_else(|| {
            (audio.is_empty() && note.is_none()).then(|| "the reply had no audio".to_owned())
        }),
        note,
    })
}

/// Where the speech of `rec` ends, in seconds: the end of the last 20 ms window whose level is at
/// least a tenth of the loudest window's (a recording may carry quiet seconds after the last word).
fn speech_seconds(rec: &Recording) -> f64 {
    let window = rec.rate.samples_in(Duration::from_millis(20)).max(1);
    let rms: Vec<f32> = rec
        .samples
        .chunks(window)
        .map(|w| (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt())
        .collect();
    let loudest = rms.iter().copied().fold(0.0f32, f32::max);
    let last = rms
        .iter()
        .rposition(|r| *r >= loudest * 0.1)
        .map_or(0, |i| i + 1);
    (last * window).min(rec.samples.len()) as f64 / f64::from(rec.rate.hz())
}
