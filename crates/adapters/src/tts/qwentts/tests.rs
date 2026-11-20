//! Adapter tests against the scripted engine: stream protocol, concurrency and cancellation,
//! back-pressure, runaway handling, warm-up, shutdown and voices. No model, no GPU, no sleeps.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};

use parking_lot::Mutex;
use prxvc_media::{ClauseId, Direction, Lang, SampleRate, SegmentId};
use prxvc_ports::{
    CancellationToken, Egress, SpeechSynth, StreamingKind, TtsError, TtsItem, TtsRequest,
    TtsStream, VoiceReference, VoiceSelection,
};

use super::cap::{RunawayRetry, SAMPLES_PER_FRAME as FRAME};
use super::request::queue_capacity;
use super::seam::{CallEnd, EngineError};
use super::test_engine::{Probe, Script, TestEngine, emit, wait_or_abort};
use super::voice::{CacheKey, VoiceCache, reference_hash};
use super::{LoadError, QwenTts, QwenTtsConfig};

const SHORT: &str = "hello there"; // cap: 40 frames (the floor), queue: 48 chunks

fn cfg() -> QwenTtsConfig {
    QwenTtsConfig::new("test-model", "model-a")
}

fn start_with(script: Script, cfg: QwenTtsConfig) -> (QwenTts, Probe) {
    let (engine, probe) = TestEngine::new(script);
    let tts = QwenTts::start(Arc::new(engine), cfg).expect("start");
    (tts, probe)
}

fn steady(chunks: usize, samples: usize) -> Script {
    Box::new(move |_, ctl| {
        emit(ctl, chunks, samples);
        Ok(CallEnd::Completed)
    })
}

fn reference(profile: &str, secs: u32) -> VoiceReference {
    VoiceReference {
        profile_id: profile.to_owned(),
        samples: (0..24_000 * secs)
            .map(|i| (i as f32 * 0.02).sin() * 0.25)
            .collect(),
        rate: SampleRate::HZ_24000,
        transcript: None,
        lang: Lang::En,
    }
}

fn clone_of(id: &str) -> VoiceSelection {
    VoiceSelection::Cloned {
        profile_id: id.to_owned(),
    }
}

fn request(direction: Direction, text: &str, voice: VoiceSelection) -> TtsRequest {
    TtsRequest {
        clause: ClauseId::new(SegmentId::first(direction), 0),
        text: text.to_owned(),
        lang: Lang::En,
        voice,
    }
}

fn ready(script: Script, cfg: QwenTtsConfig) -> (QwenTts, Probe) {
    let (tts, probe) = start_with(script, cfg);
    tts.prepare_voice(&reference("me", 2)).expect("prepared");
    (tts, probe)
}

fn open(tts: &QwenTts, direction: Direction, text: &str) -> Box<dyn TtsStream> {
    tts.synthesize(request(direction, text, clone_of("me")))
        .expect("stream")
}

fn drain(stream: &mut dyn TtsStream) -> Vec<Result<TtsItem, TtsError>> {
    let never = CancellationToken::new();
    std::iter::from_fn(|| stream.next(&never)).collect()
}

fn chunk_values(items: &[Result<TtsItem, TtsError>]) -> Vec<f32> {
    items
        .iter()
        .filter_map(|i| match i {
            Ok(TtsItem::Chunk(c)) => c.samples.first().copied(),
            _ => None,
        })
        .collect()
}

