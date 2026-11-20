//! Live checks of the two native runtimes in one process on Metal (CT-9). All tests are ignored:
//! they need the benchmark models (used in place, see `prxvc_native_link::models`) and the GPU.
//!
//! ```text
//! cargo test -p prxvc-native-link --features qwentts,transcribe --release --test live \
//!     -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `PRXVC_BENCH_ROOT` overrides the benchmark directory. Nothing is played or recorded.
#![cfg(all(feature = "qwentts", feature = "transcribe"))]

use prxvc_native_link::asr::{self, AsrModel, AsrSession};
use prxvc_native_link::tts::{self, Request, TtsEngine, Voice};
use prxvc_native_link::{models, resample, text, wav};
use prxvc_qwentts_sys::QT_STATUS_CANCELLED;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const SENTENCE: &str = "The meeting has been moved to Thursday afternoon, so please update your calendar before the call.";

fn ms(d: Option<Duration>) -> f64 {
    d.map_or(f64::NAN, |d| d.as_secs_f64() * 1000.0)
}

fn load_engine(max_batch: i32) -> (TtsEngine, Voice) {
    let t0 = Instant::now();
    let engine = TtsEngine::load(&models::talker_1_7b(), &models::codec(), max_batch)
        .expect("load TTS models");
    println!(
        "tts: loaded 1.7B Q8_0 in {:.0} ms, model type {}",
        t0.elapsed().as_secs_f64() * 1000.0,
        engine.model_type()
    );
    let reference = wav::read(&models::en_owner_wav()).expect("read the owner's English reference");
    assert_eq!(reference.sample_rate, tts::SAMPLE_RATE);
    let t0 = Instant::now();
    let voice = engine
        .prepare_voice(&reference.samples)
        .expect("prepare the voice");
    println!(
        "tts: voice prepared in {:.0} ms (embedding dim {})",
        t0.elapsed().as_secs_f64() * 1000.0,
        voice.embedding_dim()
    );
    (engine, voice)
}

fn request(text: &str, seed: i64) -> Request<'_> {
    Request {
        text,
        language: "english",
        seed,
        max_new_tokens: tts::max_new_tokens_for(text),
    }
}

fn owner_16k() -> Vec<f32> {
    let w = wav::read(&models::en_owner_wav()).expect("read the owner's English reference");
    resample::resample(&w.samples, w.sample_rate, asr::SAMPLE_RATE)
}

/// Throw-away synthesis: the first synthesis in a process is slow (bench REPORT section 2).
fn warm_up(engine: &TtsEngine, voice: &Voice) {
    let t0 = Instant::now();
    let r = engine
        .synthesize(voice, &request("Warm up.", 1), None)
        .expect("warm-up synthesis");
    println!(
        "tts: warm-up synthesis {:.0} ms, first chunk {:.0} ms",
        t0.elapsed().as_secs_f64() * 1000.0,
        ms(r.time_to_first_chunk)
    );
}

