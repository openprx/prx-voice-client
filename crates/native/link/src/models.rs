//! Locations of the benchmark models and reference recordings used by the ignored tests.
//!
//! Files are used in place and never copied into the repository. The root defaults to the
//! workspace's `bench/` directory and can be overridden with `PRXVC_BENCH_ROOT`.

use std::path::PathBuf;

/// Root of the benchmark directory.
pub fn bench_root() -> PathBuf {
    std::env::var_os("PRXVC_BENCH_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/nn/worker/voice/bench"))
}

/// Qwen3-TTS 1.7B talker, Q8_0.
pub fn talker_1_7b() -> PathBuf {
    bench_root().join("tts/models/qwen-talker-1.7b-base-Q8_0.gguf")
}

/// Qwen3-TTS 0.6B talker, Q8_0.
pub fn talker_0_6b() -> PathBuf {
    bench_root().join("tts/models/qwen-talker-0.6b-base-Q8_0.gguf")
}

/// Codec (12 Hz tokenizer), Q8_0.
pub fn codec() -> PathBuf {
    bench_root().join("tts/models/qwen-tokenizer-12hz-Q8_0.gguf")
}

/// The owner's English reference recording (24 kHz mono PCM16).
pub fn en_owner_wav() -> PathBuf {
    bench_root().join("tts/refs/en_owner.wav")
}

/// The exact text of the English reference recording.
pub fn en_owner_text() -> String {
    std::fs::read_to_string(bench_root().join("tts/refs/en_owner.txt"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Qwen3-ASR 0.6B (offline, multilingual), Q8_0.
pub fn asr_qwen3_0_6b() -> PathBuf {
    bench_root().join("asr/models/hf/Qwen3-ASR-0.6B-gguf/Qwen3-ASR-0.6B-Q8_0.gguf")
}

/// Nemotron streaming English ASR, Q8_0.
pub fn asr_nemotron_en() -> PathBuf {
    bench_root().join("asr/models/hf/nemotron-speech-streaming-en-0.6b-gguf/nemotron-speech-streaming-en-0.6b-Q8_0.gguf")
}