#[test]
fn a_request_streams_chunks_then_done_then_ends() {
    let (tts, probe) = ready(steady(3, 2400), cfg());
    let mut s = open(&tts, Direction::Outgoing, SHORT);
    let items = drain(s.as_mut());
    assert_eq!(items.len(), 4);
    for (i, item) in items[..3].iter().enumerate() {
        let Ok(TtsItem::Chunk(c)) = item else {
            panic!("chunk expected: {item:?}")
        };
        assert_eq!(
            c.rate,
            SampleRate::HZ_24000,
            "the rate rides on every chunk"
        );
        assert_eq!(c.samples.len(), 2400);
        assert_eq!(c.samples[0], (i + 1) as f32, "chunks arrive in order");
    }
    assert_eq!(items[3], Ok(TtsItem::Done { audio_secs: 0.3 }));
    assert!(
        s.next(&CancellationToken::new()).is_none(),
        "None forever after Done"
    );
    assert_eq!(s.voice_used(), &clone_of("me"));
    assert_eq!(tts.stats().completed, tts.stats().requests);
    assert_eq!(probe.calls.lock().len(), 1);
}

#[test]
fn the_engine_receives_the_cap_the_voice_the_language_and_the_configured_seed() {
    let mut c = cfg();
    c.seed = Some(42);
    let (tts, probe) = ready(steady(1, 100), c);
    let mut zh = request(
        Direction::Incoming,
        "\u{4f60}\u{597d}\u{4e16}\u{754c}",
        clone_of("me"),
    );
    zh.lang = Lang::Zh;
    drain(tts.synthesize(zh).expect("stream").as_mut());
    let calls = probe.calls.lock().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].lang, Lang::Zh);
    assert_eq!(calls[0].seed, 42);
    assert_eq!(
        calls[0].max_new_tokens, 40,
        "four characters get the floor, never 2048"
    );
    assert_eq!(
        calls[0].speaker.len(),
        8,
        "the prepared embedding reaches the engine"
    );
}

#[test]
fn unseeded_calls_ask_the_runtime_for_a_random_seed() {
    let (tts, probe) = ready(steady(1, 100), cfg());
    drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert_eq!(probe.calls.lock()[0].seed, -1);
}

#[test]
fn cancelling_one_request_stops_it_while_the_other_direction_keeps_speaking() {
    let gate = Arc::new(AtomicBool::new(false));
    let (started_tx, started_rx) = channel::<String>();
    let (ended_tx, ended_rx) = channel::<String>();
    let started_tx = Mutex::new(started_tx);
    let ended_tx = Mutex::new(ended_tx);
    let script_gate = Arc::clone(&gate);
    let script: Script = Box::new(move |call, ctl| {
        emit(ctl, 1, 1920);
        send(&started_tx, call.text);
        if wait_or_abort(ctl, &script_gate) {
            send(&ended_tx, call.text);
            return Ok(CallEnd::Aborted);
        }
        ctl.on_chunk(&[9.0; 1920]);
        Ok(CallEnd::Completed)
    });
    let (tts, _probe) = ready(script, cfg());
    let mut out = open(&tts, Direction::Outgoing, "outgoing");
    let mut inc = open(&tts, Direction::Incoming, "incoming");
    let never = CancellationToken::new();
    assert!(matches!(out.next(&never), Some(Ok(TtsItem::Chunk(_)))));
    assert!(matches!(inc.next(&never), Some(Ok(TtsItem::Chunk(_)))));
    // Both calls are inside the engine at once: two workers, one per direction.
    let mut began = [
        started_rx.recv().expect("began"),
        started_rx.recv().expect("began"),
    ];
    began.sort();
    assert_eq!(began, ["incoming", "outgoing"]);

    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(out.next(&cancel), Some(Err(TtsError::Cancelled)));
    assert!(out.next(&never).is_none(), "a cancelled stream ends");
    assert_eq!(
        ended_rx.recv().expect("aborted"),
        "outgoing",
        "the engine call stopped"
    );

    gate.store(true, Ordering::SeqCst);
    let rest = drain(inc.as_mut());
    assert_eq!(chunk_values(&rest), vec![9.0]);
    assert_eq!(rest.last(), Some(&Ok(TtsItem::Done { audio_secs: 0.16 })));
    assert!(
        ended_rx.try_recv().is_err(),
        "the other request was never aborted"
    );
    let stats = tts.stats();
    assert_eq!((stats.completed, stats.cancelled), (stats.requests - 1, 1));
}

