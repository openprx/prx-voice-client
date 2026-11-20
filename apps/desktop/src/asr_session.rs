//! ASR session start-up: locates the Whisper model files and starts the audio capture and recognition loop.

use prxvc_audio::audio_loop::{AudioProcessingLoop, LoopConfig, LoopLinks, RecognizedUtterance};
use prxvc_audio::input_supervisor::AudioLoopEvent;
use prxvc_audio::echo_gate::EchoGateHandle;
use prxvc_desktop_ui::SharedAppState;

use crate::config_load::app_paths;
use crate::event_map::push_error;

/// Channels and settings the audio loop is started with.
pub(crate) struct AudioWiring {
    /// Recognized utterances go to the engine thread through this sender.
    pub text_tx: std::sync::mpsc::Sender<RecognizedUtterance>,
    /// Microphone changes go to the engine thread through this sender.
    pub events_tx: std::sync::mpsc::Sender<AudioLoopEvent>,
    /// Configured input device (UID or name); empty follows the policy defaults.
    pub input_device: String,
}

/// Start the local audio capture + Whisper ASR loop for the active session.
pub(crate) fn start_audio_loop(
    engine: &prxvc_core::engine::Engine,
    audio_loop: &mut AudioProcessingLoop,
    wiring: &AudioWiring,
    shared: &SharedAppState,
    gate: EchoGateHandle,
    loop_config: LoopConfig,
) {
    {
        let Some(session) = engine.active_session() else {
            return;
        };
        let session_id = session.id.0;

        // Model paths — look in standard locations.
        let models_dir = app_paths()
            .map(|p| p.models_dir.clone())
            .unwrap_or_else(|| std::path::PathBuf::from("models"));

        let model_path = models_dir.join("ggml-small.bin");
        let vad_model_path = models_dir.join("ggml-silero-vad.bin");

        if !model_path.exists() || !vad_model_path.exists() {
            tracing::warn!(
                model = %model_path.display(),
                vad = %vad_model_path.display(),
                "Whisper model files not found — audio ASR disabled. \
                 Download models to the paths above."
            );
            return;
        }

        let asr_config = prxvc_audio::whisper_asr::WhisperAsrConfig {
            model_path: model_path.to_string_lossy().into(),
            vad_model_path: vad_model_path.to_string_lossy().into(),
            language: "zh".into(),
            n_threads: 4,
            ..Default::default()
        };

        let links = LoopLinks {
            text_tx: wiring.text_tx.clone(),
            events_tx: wiring.events_tx.clone(),
            gate,
        };
        if let Err(e) = audio_loop.start(asr_config, session_id, links, loop_config, wiring.input_device.clone()) {
            tracing::warn!(error = %e, "failed to start audio loop");
            push_error(shared, &format!("Audio: {e}"));
        }
    }

}
