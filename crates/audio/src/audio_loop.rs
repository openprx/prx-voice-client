//! Audio processing loop: microphone → VAD → Whisper ASR → text.
//!
//! The [`AudioProcessingLoop`] captures audio from the microphone, feeds it
//! to the local Whisper ASR engine, and sends recognized text through a channel.
//! It runs on a dedicated thread to avoid blocking the UI or engine threads.
//!
//! # Lifecycle
//!
//! ```text
//! start() → captures audio, runs VAD + Whisper in a loop
//! stop()  → stops capture, returns to idle
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// The capture + ASR machinery below is only compiled with the `whisper`
// feature; its imports are gated to keep `--no-default-features` warning-free.
#[cfg(feature = "whisper")]
use std::time::Duration;

#[cfg(feature = "whisper")]
use tracing::{info, warn};

#[cfg(feature = "whisper")]
use crate::AudioError;
#[cfg(feature = "whisper")]
use crate::cpal_input::CpalAudioInput;
#[cfg(feature = "whisper")]
use crate::pipeline::AudioInput;
#[cfg(feature = "whisper")]
use crate::whisper_asr::{WhisperAsr, WhisperAsrConfig};

/// A recognized utterance from the audio processing loop.
#[derive(Debug, Clone)]
pub struct RecognizedUtterance {
    /// The recognized text.
    pub text: String,
}

/// Controls and runs the audio capture + local ASR pipeline.
pub struct AudioProcessingLoop {
    /// Whether the loop is currently running.
    running: Arc<AtomicBool>,
    /// Handle to the background thread (for join on stop).
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl AudioProcessingLoop {
    /// Create a new (idle) audio processing loop.
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }

    /// Whether the loop is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Start the audio processing loop.
    ///
    /// Spawns a background thread that:
    /// 1. Opens the microphone via `CpalAudioInput`
    /// 2. Accumulates audio frames
    /// 3. Periodically checks for speech via Whisper's VAD
    /// 4. When speech ends, transcribes and sends text through `text_tx`
    ///
    /// The `session_id` is passed through for tagging but is not used by
    /// the audio loop itself.
    #[cfg(feature = "whisper")]
    pub fn start(
        &mut self,
        asr_config: WhisperAsrConfig,
        session_id: uuid::Uuid,
        text_tx: std::sync::mpsc::Sender<RecognizedUtterance>,
    ) -> Result<(), AudioError> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(()); // Already running.
        }

        self.running.store(true, Ordering::Relaxed);
        let running = Arc::clone(&self.running);

        let handle = std::thread::Builder::new()
            .name("prxvc-audio-loop".into())
            .spawn(move || {
                if let Err(e) = run_audio_loop(running.clone(), asr_config, session_id, text_tx) {
                    warn!(error = %e, "audio loop exited with error");
                }
                running.store(false, Ordering::Relaxed);
                info!("audio processing loop stopped");
            })
            .map_err(|e| AudioError::InitializationFailed(format!("failed to spawn audio thread: {e}")))?;

        self.thread_handle = Some(handle);
        info!("audio processing loop started");
        Ok(())
    }

    /// Stop the audio processing loop.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AudioProcessingLoop {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The actual audio loop running on a dedicated thread.
#[cfg(feature = "whisper")]
fn run_audio_loop(
    running: Arc<AtomicBool>,
    asr_config: WhisperAsrConfig,
    session_id: uuid::Uuid,
    text_tx: std::sync::mpsc::Sender<RecognizedUtterance>,
) -> Result<(), AudioError> {
    // Initialize Whisper ASR.
    let mut asr = WhisperAsr::new(&asr_config)?;

    // Initialize microphone input.
    let mut mic = CpalAudioInput::new(session_id)?;
    mic.start()?;
    info!("microphone started, listening for speech");

    // Strategy: accumulate audio continuously, check VAD every ~2 seconds.
    // When VAD finds speech segments, transcribe them immediately.
    // When no speech for a while after speech was detected, do a forced
    // transcription to catch anything VAD might have missed.
    let mut last_vad_check = std::time::Instant::now();
    let mut had_speech_ever = false;
    let mut silence_since_speech = std::time::Instant::now();
    let vad_check_interval = Duration::from_secs(2);
    let silence_timeout = Duration::from_secs(3);
    let max_buffer = Duration::from_secs(30);

    while running.load(Ordering::Relaxed) {
        // Read all available audio frames (non-blocking).
        let mut got_frame = false;
        while let Ok(Some(input_frame)) = mic.read_frame() {
            asr.push_frame(&input_frame.frame);
            got_frame = true;
        }

        if !got_frame {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }

        let buffered = asr.buffered_duration_secs();

        // Don't check VAD until we have at least 1 second of audio.
        if buffered < 1.0 {
            continue;
        }

        // Check VAD periodically (not every frame).
        if last_vad_check.elapsed() < vad_check_interval {
            // Prevent buffer from growing unbounded if we never detect speech.
            if buffered > max_buffer.as_secs_f32() {
                asr.reset();
            }
            continue;
        }
        last_vad_check = std::time::Instant::now();

        // Run VAD + Whisper on accumulated audio.
        match asr.detect_and_transcribe() {
            Ok(segments) if !segments.is_empty() => {
                had_speech_ever = true;
                silence_since_speech = std::time::Instant::now();

                for seg in &segments {
                    info!(text = %seg.text, "local ASR recognized speech");
                    let _ = text_tx.send(RecognizedUtterance {
                        text: seg.text.clone(),
                    });
                }

                // Reset buffer after successful transcription.
                asr.reset();
            }
            Ok(_) => {
                // No speech in this check.
                if had_speech_ever && silence_since_speech.elapsed() > silence_timeout {
                    // Silence timeout after speech — force transcribe remaining audio
                    // in case VAD missed trailing speech.
                    if buffered > 1.0 {
                        if let Ok(text) = asr.transcribe_all() {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() && trimmed != "[BLANK_AUDIO]" {
                                info!(text = %trimmed, "local ASR tail transcription");
                                let _ = text_tx.send(RecognizedUtterance {
                                    text: trimmed.to_string(),
                                });
                            }
                        }
                    }
                    asr.reset();
                    had_speech_ever = false;
                }

                // Trim buffer if it gets too long without any speech.
                if buffered > 10.0 && !had_speech_ever {
                    asr.reset();
                }
            }
            Err(e) => {
                warn!(error = %e, "VAD/transcription check failed");
                // Don't reset on error — retry next interval.
            }
        }
    }

    // Cleanup.
    mic.stop()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_loop_new_is_not_running() {
        let loop_ = AudioProcessingLoop::new();
        assert!(!loop_.is_running());
    }

    #[test]
    fn test_audio_loop_stop_when_idle_is_safe() {
        let mut loop_ = AudioProcessingLoop::new();
        loop_.stop(); // Should not panic.
        assert!(!loop_.is_running());
    }
}