fn send(tx: &Mutex<Sender<String>>, text: &str) {
    // A closed receiver only means the test no longer watches.
    let _ = tx.lock().send(text.to_owned());
}

#[test]
fn dropping_a_stream_abandons_the_request_and_frees_the_worker() {
    let never_open = Arc::new(AtomicBool::new(false));
    let (ended_tx, ended_rx) = channel::<String>();
    let ended_tx = Mutex::new(ended_tx);
    let script: Script = Box::new(move |call, ctl| {
        if call.text == "stuck" {
            emit(ctl, 1, 1920);
            if wait_or_abort(ctl, &never_open) {
                send(&ended_tx, call.text);
                return Ok(CallEnd::Aborted);
            }
        }
        emit(ctl, 2, 1920);
        Ok(CallEnd::Completed)
    });
    let mut c = cfg();
    c.workers = 1;
    let (tts, _probe) = ready(script, c);
    let mut stuck = open(&tts, Direction::Outgoing, "stuck");
    assert!(matches!(
        stuck.next(&CancellationToken::new()),
        Some(Ok(TtsItem::Chunk(_)))
    ));
    drop(stuck);
    assert_eq!(ended_rx.recv().expect("aborted"), "stuck");
    // The single worker is free again.
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert!(matches!(items.last(), Some(Ok(TtsItem::Done { .. }))));
    assert_eq!(tts.stats().cancelled, 1);
}

#[test]
fn a_request_cancelled_before_a_worker_takes_it_never_reaches_the_engine() {
    let gate = Arc::new(AtomicBool::new(false));
    let script_gate = Arc::clone(&gate);
    let script: Script = Box::new(move |call, ctl| {
        if call.text == "first" {
            emit(ctl, 1, 1920);
            wait_or_abort(ctl, &script_gate);
        }
        Ok(CallEnd::Completed)
    });
    let mut c = cfg();
    c.workers = 1;
    let (tts, probe) = ready(script, c);
    let mut first = open(&tts, Direction::Outgoing, "first");
    let never = CancellationToken::new();
    assert!(matches!(first.next(&never), Some(Ok(TtsItem::Chunk(_)))));
    let queued = open(&tts, Direction::Incoming, "queued");
    drop(queued);
    gate.store(true, Ordering::SeqCst);
    drain(first.as_mut());
    drop(tts); // joins the worker: the queued job has been handled by now
    let texts: Vec<_> = probe.calls.lock().iter().map(|c| c.text.clone()).collect();
    assert_eq!(texts, ["first"], "the abandoned job was skipped");
}

#[test]
fn an_engine_failure_is_one_typed_error_then_the_stream_ends() {
    let script: Script = Box::new(|_, ctl| {
        emit(ctl, 1, 1920);
        Err(EngineError::Call {
            status: -3,
            message: "generation failed".to_owned(),
        })
    });
    let (tts, _p) = ready(script, cfg());
    let mut s = open(&tts, Direction::Outgoing, SHORT);
    let items = drain(s.as_mut());
    assert_eq!(items.len(), 2);
    assert!(matches!(&items[1], Err(TtsError::Internal(m)) if m.contains("generation failed")));
    assert!(s.next(&CancellationToken::new()).is_none());
    assert_eq!(tts.stats().failed, 1);
}

#[test]
fn a_panicking_engine_fails_the_request_and_the_worker_survives() {
    let script: Script = Box::new(|call, ctl| {
        assert!(call.text != "boom", "scripted panic");
        emit(ctl, 1, 1920);
        Ok(CallEnd::Completed)
    });
    let mut c = cfg();
    c.workers = 1;
    let (tts, _p) = ready(script, c);
    let items = drain(open(&tts, Direction::Outgoing, "boom").as_mut());
    assert!(matches!(items.as_slice(), [Err(TtsError::Internal(m))] if m.contains("panicked")));
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert!(matches!(items.last(), Some(Ok(TtsItem::Done { .. }))));
}

