//! End-to-end capture → 16 kHz → ASR probe.
//!
//! Wires the full "other party → text" path:
//!   CoreAudioTapSource → MonoResampler16k → WhisperAsr (English) → printed text
//!
//! Requires the `whisper` feature (on by default) and model files:
//!   export PRXVC_WHISPER_MODEL=/path/to/ggml-small.bin
//!   export PRXVC_VAD_MODEL=/path/to/ggml-silero-vad.bin
//!
//! Usage:
//!   cargo run -p prxvc-audio --example tap_to_text -- <PID> [SECONDS] [LANG]
//!     <PID>    : meeting app / browser tab pid currently playing English speech
//!     SECONDS  : capture duration (default 20)
//!     LANG     : ASR language code (default "en")

#[cfg(all(target_os = "macos", feature = "whisper"))]
fn main() {
    use std::time::{Duration, Instant};

    use prxvc_audio::core_audio_tap::CoreAudioTapSource;
    use prxvc_audio::resample::MonoResampler16k;
    use prxvc_audio::routing::{AppAudioSource, ProcessFilter};
    use prxvc_audio::whisper_asr::{WhisperAsr, WhisperAsrConfig};

    let mut args = std::env::args().skip(1);
    let pid: i32 = match args.next().and_then(|s| s.parse().ok()) {
        Some(p) => p,
        None => {
            eprintln!("usage: tap_to_text <PID> [SECONDS] [LANG]");
            std::process::exit(2);
        }
    };
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);
    let lang = args.next().unwrap_or_else(|| "en".to_owned());

    let model_path = std::env::var("PRXVC_WHISPER_MODEL").unwrap_or_default();
    let vad_model_path = std::env::var("PRXVC_VAD_MODEL").unwrap_or_default();
    if model_path.is_empty() || vad_model_path.is_empty() {
        eprintln!("set PRXVC_WHISPER_MODEL and PRXVC_VAD_MODEL to your ggml model paths");
        std::process::exit(2);
    }

    let cfg = WhisperAsrConfig {
        model_path,
        vad_model_path,
        language: lang.clone(),
        ..Default::default()
    };
    let mut asr = match WhisperAsr::new(&cfg) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[tap_to_text] ASR init failed: {e}");
            std::process::exit(1);
        }
    };

    let mut source = CoreAudioTapSource::new(ProcessFilter::IncludeApp(pid));
    if let Err(e) = source.start() {
        eprintln!("[tap_to_text] tap start failed: {e}");
        std::process::exit(1);
    }
    println!("[tap_to_text] capturing pid {pid} for {secs}s, ASR lang='{lang}' ...");

    let mut resampler: Option<MonoResampler16k> = None;
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut last_flush = Instant::now();
    let flush_every = Duration::from_millis(2000); // consecutive-style chunking

    while Instant::now() < deadline {
        // Drain all currently available captured frames.
        while let Some(frame) = source.try_recv() {
            // Build the resampler from the first frame's native rate.
            if resampler.is_none() {
                match MonoResampler16k::new(frame.sample_rate) {
                    Ok(r) => {
                        println!("[tap_to_text] source {}Hz/{}ch → 16k mono", frame.sample_rate, frame.channels);
                        resampler = Some(r);
                    }
                    Err(e) => {
                        eprintln!("[tap_to_text] resampler init failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            if let Some(conv) = resampler.as_mut() {
                match conv.push_frame(&frame) {
                    Ok(samples) => asr.push_samples(&samples),
                    Err(e) => eprintln!("[tap_to_text] convert error: {e}"),
                }
            }
        }

        // Periodically transcribe the buffered audio (one "turn").
        if last_flush.elapsed() >= flush_every && asr.buffered_duration_secs() > 0.5 {
            match asr.detect_and_transcribe() {
                Ok(segments) => {
                    for seg in segments {
                        println!("  [{:>6}ms] {}", seg.start_ms, seg.text);
                    }
                }
                Err(e) => eprintln!("[tap_to_text] transcribe error: {e}"),
            }
            asr.reset();
            last_flush = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = source.stop();
    println!("[tap_to_text] done.");
}

#[cfg(not(all(target_os = "macos", feature = "whisper")))]
fn main() {
    eprintln!("tap_to_text requires macOS and the `whisper` feature.");
    std::process::exit(2);
}
