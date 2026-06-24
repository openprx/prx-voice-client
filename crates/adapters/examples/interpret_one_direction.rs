//! End-to-end consecutive-interpreting demo for a single direction.
//!
//! Wires every layer for a single turn:
//!   CoreAudioTapSource → MonoResampler16k → WhisperAsr → InterpretDriver
//!   (Translator + SineToneSynthesizer) → BlackHoleSink playback
//!
//! Requires macOS 14.4+, the `integration` feature, and Whisper model files:
//!   export PRXVC_WHISPER_MODEL=/path/to/ggml-small.bin
//!   export PRXVC_VAD_MODEL=/path/to/ggml-silero-vad.bin
//! Optional real MT (else offline EchoTranslator is used):
//!   export PRXVC_MT_BASE_URL=http://localhost:11434/v1
//!   export PRXVC_MT_MODEL=qwen2.5:7b
//!   export PRXVC_MT_API_KEY=...            # cloud only
//!
//! Usage:
//!   cargo run -p prxvc-adapters --features integration \
//!     --example interpret_one_direction -- <PID> [self|peer] [SECONDS]
//!
//!   self : I speak (source=zh) → peer hears (en), cloned-voice tone
//!   peer : peer speaks (source=en) → I hear (zh)

#[cfg(all(target_os = "macos", feature = "integration"))]
fn main() {
    use std::time::{Duration, Instant};

    use prxvc_audio::blackhole_sink::BlackHoleSink;
    use prxvc_audio::core_audio_tap::CoreAudioTapSource;
    use prxvc_audio::resample::MonoResampler16k;
    use prxvc_audio::routing::{AppAudioSource, ProcessFilter, VirtualMicSink};
    use prxvc_audio::whisper_asr::{WhisperAsr, WhisperAsrConfig};
    use prxvc_audio::{AudioFormat, AudioFrame};

    use prxvc_adapters::{EchoTranslator, OpenAiTranslator, SineToneSynthesizer, Translator};
    use prxvc_session::SessionId;
    use prxvc_session::driver::InterpretDriver;
    use prxvc_session::interpret::{Direction, InterpretConfig, InterpretSession};

    // ---- args ----
    let mut args = std::env::args().skip(1);
    let pid: i32 = match args.next().and_then(|s| s.parse().ok()) {
        Some(p) => p,
        None => {
            eprintln!("usage: interpret_one_direction <PID> [self|peer] [SECONDS]");
            std::process::exit(2);
        }
    };
    let direction = match args.next().as_deref() {
        Some("peer") => Direction::PeerToSelf,
        _ => Direction::SelfToPeer,
    };
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(8);

    let cfg = InterpretConfig::default(); // zh self, en peer, clone on
    let source_lang = direction.source_language(&cfg).to_owned();

    // ---- ASR ----
    let model_path = std::env::var("PRXVC_WHISPER_MODEL").unwrap_or_default();
    let vad_model_path = std::env::var("PRXVC_VAD_MODEL").unwrap_or_default();
    if model_path.is_empty() || vad_model_path.is_empty() {
        eprintln!("set PRXVC_WHISPER_MODEL and PRXVC_VAD_MODEL");
        std::process::exit(2);
    }
    let mut asr = match WhisperAsr::new(&WhisperAsrConfig {
        model_path,
        vad_model_path,
        language: source_lang.clone(),
        ..Default::default()
    }) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ASR init failed: {e}");
            std::process::exit(1);
        }
    };

    // ---- driver (MT + TTS) ----
    let translator: Box<dyn Translator + Send> = if OpenAiTranslator::is_configured() {
        match OpenAiTranslator::from_env() {
            Ok(t) => {
                println!("[demo] using OpenAI-compatible translator");
                Box::new(t)
            }
            Err(e) => {
                eprintln!("[demo] MT env present but init failed ({e}); using EchoTranslator");
                Box::new(EchoTranslator)
            }
        }
    } else {
        println!("[demo] no MT configured; using offline EchoTranslator");
        Box::new(EchoTranslator)
    };
    let session = InterpretSession::new(SessionId(uuid::Uuid::now_v7()), cfg);
    let mut driver = InterpretDriver::new(session, translator, SineToneSynthesizer::default());

    // ---- capture ----
    let mut source = CoreAudioTapSource::new(ProcessFilter::IncludeApp(pid));
    if let Err(e) = source.start() {
        eprintln!("tap start failed: {e}");
        std::process::exit(1);
    }
    if let Err(e) = driver.begin_turn(direction) {
        eprintln!("begin_turn failed: {e}");
        std::process::exit(1);
    }
    println!("[demo] capturing pid {pid} for {secs}s, source lang '{source_lang}' ...");

    let mut resampler: Option<MonoResampler16k> = None;
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        while let Some(frame) = source.try_recv() {
            if resampler.is_none() {
                match MonoResampler16k::new(frame.sample_rate) {
                    Ok(r) => resampler = Some(r),
                    Err(e) => {
                        eprintln!("resampler init failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            if let Some(conv) = resampler.as_mut() {
                if let Ok(samples) = conv.push_frame(&frame) {
                    asr.push_samples(&samples);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = source.stop();

    // ---- recognize → translate → synthesize ----
    if let Err(e) = driver.end_utterance() {
        eprintln!("end_utterance failed: {e}");
        std::process::exit(1);
    }
    let text = asr.transcribe_all().unwrap_or_default();
    println!("[demo] recognized: {text:?}");

    let output = match driver.process_recognized(text) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("pipeline failed: {e}");
            std::process::exit(1);
        }
    };

    // ---- play synthesized translation (best-effort via BlackHole) ----
    if let Some(audio) = output.audio {
        let mut data = Vec::with_capacity(audio.samples.len() * 4);
        for s in &audio.samples {
            data.extend_from_slice(&s.to_le_bytes());
        }
        let frame = AudioFrame {
            sequence_no: 0,
            sample_rate: audio.sample_rate,
            channels: 1,
            format: AudioFormat::F32,
            timestamp_ms: 0,
            data,
        };
        let dur = Duration::from_secs_f32(audio.samples.len() as f32 / audio.sample_rate as f32);
        match BlackHoleSink::new() {
            Ok(mut sink) => match sink.start().and_then(|_| sink.write(&frame)) {
                Ok(()) => {
                    println!("[demo] playing {:.1}s of synthesized audio into BlackHole ...", dur.as_secs_f32());
                    std::thread::sleep(dur + Duration::from_millis(200));
                    let _ = sink.stop();
                }
                Err(e) => eprintln!("[demo] BlackHole playback skipped: {e}"),
            },
            Err(e) => eprintln!("[demo] BlackHole not available ({e}); skipping playback"),
        }
    } else {
        println!("[demo] nothing recognized; turn aborted");
    }

    // ---- finalize turn ----
    if let Err(e) = driver.finish_playback() {
        eprintln!("finish_playback failed: {e}");
    }

    println!("--------------------------------------------------");
    for turn in driver.session().history() {
        println!("[turn {:?} {:?}]", turn.id, turn.direction);
        println!("  source     : {}", turn.source_text);
        println!("  translation: {}", turn.translated_text);
    }
    println!("[demo] final state: {}", driver.session().state());
}

#[cfg(not(all(target_os = "macos", feature = "integration")))]
fn main() {
    eprintln!("this example requires macOS and `--features integration`.");
    std::process::exit(2);
}
