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
use crate::echo_gate::EchoGateHandle;
#[cfg(feature = "whisper")]
use crate::echo_gate::{Routed, route_frame};
#[cfg(feature = "whisper")]
use crate::endpoint::{Endpoint, decide, speech_duration};
#[cfg(feature = "whisper")]
use crate::turn_policy::join_text;
#[cfg(feature = "whisper")]
use crate::input_supervisor::{InputSupervisor, PlatformInputBackend};
#[cfg(feature = "whisper")]
use crate::whisper_asr::{WhisperAsr, WhisperAsrConfig};

/// How often the VAD is consulted. The VAD is incremental, so a pass costs
/// only the audio that arrived since the previous one; a short interval keeps
/// the delay between the end of silence and the transcription small.
#[cfg(feature = "whisper")]
const VAD_CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// Least buffered audio worth running the VAD on.
#[cfg(feature = "whisper")]
const MIN_CHECK_AUDIO_SECS: f32 = 0.5;

/// A recognized utterance from the audio processing loop.
#[derive(Debug, Clone)]
pub struct RecognizedUtterance {
    /// The recognized text.
    pub text: String,
    /// Speech in the utterance according to the VAD segments (silence
    /// between and around them excluded).
    pub speech: std::time::Duration,
}

/// Channels the audio loop talks to the application through.
pub struct LoopLinks {
    /// Recognized utterances.
    pub text_tx: std::sync::mpsc::Sender<RecognizedUtterance>,
    /// Microphone changes (device chosen, switched, lost).
    pub events_tx: std::sync::mpsc::Sender<crate::input_supervisor::AudioLoopEvent>,
    /// Closed while the assistant speaks; microphone audio is dropped then.
    pub gate: EchoGateHandle,
}

/// How the audio loop ends utterances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopConfig {
    /// End-of-utterance decision parameters.
    pub endpoint: crate::endpoint::EndpointConfig,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            endpoint: crate::endpoint::EndpointConfig::from_end_silence_ms(800),
        }
    }
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
    /// 1. Opens the microphone chosen by the input-device policy (never blindly
    ///    the system default; `input_device` is the configured UID or name)
    /// 2. Accumulates audio frames
    /// 3. Periodically checks for speech via Whisper's VAD
    /// 4. When speech ends, transcribes and sends text through `text_tx`
    ///
    /// The `session_id` is passed through for tagging but is not used by
    /// the audio loop itself. Microphone audio is discarded while `gate` is
    /// closed (the assistant is speaking) and the recogniser starts afresh
    /// when it reopens.
    #[cfg(feature = "whisper")]
    pub fn start(
        &mut self,
        asr_config: WhisperAsrConfig,
        session_id: uuid::Uuid,
        links: LoopLinks,
        loop_config: LoopConfig,
        input_device: String,
    ) -> Result<(), AudioError> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(()); // Already running.
        }

        self.running.store(true, Ordering::Relaxed);
        let running = Arc::clone(&self.running);

        let handle = std::thread::Builder::new()
            .name("prxvc-audio-loop".into())
            .spawn(move || {
                if let Err(e) = run_audio_loop(running.clone(), asr_config, session_id, links, loop_config, &input_device) {
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
    links: LoopLinks,
    loop_config: LoopConfig,
    input_device: &str,
) -> Result<(), AudioError> {
    let LoopLinks { text_tx, events_tx, gate } = links;
    // Initialize Whisper ASR.
    let mut asr = WhisperAsr::new(&asr_config)?;

    // Initialize microphone input on the device the policy chooses.
    let mut mic = InputSupervisor::new(PlatformInputBackend, session_id, input_device, events_tx);
    mic.start(std::time::Instant::now())?;
    info!("microphone started, listening for speech");

    // Strategy: accumulate audio, run the (incremental) VAD every
    // `VAD_CHECK_INTERVAL`, and finalise the utterance only when the VAD shows
    // `end_silence` of silence after the last speech (or the utterance hits
    // its maximum length). Nothing is transcribed while the speaker may still
    // continue, so a pause inside a sentence does not split it.
    let mut last_vad_check = std::time::Instant::now();
        let max_idle_buffer = Duration::from_secs(10);

    while running.load(Ordering::Relaxed) {
        // Follow device changes; audio buffered from the old device is stale.
        if mic.tick(std::time::Instant::now()) {
            asr.reset();
            last_vad_check = std::time::Instant::now();
        }

        // Read all available audio frames (non-blocking).
        let mut got_frame = false;
        while let Some(frame) = mic.read_frame() {
            // Frames captured while the assistant speaks (and shortly after)
            // are dropped; on reopening the recogniser starts afresh (the
            // gate resets it) and so do the timers.
            match route_frame(&gate, std::time::Instant::now(), &frame, &mut asr) {
                Routed::Dropped => continue,
                Routed::DeliveredAfterReset => {
                    last_vad_check = std::time::Instant::now();
                }
                Routed::Delivered => {}
            }
            got_frame = true;
        }

        if !got_frame {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }

        // Don't check VAD until there is some audio to look at.
        if asr.buffered_duration_secs() < MIN_CHECK_AUDIO_SECS
            || last_vad_check.elapsed() < VAD_CHECK_INTERVAL
        {
            continue;
        }
        last_vad_check = std::time::Instant::now();

        let segments = match asr.speech_segments() {
            Ok(segments) => segments,
            Err(e) => {
                warn!(error = %e, "VAD check failed");
                // Don't reset on error — retry next interval.
                continue;
            }
        };
        match decide(&loop_config.endpoint, asr.buffered_samples(), &segments) {
            Endpoint::NoSpeech => {
                // Don't let silence accumulate without bound.
                if asr.buffered_duration_secs() > max_idle_buffer.as_secs_f32() {
                    asr.reset();
                }
            }
            Endpoint::Continue => {}
            Endpoint::Finalize(reason) => {
                let speech = speech_duration(&segments);
                let pieces = asr.transcribe_segments(&segments);
                let text = pieces
                    .iter()
                    .fold(String::new(), |acc, seg| join_text(&acc, &seg.text));
                info!(
                    ?reason,
                    speech_ms = speech.as_millis() as u64,
                    text_chars = text.chars().count(),
                    "local ASR utterance finalised"
                );
                tracing::debug!(text = %text, "local ASR utterance text");
                if !text.is_empty() {
                    let _ = text_tx.send(RecognizedUtterance { text, speech });
                }
                asr.reset();
            }
        }
    }

    // Cleanup.
    mic.stop();
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
