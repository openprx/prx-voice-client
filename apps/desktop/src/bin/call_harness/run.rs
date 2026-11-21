//! One utterance through one direction of the real pipeline: a paced file as the source, the real mixer and pump, a WAV file or the virtual microphone as the sink.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use prxvc_audio::call_pump::{
    EventQueue, FrameTap, PlaybackSignal, Pump, PumpConfig, PumpLink, PumpRoute, PumpSpec,
};
use prxvc_audio::mix::{MixMode, Mixer, MixerConfig};
use prxvc_audio::resample::MonoResampler;
use prxvc_media::{ClauseId, Lang, MonoTime, SampleRate, SystemClock};
use prxvc_ports::{AsrContext, AudioSink, CancellationToken, RefineAsr, VadProvider};
use prxvc_session::ClauseState;
use prxvc_session::pipeline::{
    DirectionPipeline, PipelineEvent, PipelineParams, PipelineProviders,
};

use crate::call_kit::trace::{CallTrace, ClauseTimeline};
use crate::report::{RoundTrip, UtteranceRun, error_rate};
use crate::sinks::{Recorder, WavSink, path_of, write_wav};
use crate::source::{PacedFileSource, Recording};

/// Silence before and after the speech (the prototype's 0.3 s and 2 s).
const LEAD: Duration = Duration::from_millis(300);
const TAIL: Duration = Duration::from_secs(2);
/// Quiet time after the last clause played before the run counts as over.
const SETTLE: Duration = Duration::from_millis(800);
/// What the sink holds while the pump only feeds it silence (the pump keeps it about 40 ms ahead).
const SINK_IDLE: Duration = Duration::from_millis(100);
/// How long past the end of the feeding a run may take before it is given up.
const GIVE_UP: Duration = Duration::from_secs(60);

/// Where the translated speech goes.
#[derive(Clone)]
pub(crate) enum Output {
    /// A file per utterance in this directory.
    Wav(std::path::PathBuf),
    /// The virtual microphone, recorded and recognised again.
    VirtualMic {
        /// Directory for the recordings.
        dir: std::path::PathBuf,
        /// Recognises the recording (the refine model of the target language).
        recogniser: Arc<dyn RefineAsr>,
    },
}

/// What a run needs besides the utterance.
pub(crate) struct Rig {
    pub clock: Arc<SystemClock>,
    pub params: PipelineParams,
    pub providers: PipelineProviders,
    pub output: Output,
    /// The mixer's settings (jitter buffer, adaptive pre-roll); the mode is set per run.
    pub mixer: MixerConfig,
}

/// Speech start and end in the recording, from the VAD the pipeline uses (probability of at least
/// 0.5); `None` when no frame reaches it (a noise-only input).
pub(crate) fn speech_bounds(
    vad: &dyn VadProvider,
    rec: &Recording,
) -> Result<Option<(f64, f64)>, String> {
    let mut engine = vad.open().map_err(|e| e.to_string())?;
    let rate = engine.rate();
    let samples = if rec.rate == rate {
        rec.samples.clone()
    } else {
        let mut r = MonoResampler::new(rec.rate.hz(), rate.hz()).map_err(|e| e.to_string())?;
        let mut s = r.stream_write(&rec.samples).map_err(|e| e.to_string())?;
        s.extend(r.stream_finish().map_err(|e| e.to_string())?);
        s
    };
    let n = engine.frame_len();
    let frame_s = n as f64 / f64::from(rate.hz());
    let (mut first, mut last) = (None, None);
    for (i, f) in samples.chunks_exact(n).enumerate() {
        if engine.probability(f).map_err(|e| e.to_string())? >= 0.5 {
            first.get_or_insert(i);
            last = Some(i);
        }
    }
    match (first, last) {
        (Some(a), Some(b)) => Ok(Some((a as f64 * frame_s, (b + 1) as f64 * frame_s))),
        _ => Ok(None),
    }
}

fn mono_16k(samples: &[f32], rate: SampleRate) -> Result<Vec<f32>, String> {
    if rate.hz() == 16_000 {
        return Ok(samples.to_vec());
    }
    let mut r = MonoResampler::new(rate.hz(), 16_000).map_err(|e| e.to_string())?;
    let mut s = r.stream_write(samples).map_err(|e| e.to_string())?;
    s.extend(r.stream_finish().map_err(|e| e.to_string())?);
    Ok(s)
}

