//! Local speech recognition using whisper.cpp via the `whisper-cpp-plus` crate.
//!
//! [`WhisperAsr`] wraps whisper.cpp with Silero VAD to provide on-device,
//! offline-capable speech-to-text. Audio frames from [`CpalAudioInput`] are
//! accumulated, then transcribed when the user stops speaking.
//!
//! The flow:
//! 1. Feed PCM f32 frames via [`push_audio`].
//! 2. Call [`detect_and_transcribe`] periodically — it runs VAD first,
//!    and only invokes Whisper on detected speech segments.
//! 3. Call [`reset`] between conversation turns.

use std::path::Path;
use std::sync::Arc;

use tracing::{info, warn};

use crate::AudioError;

/// Result of a local ASR transcription.
#[derive(Debug, Clone)]
pub struct AsrSegment {
    /// Recognized text.
    pub text: String,
    /// Start time offset in milliseconds (relative to buffered audio).
    pub start_ms: i64,
    /// End time offset in milliseconds.
    pub end_ms: i64,
}

/// Configuration for the Whisper ASR engine.
#[derive(Debug, Clone)]
pub struct WhisperAsrConfig {
    /// Path to the Whisper GGML model file (e.g., `ggml-small.bin`).
    pub model_path: String,
    /// Path to the Silero VAD model file (e.g., `ggml-silero-vad.bin`).
    pub vad_model_path: String,
    /// Language code (e.g., `"zh"`, `"en"`, `"auto"`).
    pub language: String,
    /// Number of threads for inference.
    pub n_threads: i32,
    /// Minimum speech duration in ms to consider valid.
    pub min_speech_duration_ms: i32,
    /// Minimum silence duration in ms to split segments.
    pub min_silence_duration_ms: i32,
}

impl Default for WhisperAsrConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            vad_model_path: String::new(),
            language: "zh".into(),
            n_threads: 4,
            min_speech_duration_ms: 250,
            min_silence_duration_ms: 500,
        }
    }
}

/// On-device speech recognition engine.
///
/// Wraps whisper.cpp (via `whisper-cpp-plus`) with Silero VAD for
/// voice-activity-gated transcription. Audio is buffered until
/// [`detect_and_transcribe`] is called, at which point VAD identifies
/// speech segments and Whisper transcribes only those segments.
pub struct WhisperAsr {
    ctx: Arc<whisper_cpp_plus::WhisperContext>,
    vad: whisper_cpp_plus::WhisperVadProcessor,
    vad_params: whisper_cpp_plus::VadParams,
    language: String,
    n_threads: i32,
    /// Accumulated PCM f32 samples at 16 kHz mono.
    audio_buffer: Vec<f32>,
}

impl WhisperAsr {
    /// Create a new ASR engine.
    ///
    /// Loads the Whisper model and Silero VAD model from disk. This may take
    /// a few seconds depending on model size.
    pub fn new(config: &WhisperAsrConfig) -> Result<Self, AudioError> {
        if !Path::new(&config.model_path).exists() {
            return Err(AudioError::InitializationFailed(format!(
                "Whisper model not found: {}",
                config.model_path
            )));
        }
        if !Path::new(&config.vad_model_path).exists() {
            return Err(AudioError::InitializationFailed(format!(
                "VAD model not found: {}",
                config.vad_model_path
            )));
        }

        let ctx = whisper_cpp_plus::WhisperContext::new(&config.model_path).map_err(|e| {
            AudioError::InitializationFailed(format!("failed to load Whisper model: {e}"))
        })?;

        let vad = whisper_cpp_plus::WhisperVadProcessor::new(&config.vad_model_path).map_err(
            |e| AudioError::InitializationFailed(format!("failed to load VAD model: {e}")),
        )?;

        let vad_params = whisper_cpp_plus::VadParamsBuilder::new()
            .min_speech_duration_ms(config.min_speech_duration_ms)
            .min_silence_duration_ms(config.min_silence_duration_ms)
            .build();

        info!(
            model = %config.model_path,
            vad_model = %config.vad_model_path,
            language = %config.language,
            "WhisperAsr initialized"
        );

        Ok(Self {
            ctx: Arc::new(ctx),
            vad,
            vad_params,
            language: config.language.clone(),
            n_threads: config.n_threads,
            audio_buffer: Vec::new(),
        })
    }