fn check_audio(s: &tts::Synthesis) {
    let expected = text::words(SENTENCE).len() as f64 * 0.28;
    let got = s.audio_seconds();
    assert!(!s.pcm.is_empty(), "no audio");
    assert!(
        got > expected * 0.5 && got < expected * 2.0,
        "audio of {got:.2} s for an expected {expected:.2} s"
    );
    assert!(s.pcm.iter().all(|v| v.is_finite()));
    let peak = s.pcm.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.05, "silent output (peak {peak})");
}

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn tts_streams_plausible_audio_alone() {
    let (engine, voice) = load_engine(2);
    warm_up(&engine, &voice);
    for i in 0..3 {
        let s = engine
            .synthesize(&voice, &request(SENTENCE, 100 + i), None)
            .expect("synthesis");
        check_audio(&s);
        println!(
            "tts alone #{i}: first chunk {:.1} ms, {} chunks, audio {:.2} s, total {:.0} ms, rtf {:.3}",
            ms(s.time_to_first_chunk),
            s.chunks,
            s.audio_seconds(),
            s.total.as_secs_f64() * 1000.0,
            s.rtf()
        );
    }
}

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn tts_cancel_returns_and_the_engine_stays_usable() {
    let (engine, voice) = load_engine(1);
    warm_up(&engine, &voice);
    let flag = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });
        let long = format!("{SENTENCE} {SENTENCE} {SENTENCE}");
        let t0 = Instant::now();
        let s = engine
            .synthesize(&voice, &request(&long, 5), Some(&flag))
            .expect("cancelled synthesis");
        println!(
            "tts cancel: status {} after {:.0} ms with {:.2} s of audio",
            s.status,
            t0.elapsed().as_secs_f64() * 1000.0,
            s.audio_seconds()
        );
        assert_eq!(s.status, QT_STATUS_CANCELLED);
    });
    let s = engine
        .synthesize(&voice, &request(SENTENCE, 6), None)
        .expect("synthesis after cancel");
    check_audio(&s);
}

#[test]
#[ignore = "needs the ASR model and the GPU"]
fn asr_recognises_the_owner_reference_alone() {
    let t0 = Instant::now();
    let model = AsrModel::load(&models::asr_qwen3_0_6b()).expect("load ASR model");
    println!(
        "asr: loaded Qwen3-ASR 0.6B Q8_0 in {:.0} ms on {}",
        t0.elapsed().as_secs_f64() * 1000.0,
        model.backend()
    );
    let mut session = AsrSession::new(&model).expect("session");
    let pcm = owner_16k();
    let reference = models::en_owner_text();
    for i in 0..3 {
        let t0 = Instant::now();
        let hyp = session.run(&pcm, Some("en")).expect("run");
        let wer = text::wer(&reference, &hyp);
        println!(
            "asr alone #{i}: {:.0} ms for {:.2} s (rtf {:.3}), wer {wer:.3}: {hyp}",
            t0.elapsed().as_secs_f64() * 1000.0,
            pcm.len() as f64 / 16000.0,
            t0.elapsed().as_secs_f64() / (pcm.len() as f64 / 16000.0)
        );
        assert!(
            wer <= 0.15,
            "recognised text too far from the known transcript: {hyp}"
        );
    }
}

#[test]
#[ignore = "needs the streaming ASR model and the GPU"]
fn asr_streaming_api_produces_text_before_the_end() {
    let model = AsrModel::load(&models::asr_nemotron_en()).expect("load streaming ASR model");
    let mut session = AsrSession::new(&model).expect("session");
    let pcm = owner_16k();
    let r = session
        .run_streaming(&pcm, 2560, Some("en"))
        .expect("streaming run");
    let wer = text::wer(&models::en_owner_text(), &r.text);
    println!(
        "asr streaming: {:.0} ms, first text after chunk {:?} of {}, wer {wer:.3}: {}",
        r.elapsed.as_secs_f64() * 1000.0,
        r.first_text_chunk,
        pcm.len().div_ceil(2560),
        r.text
    );
    assert!(
        r.first_text_chunk
            .is_some_and(|c| c + 2 < pcm.len().div_ceil(2560)),
        "no text before the end of the audio"
    );
    assert!(
        wer <= 0.3,
        "streamed text too far from the known transcript: {}",
        r.text
    );
}