#[test]
fn a_slow_consumer_neither_blocks_the_engine_nor_loses_audio() {
    let (done_tx, done_rx) = channel::<()>();
    let done_tx = Mutex::new(done_tx);
    let script: Script = Box::new(move |_, ctl| {
        emit(ctl, 100, FRAME);
        // A closed receiver only means the test no longer watches.
        let _ = done_tx.lock().send(());
        Ok(CallEnd::Completed)
    });
    let (tts, _p) = ready(script, cfg());
    // 30 words: cap 315 frames, so 100 frames of audio is a normal clause.
    let text = "word ".repeat(30);
    let mut s = open(&tts, Direction::Outgoing, &text);
    // The engine finishes while the consumer has not read a single chunk.
    done_rx.recv().expect("engine returned without a reader");
    let items = drain(s.as_mut());
    let values = chunk_values(&items);
    assert_eq!(values.len(), 100, "no chunk lost");
    assert!(values.windows(2).all(|w| w[0] < w[1]), "order kept");
    assert_eq!(items.last(), Some(&Ok(TtsItem::Done { audio_secs: 8.0 })));
    assert_eq!(tts.stats().overruns, 0);
}

#[test]
fn a_queue_overflow_fails_the_request_visibly_after_the_audio_that_was_kept() {
    let (done_tx, done_rx) = channel::<()>();
    let done_tx = Mutex::new(done_tx);
    let script: Script = Box::new(move |_, ctl| {
        let mut end = CallEnd::Completed;
        for _ in 0..200 {
            if ctl.should_abort() {
                end = CallEnd::Aborted;
                break;
            }
            ctl.on_chunk(&[0.5; 10]); // far smaller than a frame: the queue sizing is defeated
        }
        // A closed receiver only means the test no longer watches.
        let _ = done_tx.lock().send(());
        Ok(end)
    });
    let (tts, _p) = ready(script, cfg());
    let mut stream = open(&tts, Direction::Outgoing, SHORT);
    // The consumer reads only after the engine has stopped, so the queue really fills.
    done_rx.recv().expect("engine returned");
    let items = drain(stream.as_mut());
    assert_eq!(
        chunk_values(&items).len(),
        queue_capacity(40),
        "everything queued is delivered"
    );
    assert!(
        matches!(items.last(), Some(Err(TtsError::Unavailable { reason })) if reason.contains("fell behind"))
    );
    assert_eq!(tts.stats().overruns, 1, "counted, not silent");
    assert_eq!(tts.stats().failed, 1);
}

fn runaway_script() -> Script {
    Box::new(|_, ctl| {
        for _ in 0..1000 {
            if ctl.should_abort() {
                return Ok(CallEnd::Aborted);
            }
            ctl.on_chunk(&vec![0.1; FRAME]);
        }
        Ok(CallEnd::Completed)
    })
}

#[test]
fn a_runaway_is_aborted_and_reported_with_the_audio_it_produced() {
    let (tts, probe) = ready(runaway_script(), cfg());
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    let Some(Err(TtsError::Runaway { produced_secs })) = items.last() else {
        panic!("runaway expected: {:?}", items.last())
    };
    assert!(
        (produced_secs - 3.12).abs() < 1e-3,
        "39 frames of 80 ms: {produced_secs}"
    );
    assert_eq!(
        chunk_values(&items).len(),
        39,
        "audio up to the guard was streamed"
    );
    assert_eq!(probe.calls.lock().len(), 1, "no retry by default");
    assert_eq!((tts.stats().runaways, tts.stats().retries), (1, 0));
}