/// Run `rec` through the pipeline in real time and collect what happened.
pub(crate) fn run_utterance(
    rig: &Rig,
    id: &str,
    reference: &str,
    rec: &Recording,
    bounds: (f64, f64),
) -> Result<UtteranceRun, String> {
    let trace = CallTrace::collecting();
    let call = CancellationToken::new();
    let started = DirectionPipeline::start(
        rig.params.clone(),
        rig.providers.clone(),
        Arc::clone(&rig.clock) as _,
        Arc::clone(&trace) as _,
        &call,
    )
    .map_err(|e| e.to_string())?;
    let pipeline = Arc::new(started.pipeline);
    let snaps = started.snapshots;

    // The sink: a file, or the virtual microphone with a recorder started first.
    let (sink, captured, recorder): (Box<dyn AudioSink>, _, _) = match &rig.output {
        Output::Wav(_) => {
            let (sink, captured) = WavSink::new(SampleRate::HZ_48000);
            (Box::new(sink), Some(captured), None)
        }
        Output::VirtualMic { .. } => {
            let recorder = Recorder::start()?;
            std::thread::sleep(Duration::from_millis(300)); // let the capture settle
            let sink = prxvc_audio::virtual_mic::VirtualMicSink::open(
                &prxvc_audio::virtual_mic::VirtualMicOptions::from_config("", None),
            )
            .map_err(|e| e.to_string())?;
            (Box::new(sink), None, Some(recorder))
        }
    };
    let source = PacedFileSource::new(rec, LEAD, TAIL, Arc::clone(&rig.clock));
    let (began, fed) = (Arc::clone(&source.began), Arc::clone(&source.finished));
    let lead_s = source.lead_seconds();
    let mut pump = Pump::start(PumpSpec {
        config: PumpConfig::new("harness"),
        source: Box::new(source),
        route: PumpRoute::Mix {
            mixer: Box::new(Mixer::new(MixerConfig {
                mode: MixMode::Translation,
                ..rig.mixer
            })),
            sink,
        },
        events: EventQueue::new(),
        echo: None,
    })
    .map_err(|e| e.to_string())?;
    let input = pipeline.audio();
    let report = Arc::clone(&pipeline);
    let feed = pump
        .connect(PumpLink {
            tap: FrameTap {
                rate: SampleRate::HZ_16000,
                sink: Box::new(move |f| {
                    let _ = input.push(f);
                }),
            },
            on_playback: Box::new(move |s| match s {
                PlaybackSignal::Started { clause, at } => report.playback_started(clause, at),
                PlaybackSignal::Finished { clause, at } => report.playback_finished(clause, at),
            }),
            clock: Arc::clone(&rig.clock) as _,
        })
        .ok_or("the pump has no mixer")?;

    // Events: translated audio into the mixer's lane; the rest is only counted.
    let events = started.events;
    let consumer = std::thread::Builder::new()
        .name("harness-events".into())
        .spawn(move || {
            let mut audio_samples: Vec<(ClauseId, f64)> = Vec::new();
            while let Some(event) = events.recv() {
                match event {
                    PipelineEvent::Audio(a) => {
                        let secs = a.payload.chunk.duration().as_secs_f64();
                        match audio_samples
                            .iter_mut()
                            .find(|(c, _)| *c == a.payload.clause)
                        {
                            Some(e) => e.1 += secs,
                            None => audio_samples.push((a.payload.clause, secs)),
                        }
                        feed.push(a.payload.clause, &a.payload.chunk);
                    }
                    PipelineEvent::AudioEnd(e) => feed.end_clause(e.payload),
                    // A clause that has not started playing leaves nothing in the lane (the
                    // coordinator's forwarder does the same).
                    PipelineEvent::Flush(prxvc_session::CancelAction::CancelClause(_)) => {}
                    PipelineEvent::Flush(_) => feed.flush(),
                    PipelineEvent::Failed { stage, fault } => {
                        eprintln!("pipeline failure: {stage:?} {fault:?}");
                    }
                    _ => {}
                }
            }
            audio_samples
        })
        .map_err(|e| e.to_string())?;

    // Wait until the feeding is over and every clause is finished and quiet for a while.
    let deadline = Instant::now() + Duration::from_secs_f64(rec.seconds()) + GIVE_UP;
    let mut quiet_since: Option<Instant> = None;
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let done = fed.load(Ordering::Acquire)
            && snaps.latest().is_some_and(|s| {
                s.activity == prxvc_session::Activity::Listening
                    && s.clauses.iter().all(|c| {
                        matches!(
                            c.state,
                            ClauseState::Played
                                | ClauseState::Cancelled
                                | ClauseState::Skipped
                                | ClauseState::Failed(_)
                        )
                    })
            })
            && pump.stats().sink_queued < SINK_IDLE;
        match (done, quiet_since) {
            (true, None) => quiet_since = Some(Instant::now()),
            (true, Some(t)) if t.elapsed() >= SETTLE => break,
            (false, _) => quiet_since = None,
            _ => {}
        }
        if Instant::now() > deadline {
            eprintln!("{id}: gave up waiting for the pipeline to finish");
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(300)); // the last samples leave the sink
    let stats = pump.stats();
    let _ = pump.stop(Duration::from_secs(2));
    let final_snapshot = snaps.latest();
    let stop = pipeline.stop(Duration::from_secs(5));
    if !stop.clean {
        eprintln!("{id}: the pipeline did not stop cleanly: {stop:?}");
    }
    let audio_by_clause = consumer.join().unwrap_or_default();
    let (recorded, recorded_rate) =
        recorder.map_or((Vec::new(), SampleRate::HZ_48000), Recorder::finish);

    let mut clauses: Vec<ClauseTimeline> = trace.timelines();
    clauses.sort_by_key(|c| (c.clause.segment().seq(), c.clause.index()));
    let text_of = |c: &ClauseTimeline, s| {
        c.texts
            .iter()
            .find(|(st, _)| *st == s)
            .map(|(_, t)| t.clone())
            .unwrap_or_default()
    };
    let source_text = clauses
        .iter()
        .map(|c| text_of(c, prxvc_ports::TraceStage::ClauseCommitted))
        .collect::<Vec<_>>()
        .join(" ");
    let translation = clauses
        .iter()
        .map(|c| text_of(c, prxvc_ports::TraceStage::MtDone))
        .collect::<Vec<_>>()
        .join(" ");
    let spoken = clauses
        .iter()
        .map(|c| text_of(c, prxvc_ports::TraceStage::TtsRequestSent))
        .collect::<Vec<_>>()
        .join(" ");
    let audio_seconds: Vec<f64> = clauses
        .iter()
        .map(|c| {
            audio_by_clause
                .iter()
                .find(|(k, _)| *k == c.clause)
                .map_or(0.0, |(_, s)| *s)
        })
        .collect();

    // Files and the round trip.
    let began_ns = began.lock().map_or(0.0, |t: MonoTime| t.as_nanos() as f64);
    let counters = final_snapshot
        .as_ref()
        .map(|s| s.counters)
        .unwrap_or_default();
    let mut round_trip = None;
    match &rig.output {
        Output::Wav(dir) => {
            if let Some(c) = captured {
                write_wav(&path_of(dir, id, ""), &c.lock(), SampleRate::HZ_48000)?;
            }
        }
        Output::VirtualMic { dir, recogniser } => {
            write_wav(&path_of(dir, id, "-vmic"), &recorded, recorded_rate)?;
            round_trip = Some(recognise(
                recogniser.as_ref(),
                &recorded,
                recorded_rate,
                rig.params.target,
                &spoken,
            )?);
        }
    }
    Ok(UtteranceRun {
        id: id.to_owned(),
        reference: reference.to_owned(),
        speech_start_ns: began_ns + (lead_s + bounds.0) * 1e9,
        speech_end_ns: began_ns + (lead_s + bounds.1) * 1e9,
        clauses,
        source_text,
        translation,
        spoken,
        audio_seconds,
        underruns: stats.translated_underruns,
        gap_ms: stats.translated_gap_ms,
        worst_gap_ms: stats.translated_worst_gap_ms,
        sink_underruns: stats.sink_underruns,
        runaway_synth: counters.runaway_synth,
        mt_suspect: counters.mt_suspect,
        clauses_failed: counters.clauses_failed,
        clauses_skipped: counters.skipped,
        final_rewrites: counters.final_rewrites,
        counters: format!("{counters:?}"),
        round_trip,
    })
}

/// Recognise the recording of the virtual microphone and compare it with what was spoken.
fn recognise(
    recogniser: &dyn RefineAsr,
    samples: &[f32],
    rate: SampleRate,
    lang: Lang,
    spoken: &str,
) -> Result<RoundTrip, String> {
    let peak = samples.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
    let audio = mono_16k(samples, rate)?;
    let seconds = audio.len() as f64 / 16_000.0;
    let hyp = recogniser
        .transcribe(
            &audio,
            SampleRate::HZ_16000,
            lang,
            &AsrContext::default(),
            &CancellationToken::new(),
        )
        .map_err(|e| e.to_string())?;
    Ok(RoundTrip {
        error_rate: error_rate(spoken, &hyp.text, lang),
        recognised: hyp.text,
        peak,
        seconds,
    })
}
