//! Runs one utterance through a VAD gate and a recognition stream at real-time pace and measures it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use prxvc_adapters::vad::{GateParams, VadEvent, VadGate};
use prxvc_media::{Lang, MonoTime};
use prxvc_ports::{AsrContext, AsrHypothesis, CancellationToken, StreamingAsr, VadProvider};

use super::{errors, sleep_until, tokens};

const CHUNK: usize = 320; // 20 ms
const TAIL_SILENCE: Duration = Duration::from_millis(1400);

/// What one utterance measured.
#[derive(Debug, Default)]
pub struct Utterance {
    pub id: String,
    pub audio_s: f64,
    pub final_text: String,
    pub errors: usize,
    pub ref_len: usize,
    pub partials: usize,
    /// Segments the VAD cut the utterance into (a pause longer than the segment-end pause).
    pub segments: usize,
    /// Partials whose text differs from the previous one before the end of the previous text.
    pub rewrites: usize,
    /// Partials after which an earlier declared stable prefix was no longer a prefix.
    pub retractions: usize,
    /// Declared stable prefixes that were not a prefix of the final text.
    pub stable_wrong: usize,
    pub first_partial_after_onset_ms: Option<f64>,
    pub first_partial_after_vad_ms: Option<f64>,
    /// Wall time from the end-of-speech event to the final hypothesis.
    pub final_after_endpoint_ms: f64,
    /// Wall time from the true end of speech to the final hypothesis.
    pub final_after_speech_end_ms: f64,
    /// Seconds before the end of speech at which the stable prefix reached 50 % and 90 % of the
    /// final text (None: never before the final).
    pub stable_50_early_s: Option<f64>,
    pub stable_90_early_s: Option<f64>,
    pub partial_gaps_ms: Vec<f64>,
}

fn is_prefix(a: &[String], b: &[String]) -> bool {
    b.len() >= a.len() && b[..a.len()] == *a
}