#[test]
fn a_call_that_ends_at_its_cap_is_a_runaway_not_a_success() {
    let script: Script = Box::new(|call, ctl| {
        emit(ctl, call.max_new_tokens as usize, FRAME);
        Ok(CallEnd::Completed) // the runtime reports success at the cap
    });
    let (tts, _p) = ready(script, cfg());
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert!(
        matches!(items.last(), Some(Err(TtsError::Runaway { .. }))),
        "{:?}",
        items.last()
    );
    assert!(!items.iter().any(|i| matches!(i, Ok(TtsItem::Done { .. }))));
}

fn runaway_once_then_fine() -> (Script, Arc<AtomicUsize>) {
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&attempts);
    let script: Script = Box::new(move |call, ctl| {
        if seen.fetch_add(1, Ordering::SeqCst) == 0 {
            return runaway_script()(call, ctl);
        }
        emit(ctl, 5, FRAME);
        Ok(CallEnd::Completed)
    });
    (script, attempts)
}

#[test]
fn with_retry_enabled_a_runaway_is_retried_once_with_another_seed_and_only_the_good_audio_streams()
{
    let (script, attempts) = runaway_once_then_fine();
    let mut c = cfg();
    c.runaway_retry = RunawayRetry::Once;
    c.seed = Some(7);
    let (tts, probe) = ready(script, c);
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        chunk_values(&items),
        vec![1.0, 2.0, 3.0, 4.0, 5.0],
        "no runaway audio reached the consumer"
    );
    assert_eq!(items.last(), Some(&Ok(TtsItem::Done { audio_secs: 0.4 })));
    let seeds: Vec<i64> = probe.calls.lock().iter().map(|c| c.seed).collect();
    assert_eq!(seeds[0], 7);
    assert_ne!(seeds[1], 7, "the retry uses a different seed");
    assert!(seeds[1] >= 0);
    assert_eq!((tts.stats().runaways, tts.stats().retries), (1, 1));
}

#[test]
fn two_runaways_in_a_row_end_in_the_typed_error_and_no_audio() {
    let mut c = cfg();
    c.runaway_retry = RunawayRetry::Once;
    let (tts, probe) = ready(runaway_script(), c);
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert!(
        matches!(items.as_slice(), [Err(TtsError::Runaway { .. })]),
        "{items:?}"
    );
    assert_eq!(probe.calls.lock().len(), 2, "one retry, not more");
    assert_eq!(tts.stats().runaways, 2);
}