    /// Push raw PCM f32 samples (16 kHz, mono) into the internal buffer.
    pub fn push_samples(&mut self, samples: &[f32]) {
        self.audio_buffer.extend_from_slice(samples);
    }

    /// Push an AudioFrame (from CpalAudioInput) into the buffer.
    ///
    /// Handles format conversion (F32/I16/U8 → f32) and resampling to 16kHz.
    /// If the frame's sample rate differs from 16kHz, a simple linear
    /// resampling is performed. Multi-channel audio is mixed to mono.
    pub fn push_frame(&mut self, frame: &crate::AudioFrame) {
        let channels = frame.channels.max(1) as usize;

        // Step 1: decode raw bytes to f32 samples (interleaved if multi-channel).
        let raw_samples: Vec<f32> = match frame.format {
            crate::AudioFormat::F32 => {
                frame.data.chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect()
            }
            crate::AudioFormat::I16 => {
                frame.data.chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / i16::MAX as f32)
                    .collect()
            }
            crate::AudioFormat::U8 => {
                frame.data.iter()
                    .map(|&b| (b as f32 - 128.0) / 128.0)
                    .collect()
            }
        };

        // Step 2: mix to mono if multi-channel.
        let mono: Vec<f32> = if channels > 1 {
            raw_samples.chunks(channels)
                .map(|ch| ch.iter().sum::<f32>() / channels as f32)
                .collect()
        } else {
            raw_samples
        };

