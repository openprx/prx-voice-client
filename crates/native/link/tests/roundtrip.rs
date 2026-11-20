//! ASR round trip for audio synthesized by `QwenTts` (CT-10): transcribes the WAV files that the
//! adapter's live test wrote and compares the text with what was synthesized. Ignored: it needs the
//! ASR model and the GPU.
//!
//! ```text
//! cargo test -p prxvc-adapters --features qwentts --release --test qwentts_live \
//!     -- --ignored --nocapture --test-threads=1          # prints the output directory
//! PRXVC_ROUNDTRIP_DIR=<that directory> \
//! cargo test -p prxvc-native-link --features transcribe --release --test roundtrip \
//!     -- --ignored --nocapture
//! ```
//!
//! `PRXVC_BENCH_ROOT` overrides the benchmark directory (ASR model). Nothing is played.
#![cfg(feature = "transcribe")]

use prxvc_native_link::asr::{AsrModel, AsrSession};
use prxvc_native_link::{models, resample, text, wav};
use std::path::PathBuf;

const EN: &str = "The meeting has been moved to Thursday afternoon, so please update your calendar before the call.";
const ZH: &str = "会议改到了星期四下午，请在通话之前更新你的日历。";

/// Character error rate: the edit distance over characters, punctuation ignored.
fn cer(reference: &str, hyp: &str) -> f64 {
    let spaced = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    };
    text::wer(&spaced(reference), &spaced(hyp))
}

#[test]
#[ignore = "needs the ASR model, the GPU and the WAV files of the adapter's live test"]
fn synthesized_audio_round_trips_through_the_recognizer() {
    let dir = PathBuf::from(
        std::env::var_os("PRXVC_ROUNDTRIP_DIR")
            .expect("set PRXVC_ROUNDTRIP_DIR to the live test's output directory"),
    );
    let model = AsrModel::load(&models::asr_qwen3_0_6b()).expect("load the ASR model");
    let mut session = AsrSession::new(&model).expect("session");
    for (file, lang, reference, limit) in [
        ("en_owner.wav", "en", EN, 0.15),
        ("en_preset_remote.wav", "en", EN, 0.15),
        ("zh_owner.wav", "zh", ZH, 0.15),
    ] {
        let w = wav::read(&dir.join(file)).expect("read the synthesized wav");
        let pcm = resample::resample(&w.samples, w.sample_rate, 16_000);
        let hyp = session.run(&pcm, Some(lang)).expect("recognise");
        let error = if lang == "zh" {
            cer(reference, &hyp)
        } else {
            text::wer(reference, &hyp)
        };
        println!(
            "{file}: error rate {error:.3} ({:.2} s of audio): {hyp}",
            pcm.len() as f64 / 16_000.0
        );
        assert!(
            error <= limit,
            "{file}: recognised text too far from the synthesized text: {hyp}"
        );
    }
}