#[test]
fn without_the_retry_option_a_runaway_is_not_retried() {
    let (script, attempts) = runaway_once_then_fine();
    let (tts, _p) = ready(script, cfg());
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert!(matches!(items.last(), Some(Err(TtsError::Runaway { .. }))));
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[test]
fn empty_or_unusable_text_is_rejected_before_the_engine() {
    let (tts, probe) = ready(steady(1, 10), cfg());
    for text in ["", "   \n\t", "has\0nul"] {
        let r = tts.synthesize(request(Direction::Outgoing, text, clone_of("me")));
        assert_eq!(
            r.err(),
            Some(TtsError::Rejected { status: 400 }),
            "{text:?}"
        );
    }
    let long = "x".repeat(super::cap::MAX_TEXT_CHARS + 1);
    assert_eq!(
        tts.synthesize(request(Direction::Outgoing, &long, clone_of("me")))
            .err(),
        Some(TtsError::Rejected { status: 400 })
    );
    assert!(probe.calls.lock().is_empty());
}

#[test]
fn non_finite_audio_from_the_engine_fails_the_request() {
    let script: Script = Box::new(|_, ctl| {
        ctl.on_chunk(&[0.0, f32::NAN]);
        Ok(CallEnd::Completed)
    });
    let (tts, _p) = ready(script, cfg());
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert!(matches!(items.as_slice(), [Err(TtsError::Internal(m))] if m.contains("not finite")));
}

#[test]
fn an_engine_that_returns_without_audio_is_an_error() {
    let (tts, _p) = ready(steady(0, 0), cfg());
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert!(matches!(items.as_slice(), [Err(TtsError::Internal(m))] if m.contains("no audio")));
}

#[test]
fn a_full_job_queue_is_unavailable_not_a_wait() {
    let gate = Arc::new(AtomicBool::new(false));
    let script_gate = Arc::clone(&gate);
    let script: Script = Box::new(move |_, ctl| {
        emit(ctl, 1, FRAME);
        wait_or_abort(ctl, &script_gate);
        Ok(CallEnd::Completed)
    });
    let mut c = cfg();
    c.workers = 1;
    c.queue_depth = 1;
    let (tts, _p) = ready(script, c);
    let mut running = open(&tts, Direction::Outgoing, "running");
    assert!(matches!(
        running.next(&CancellationToken::new()),
        Some(Ok(_))
    ));
    let _queued = open(&tts, Direction::Incoming, "queued");
    let refused = tts.synthesize(request(Direction::Incoming, "third", clone_of("me")));
    assert!(
        matches!(refused.err(), Some(TtsError::Unavailable { reason }) if reason.contains("queue is full"))
    );
    gate.store(true, Ordering::SeqCst);
}

#[test]
fn warm_up_runs_before_the_engine_is_ready_and_its_audio_is_discarded() {
    let (tts, probe) = start_with(steady(2, FRAME), cfg());
    // One solo call, then two at once so the batched path is warm too.
    assert_eq!(probe.warm_up_calls.load(Ordering::SeqCst), 3);
    assert!(
        probe.calls.lock().is_empty(),
        "warm-up is not a user request"
    );
    assert_eq!(tts.stats().requests, 3);
    assert_eq!(tts.stats().completed, 3);
    let (single, probe1) = start_with(steady(2, FRAME), {
        let mut c = cfg();
        c.workers = 1;
        c
    });
    assert_eq!(probe1.warm_up_calls.load(Ordering::SeqCst), 1);
    drop(single);
}

#[test]
fn a_failed_warm_up_means_the_engine_is_not_ready_and_is_freed() {
    let (engine, probe) = TestEngine::new(steady(1, 10));
    let result = QwenTts::start(Arc::new(engine.failing_warm_up()), cfg());
    assert!(matches!(
        result,
        Err(LoadError::WarmUp(TtsError::Internal(_)))
    ));
    assert!(
        probe.dropped.load(Ordering::SeqCst),
        "the engine is released on a failed start"
    );
}

#[test]
fn a_warm_up_that_runs_away_is_tried_again_and_a_persistent_runaway_fails_the_load() {
    // Two runaways, then good: the engine becomes ready (a bug fix: the load used to fail).
    let (engine, probe) = TestEngine::new(steady(1, 10));
    let ready = QwenTts::start(Arc::new(engine.running_away_in_warm_up(2)), cfg());
    assert!(ready.is_ok(), "{:?}", ready.err());
    assert!(probe.warm_up_calls.load(Ordering::SeqCst) > 3);
    drop(ready);
    // A warm-up that always runs away is a failed load, with the typed runaway.
    let (engine, probe) = TestEngine::new(steady(1, 10));
    let result = QwenTts::start(Arc::new(engine.running_away_in_warm_up(1000)), cfg());
    assert!(matches!(
        result,
        Err(LoadError::WarmUp(TtsError::Runaway { .. }))
    ));
    assert!(probe.dropped.load(Ordering::SeqCst));
}

#[test]
fn start_refuses_settings_that_cannot_work() {
    let (engine, _p) = TestEngine::new(steady(1, 10));
    let engine = Arc::new(engine);
    let mut zero = cfg();
    zero.workers = 0;
    assert!(matches!(
        QwenTts::start(engine.clone(), zero),
        Err(LoadError::Config(_))
    ));
    let mut nohash = cfg();
    nohash.model_hash.clear();
    assert!(matches!(
        QwenTts::start(engine, nohash),
        Err(LoadError::Config(_))
    ));
}

#[test]
fn dropping_the_adapter_stops_in_flight_calls_joins_the_workers_and_then_frees_the_engine() {
    let never_open = Arc::new(AtomicBool::new(false));
    let script: Script = Box::new(move |_, ctl| {
        emit(ctl, 1, FRAME);
        if wait_or_abort(ctl, &never_open) {
            return Ok(CallEnd::Aborted);
        }
        Ok(CallEnd::Completed)
    });
    let (tts, probe) = ready(script, cfg());
    let mut s = open(&tts, Direction::Outgoing, SHORT);
    let never = CancellationToken::new();
    assert!(matches!(s.next(&never), Some(Ok(TtsItem::Chunk(_)))));
    assert_eq!(probe.in_flight.load(Ordering::SeqCst), 1);
    drop(tts);
    assert!(probe.dropped.load(Ordering::SeqCst), "the engine was freed");
    assert_eq!(
        probe.in_flight_when_dropped.load(Ordering::SeqCst),
        0,
        "no call was in flight when the engine was freed"
    );
    let rest = drain(s.as_mut());
    assert!(
        matches!(rest.last(), Some(Err(TtsError::Unavailable { reason })) if reason.contains("shutting down"))
    );
}

#[test]
fn the_descriptor_declares_native_local_streaming_at_24_khz_for_both_languages() {
    let (tts, _p) = start_with(steady(1, 10), cfg());
    let d = tts.descriptor();
    assert_eq!(d.id.as_str(), "qwentts");
    assert_eq!(d.model, "test-model");
    assert_eq!(d.streaming, StreamingKind::Native);
    assert!(d.require_native().is_ok());
    assert_eq!(d.egress, Egress::Local);
    assert!(d.require_pair(Lang::En, Lang::Zh).is_ok());
    assert!(d.require_rate(SampleRate::HZ_24000).is_ok());
    assert!(d.require_rate(SampleRate::HZ_16000).is_err());
    let _dyn: Box<dyn SpeechSynth> = Box::new(tts);
}

#[test]
fn voices_must_be_prepared_and_stop_working_once_released() {
    let (tts, _p) = start_with(steady(1, FRAME), cfg());
    let unknown = tts.synthesize(request(Direction::Outgoing, SHORT, clone_of("me")));
    assert_eq!(unknown.err(), Some(TtsError::UnknownVoice("me".to_owned())));
    let prepared = tts.prepare_voice(&reference("me", 2)).expect("prepared");
    assert_eq!(prepared.engine, "qwentts.cpp");
    assert_eq!(prepared.model_hash, "model-a");
    assert!(prepared.handle.ends_with(":me"));
    assert!(
        tts.synthesize(request(Direction::Outgoing, SHORT, clone_of("me")))
            .is_ok()
    );
    tts.release_voice(&prepared.handle).expect("released");
    assert!(
        tts.synthesize(request(Direction::Outgoing, SHORT, clone_of("me")))
            .is_err()
    );
    assert_eq!(
        tts.release_voice(&prepared.handle),
        Err(TtsError::UnknownVoice(prepared.handle.clone()))
    );
    let preset = VoiceSelection::Preset {
        name: "nobody".to_owned(),
    };
    assert_eq!(
        tts.synthesize(request(Direction::Outgoing, SHORT, preset))
            .err(),
        Some(TtsError::UnknownVoice("nobody".to_owned()))
    );
}

#[test]
fn a_bad_reference_is_refused_before_the_engine() {
    let (tts, probe) = start_with(steady(1, FRAME), cfg());
    let before = probe.extract_calls.load(Ordering::SeqCst);
    let mut short = reference("me", 2);
    short.samples.truncate(1000);
    assert!(tts.prepare_voice(&short).is_err());
    assert_eq!(probe.extract_calls.load(Ordering::SeqCst), before);
}

#[test]
fn the_voice_cache_skips_preparation_and_is_invalidated_by_the_reference_or_the_model() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = VoiceCache::new(dir.path());
    let (tts, probe) = start_with(steady(1, FRAME), cfg());
    let base = probe.extract_calls.load(Ordering::SeqCst);
    let r = reference("me", 3);

    let (v1, hit) = tts.prepare_voice_cached(&cache, &r).expect("prepared");
    assert!(!hit, "first start prepares");
    let (v2, hit) = tts.prepare_voice_cached(&cache, &r).expect("prepared");
    assert!(hit, "second start reads the cache");
    assert_eq!(v1, v2);
    assert_eq!(probe.extract_calls.load(Ordering::SeqCst), base + 1);
    assert!(
        tts.synthesize(request(Direction::Outgoing, SHORT, clone_of("me")))
            .is_ok()
    );

    let mut changed = r.clone();
    changed.samples[100] += 0.5;
    let (_, hit) = tts
        .prepare_voice_cached(&cache, &changed)
        .expect("prepared");
    assert!(!hit, "a changed reference is a miss");
    assert_eq!(probe.extract_calls.load(Ordering::SeqCst), base + 2);

    let (other, probe2) = TestEngine::new(steady(1, FRAME));
    let tts2 = QwenTts::start(Arc::new(other), QwenTtsConfig::new("test-model", "model-b"))
        .expect("start");
    let (v3, hit) = tts2
        .prepare_voice_cached(&cache, &changed)
        .expect("prepared");
    assert!(!hit, "a different model is a miss");
    assert_eq!(v3.model_hash, "model-b");
    assert_eq!(
        probe2.extract_calls.load(Ordering::SeqCst),
        2,
        "warm-up voice and this one"
    );
    let key = CacheKey {
        model_hash: "model-a".to_owned(),
        reference_sha256: reference_hash(&changed),
    };
    assert!(
        cache.lookup(&key).expect("lookup").is_some(),
        "the first model's entry is untouched"
    );
    assert!(
        tts2.install_prepared(&clone_of("me"), &v1).is_err(),
        "a voice prepared for another model cannot be used"
    );
}