/// Feed `audio` (with `speech` = the oracle's start and end in seconds) at real-time pace.
pub fn run(
    asr: &dyn StreamingAsr,
    vad: &dyn VadProvider,
    lang: Lang,
    id: &str,
    audio: &[f32],
    speech: (f64, f64),
    reference: &str,
) -> Utterance {
    let mut gate = VadGate::new(vad.open().expect("vad"), GateParams::default());
    let mut stream = asr.open(lang, AsrContext::default()).expect("open");
    let mut full = audio.to_vec();
    full.extend(vec![0.0; (TAIL_SILENCE.as_secs_f64() * 16_000.0) as usize]);
    let t0 = Instant::now();
    let at = |secs: f64| t0 + Duration::from_secs_f64(secs);
    let mut u = Utterance {
        id: id.to_owned(),
        audio_s: audio.len() as f64 / 16_000.0,
        ..Utterance::default()
    };
    let mut hyps: Vec<(Instant, AsrHypothesis)> = Vec::new();
    let mut vad_start: Option<Instant> = None;
    let mut speaking = false;
    let mut end_event: Option<Instant> = None;
    let mut finished_text = String::new();
    let mut segments = 0usize;
    let mut last_end_audio = 0usize;
    let mut fed = 0usize;
    for (k, chunk) in full.chunks(CHUNK).enumerate() {
        sleep_until(t0 + Duration::from_secs_f64(((k + 1) * CHUNK) as f64 / 16_000.0));
        let ts = MonoTime::from_nanos((k * CHUNK) as u64 * 62_500);
        let events = gate.feed(chunk, ts).expect("gate");
        let now_samples = (k * CHUNK) + chunk.len();
        let mut stop = false;
        for e in events {
            match e {
                VadEvent::SpeechStart { at: onset, .. } => {
                    vad_start = Some(Instant::now());
                    speaking = true;
                    // The pre-roll comes from the owner of the audio: the pipeline's ring.
                    let from = (onset.as_nanos() / 62_500) as usize;
                    let pre = &full[from.min(now_samples)..now_samples];
                    fed = from;
                    if let Some(h) = stream
                        .feed(pre, MonoTime::from_nanos(from as u64 * 62_500))
                        .expect("feed")
                    {
                        hyps.push((Instant::now(), h));
                    }
                    fed = now_samples.max(fed);
                    continue;
                }
                VadEvent::SpeechEnd { end, .. } => {
                    end_event = Some(Instant::now());
                    last_end_audio = (end.as_nanos() / 62_500) as usize;
                    // A pause inside the utterance ended a segment: finalize it and keep going; the
                    // utterance's text is the concatenation, the latencies those of the last segment.
                    if (end.as_nanos() as f64 / 1e9) < speech.1 - 0.3 {
                        let fin = stream
                            .finalize(&CancellationToken::new())
                            .expect("finalize");
                        if !finished_text.is_empty() && !fin.text.is_empty() {
                            finished_text.push(' ');
                        }
                        finished_text.push_str(&fin.text);
                        segments += 1;
                        hyps.clear();
                        speaking = false;
                        end_event = None;
                        continue;
                    }
                    stop = true;
                }
            }
        }
        if stop {
            break;
        }
        if speaking && fed < now_samples {
            let ts = MonoTime::from_nanos(fed as u64 * 62_500);
            if let Some(h) = stream.feed(&full[fed..now_samples], ts).expect("feed") {
                hyps.push((Instant::now(), h));
            }
            fed = now_samples;
        }
    }
    let end_event = end_event.expect("the VAD never ended the segment");
    let mut fin = stream
        .finalize(&CancellationToken::new())
        .expect("finalize");
    let done = Instant::now();
    let _ = (last_end_audio, segments);
    let last_text = fin.text.clone();
    if !finished_text.is_empty() {
        fin.text = format!("{finished_text} {}", fin.text);
    }
    u.final_after_endpoint_ms = done.duration_since(end_event).as_secs_f64() * 1000.0;
    u.final_after_speech_end_ms =
        done.saturating_duration_since(at(speech.1)).as_secs_f64() * 1000.0;
    u.final_text = fin.text.clone();
    (u.errors, u.ref_len) = errors(reference, &fin.text);
    u.segments = segments + 1;
    let fin = AsrHypothesis {
        text: last_text,
        ..fin
    };
    if std::env::var_os("PRXVC_ASR_TRACE").is_some_and(|v| v.to_string_lossy().contains(id)) {
        for (when, h) in &hyps {
            println!(
                "  trace {id} +{:.2}s audio {:.2}s stable {:?} | {}",
                when.duration_since(t0).as_secs_f64(),
                h.audio_end.as_nanos() as f64 / 1e9,
                &h.text[..h.stable_prefix_len],
                h.text
            );
        }
        println!("  trace {id} final: {}", fin.text);
    }
    u.partials = hyps.len();
    let final_tokens = tokens(&fin.text);
    let mut prev_tokens: Vec<String> = Vec::new();
    let mut declared: Vec<String> = Vec::new();
    let mut last_at: Option<Instant> = None;
    for (when, h) in &hyps {
        if u.first_partial_after_onset_ms.is_none() {
            u.first_partial_after_onset_ms =
                Some(when.saturating_duration_since(at(speech.0)).as_secs_f64() * 1000.0);
            u.first_partial_after_vad_ms =
                vad_start.map(|v| when.saturating_duration_since(v).as_secs_f64() * 1000.0);
        }
        if let Some(l) = last_at {
            u.partial_gaps_ms
                .push(when.duration_since(l).as_secs_f64() * 1000.0);
        }
        last_at = Some(*when);
        let now_tokens = tokens(&h.text);
        if !is_prefix(&prev_tokens, &now_tokens) {
            u.rewrites += 1;
        }
        let stable = tokens(&h.text[..h.stable_prefix_len]);
        if !is_prefix(&declared, &stable) {
            u.retractions += 1;
        }
        if stable.len() > declared.len() {
            declared = stable.clone();
        }
        if !is_prefix(&stable, &final_tokens) {
            u.stable_wrong += 1;
        }
        let ratio = stable.len() as f64 / final_tokens.len().max(1) as f64;
        let early = speech.1 - h.audio_end.as_nanos() as f64 / 1e9;
        if ratio >= 0.5 && u.stable_50_early_s.is_none() && is_prefix(&stable, &final_tokens) {
            u.stable_50_early_s = Some(early);
        }
        if ratio >= 0.9 && u.stable_90_early_s.is_none() && is_prefix(&stable, &final_tokens) {
            u.stable_90_early_s = Some(early);
        }
        prev_tokens = now_tokens;
    }
    u
}