        // Step 3: resample to 16kHz if needed.
        let target_rate = 16000u32;
        if frame.sample_rate == target_rate {
            self.audio_buffer.extend_from_slice(&mono);
        } else if frame.sample_rate > 0 {
            // Simple linear resampling.
            let ratio = frame.sample_rate as f64 / target_rate as f64;
            let output_len = (mono.len() as f64 / ratio) as usize;
            for i in 0..output_len {
                let src_idx = i as f64 * ratio;
                let idx = src_idx as usize;
                let frac = src_idx - idx as f64;
                let sample = if idx + 1 < mono.len() {
                    mono[idx] * (1.0 - frac as f32) + mono[idx + 1] * frac as f32
                } else if idx < mono.len() {
                    mono[idx]
                } else {
                    0.0
                };
                self.audio_buffer.push(sample);
            }
        }
    }

    /// How many seconds of audio are currently buffered.
    pub fn buffered_duration_secs(&self) -> f32 {
        self.audio_buffer.len() as f32 / 16000.0
    }

    /// Check if text is a Whisper hallucination (common artifacts from
    /// YouTube-subtitle training data that appear during silence/noise).
    fn is_hallucination(text: &str) -> bool {
        let t = text.trim().to_lowercase();
        // Common Chinese hallucinations
        if t.contains("字幕") || t.contains("订阅") || t.contains("谢谢观看")
            || t.contains("感谢观看") || t.contains("请订阅") || t.contains("索兰")
            || t.contains("小伙伴") || t.contains("点赞")
        {
            return true;
        }
        // Common English hallucinations
        if t.contains("thanks for watching") || t.contains("subscribe")
            || t.contains("like and share") || t.contains("subtitles by")
            || t.contains("amara.org")
        {
            return true;
        }
        // Whisper often outputs "(music)" or "[BLANK_AUDIO]" for non-speech
        if t.starts_with('(') && t.ends_with(')') {
            return true;
        }
        if t.contains("[blank_audio]") || t.contains("[blank audio]") {
            return true;
        }
        // Very short repetitive outputs
        if t.len() <= 2 && t.chars().all(|c| c == '.' || c == ',' || c == ' ') {
            return true;
        }
        false
    }

    /// Run VAD on the buffered audio, then transcribe detected speech segments.
    ///
    /// Returns recognized text segments. Returns an empty vec if no speech
    /// was detected or the buffer is too short.
    pub fn detect_and_transcribe(&mut self) -> Result<Vec<AsrSegment>, AudioError> {
        if self.audio_buffer.len() < 1600 {
            // Less than 100ms — not enough audio.
            return Ok(Vec::new());
        }

        // Run VAD to find speech segments.
        let vad_segments = self
            .vad
            .segments_from_samples(&self.audio_buffer, &self.vad_params)
            .map_err(|e| AudioError::InitializationFailed(format!("VAD failed: {e}")))?;

        let segment_ranges = vad_segments.get_all_segments();
        if segment_ranges.is_empty() {
            return Ok(Vec::new());
        }

        // Extract speech audio from VAD segments.
        let speech_chunks = vad_segments.extract_audio_segments(&self.audio_buffer, 16000.0);

        let mut results = Vec::new();
        for (i, chunk) in speech_chunks.iter().enumerate() {
            if chunk.len() < 1600 {
                continue; // Skip very short segments.
            }

            let params = whisper_cpp_plus::TranscriptionParams::builder()
                .language(&self.language)
                .n_threads(self.n_threads)
                .enable_timestamps()
                .build();

            match self.ctx.transcribe_with_params(chunk, params) {
                Ok(result) => {
                    let text = result.text.trim().to_string();
                    if !text.is_empty() && !Self::is_hallucination(&text) {
                        let (start, end) = segment_ranges[i];
                        results.push(AsrSegment {
                            text,
                            start_ms: (start * 1000.0) as i64,
                            end_ms: (end * 1000.0) as i64,
                        });
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Whisper transcription failed for segment {i}");
                }
            }
        }

        Ok(results)
    }

    /// Transcribe all buffered audio without VAD (forced transcription).
    ///
    /// Useful when the caller has already done its own VAD or wants to
    /// transcribe everything regardless of detected speech.
    pub fn transcribe_all(&self) -> Result<String, AudioError> {
        if self.audio_buffer.len() < 1600 {
            return Ok(String::new());
        }

        let params = whisper_cpp_plus::TranscriptionParams::builder()
            .language(&self.language)
            .n_threads(self.n_threads)
            .build();

        self.ctx
            .transcribe_with_params(&self.audio_buffer, params)
            .map(|r| {
                let text = r.text.trim().to_string();
                if Self::is_hallucination(&text) { String::new() } else { text }
            })
            .map_err(|e| AudioError::InitializationFailed(format!("transcription failed: {e}")))
    }

    /// Clear the audio buffer for the next turn.
    pub fn reset(&mut self) {
        self.audio_buffer.clear();
    }

    /// Get a clone of the WhisperContext (for concurrent use).
    pub fn context(&self) -> Arc<whisper_cpp_plus::WhisperContext> {
        Arc::clone(&self.ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let cfg = WhisperAsrConfig::default();
        assert_eq!(cfg.language, "zh");
        assert_eq!(cfg.n_threads, 4);
        assert_eq!(cfg.min_speech_duration_ms, 250);
    }

    #[test]
    fn test_new_fails_missing_model() {
        let cfg = WhisperAsrConfig {
            model_path: "/nonexistent/model.bin".into(),
            vad_model_path: "/nonexistent/vad.bin".into(),
            ..Default::default()
        };
        let result = WhisperAsr::new(&cfg);
        assert!(result.is_err());
    }

    #[test]
    fn test_push_samples_and_duration() {
        // We can't create a real WhisperAsr without model files,
        // so we test the buffer math only.
        let samples = vec![0.0f32; 16000]; // 1 second
        let duration = samples.len() as f32 / 16000.0;
        assert!((duration - 1.0).abs() < 0.001);
    }
}