#[test]
fn a_preset_is_a_prepared_voice_from_a_stored_reference_and_speaks_through_the_same_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = VoiceCache::new(dir.path().join("remote"));
    let (tts, probe) = start_with(steady(2, FRAME), cfg());
    let prepared = tts
        .register_preset("remote", &reference("ignored", 3), Some(&cache))
        .expect("preset");
    assert!(prepared.handle.contains("preset:remote"));
    let preset = VoiceSelection::Preset {
        name: "remote".to_owned(),
    };
    let mut s = tts
        .synthesize(request(Direction::Incoming, SHORT, preset.clone()))
        .expect("stream");
    assert_eq!(s.voice_used(), &preset);
    assert!(matches!(
        drain(s.as_mut()).last(),
        Some(Ok(TtsItem::Done { .. }))
    ));
    // A second adapter instance finds the cached form instead of preparing again.
    let before = probe.extract_calls.load(Ordering::SeqCst);
    tts.register_preset("remote", &reference("ignored", 3), Some(&cache))
        .expect("preset");
    assert_eq!(probe.extract_calls.load(Ordering::SeqCst), before);
    // Without a cache the preset is still prepared and usable.
    tts.register_preset("plain", &reference("ignored", 3), None)
        .expect("preset");
    tts.release_voice(&prepared.handle).expect("released");
    assert!(
        tts.synthesize(request(Direction::Incoming, SHORT, preset))
            .is_err()
    );
}

#[test]
fn a_cap_override_shrinks_the_cap_and_makes_a_normal_call_a_runaway() {
    let mut c = cfg();
    c.cap_frames_override = Some(5);
    let (tts, probe) = ready(steady(6, FRAME), c);
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert!(
        matches!(items.last(), Some(Err(TtsError::Runaway { .. }))),
        "{:?}",
        items.last()
    );
    assert_eq!(probe.calls.lock()[0].max_new_tokens, 5);
}