/// The CT-9 acceptance run: TTS and ASR at the same time on Metal in one process, then the
/// synthesized audio through the ASR (round trip), everything freed, clean exit.
#[test]
#[ignore = "needs the TTS and ASR models and the GPU"]
fn tts_and_asr_run_concurrently_and_round_trip() {
    let (engine, voice) = load_engine(2);
    let engine = Arc::new(engine);
    let t0 = Instant::now();
    let model = AsrModel::load(&models::asr_qwen3_0_6b()).expect("load ASR model");
    println!(
        "asr: loaded in {:.0} ms on {}",
        t0.elapsed().as_secs_f64() * 1000.0,
        model.backend()
    );
    let mut asr_a = AsrSession::new(&model).expect("session a");
    let mut asr_b = AsrSession::new(&model).expect("session b");
    let owner = owner_16k();
    let reference = models::en_owner_text();

    // Warm-ups, then the solo baselines inside this process.
    warm_up(&engine, &voice);
    let t0 = Instant::now();
    asr_a.run(&owner, Some("en")).expect("asr warm-up");
    let asr_solo = t0.elapsed();
    let solo = engine
        .synthesize(&voice, &request(SENTENCE, 11), None)
        .expect("solo synthesis");
    println!(
        "solo: tts first chunk {:.1} ms, rtf {:.3}, audio {:.2} s; asr decode {:.0} ms",
        ms(solo.time_to_first_chunk),
        solo.rtf(),
        solo.audio_seconds(),
        asr_solo.as_secs_f64() * 1000.0
    );

    // Concurrent: the recognizer decodes the owner reference in a loop for as long as the synthesis runs.
    let tts_running = AtomicBool::new(true);
    let asr_started = AtomicUsize::new(0);
    let (synth, asr_runs) = std::thread::scope(|scope| {
        let asr_thread = scope.spawn(|| {
            let mut runs = Vec::new();
            loop {
                asr_started.fetch_add(1, Ordering::SeqCst);
                let t0 = Instant::now();
                let hyp = asr_a.run(&owner, Some("en")).expect("concurrent asr run");
                runs.push((hyp, t0.elapsed()));
                if !tts_running.load(Ordering::SeqCst) && runs.len() >= 2 {
                    break;
                }
            }
            runs
        });
        while asr_started.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        let synth = engine
            .synthesize(&voice, &request(SENTENCE, 12), None)
            .expect("concurrent synthesis");
        tts_running.store(false, Ordering::SeqCst);
        (synth, asr_thread.join().expect("asr thread"))
    });
    check_audio(&synth);
    println!(
        "concurrent: tts first chunk {:.1} ms, rtf {:.3}, audio {:.2} s, {} chunks; {} asr decodes, median {:.0} ms",
        ms(synth.time_to_first_chunk),
        synth.rtf(),
        synth.audio_seconds(),
        synth.chunks,
        asr_runs.len(),
        {
            let mut t: Vec<f64> = asr_runs
                .iter()
                .map(|(_, d)| d.as_secs_f64() * 1000.0)
                .collect();
            t.sort_by(f64::total_cmp);
            t[t.len() / 2]
        }
    );
    for (hyp, _) in &asr_runs {
        let wer = text::wer(&reference, hyp);
        assert!(
            wer <= 0.15,
            "concurrent recognition drifted (wer {wer:.3}): {hyp}"
        );
    }

    // Round trip: the synthesized audio through the recognizer.
    let heard = resample::resample(&synth.pcm, tts::SAMPLE_RATE, asr::SAMPLE_RATE);
    let hyp = asr_b
        .run(&heard, Some("en"))
        .expect("round-trip recognition");
    let wer = text::wer(SENTENCE, &hyp);
    println!("round trip: wer {wer:.3}: {hyp}");
    assert!(!hyp.is_empty());
    assert!(
        wer <= 0.25,
        "round trip too far from the spoken sentence: {hyp}"
    );

    // Free everything in dependency order before the process exits (the Metal backend aborts otherwise).
    drop(asr_a);
    drop(asr_b);
    drop(model);
    drop(voice);
    drop(engine);
    println!(
        "CT9 summary: tts_first_chunk_ms={:.1} tts_rtf={:.3} solo_first_chunk_ms={:.1} solo_rtf={:.3} round_trip_wer={wer:.3}",
        ms(synth.time_to_first_chunk),
        synth.rtf(),
        ms(solo.time_to_first_chunk),
        solo.rtf()
    );
}
