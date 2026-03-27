use crate::{AudioError, AudioFrame, AudioInputFrame, AudioOutputFrame, AudioState, VadDecision};

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Audio input source trait for capturing audio from a device or stream.
pub trait AudioInput: Send {
    /// Start capturing audio.
    fn start(&mut self) -> Result<(), AudioError>;
    /// Stop capturing audio.
    fn stop(&mut self) -> Result<(), AudioError>;
    /// Read the next input frame (blocks until available or returns `None` on timeout).
    fn read_frame(&mut self) -> Result<Option<AudioInputFrame>, AudioError>;
    /// Current state of the input.
    fn state(&self) -> AudioState;
}

/// Audio output sink trait for playing audio to a device or stream.
pub trait AudioOutput: Send {
    /// Start playback.
    fn start(&mut self) -> Result<(), AudioError>;
    /// Stop playback.
    fn stop(&mut self) -> Result<(), AudioError>;
    /// Write an output frame for playback.
    fn write_frame(&mut self, frame: &AudioOutputFrame) -> Result<(), AudioError>;
    /// Current state of the output.
    fn state(&self) -> AudioState;
}

/// Voice-activity detection processor trait.
pub trait VadProcessor: Send {
    /// Process an audio frame and return a VAD decision.
    fn process(&mut self, frame: &AudioFrame) -> Result<VadDecision, AudioError>;
    /// Reset internal VAD state.
    fn reset(&mut self);
}

// ---------------------------------------------------------------------------
// AudioPipeline
// ---------------------------------------------------------------------------

/// Complete audio pipeline orchestrating input, output, and VAD processing.
pub struct AudioPipeline {
    input_state: AudioState,
    output_state: AudioState,
    frames_processed: u64,
    vad_enabled: bool,
}

impl AudioPipeline {
    /// Create a new audio pipeline with optional VAD processing.
    pub fn new(vad_enabled: bool) -> Self {
        Self {
            input_state: AudioState::InputClosed,
            output_state: AudioState::OutputClosed,
            frames_processed: 0,
            vad_enabled,
        }
    }

    /// Current input state.
    pub fn input_state(&self) -> &AudioState {
        &self.input_state
    }

    /// Current output state.
    pub fn output_state(&self) -> &AudioState {
        &self.output_state
    }

    /// Total number of input frames processed.
    pub fn frames_processed(&self) -> u64 {
        self.frames_processed
    }

    /// Whether VAD processing is enabled.
    pub fn vad_enabled(&self) -> bool {
        self.vad_enabled
    }

    /// Start the input pipeline (InputClosed -> InputStreaming).
    pub fn start_input(&mut self) -> Result<(), AudioError> {
        if self.input_state != AudioState::InputClosed {
            return Err(AudioError::InvalidStateTransition {
                from: self.input_state.to_string(),
                to: AudioState::InputStreaming.to_string(),
            });
        }
        self.input_state = AudioState::InputStreaming;
        Ok(())
    }

    /// Stop the input pipeline (any input state -> InputClosed).
    pub fn stop_input(&mut self) -> Result<(), AudioError> {
        self.input_state = AudioState::InputClosed;
        Ok(())
    }

    /// Start the output pipeline (OutputClosed -> OutputPlaying).
    pub fn start_output(&mut self) -> Result<(), AudioError> {
        if self.output_state != AudioState::OutputClosed {
            return Err(AudioError::InvalidStateTransition {
                from: self.output_state.to_string(),
                to: AudioState::OutputPlaying.to_string(),
            });
        }
        self.output_state = AudioState::OutputPlaying;
        Ok(())
    }

    /// Stop the output pipeline (any output state -> OutputClosed).
    pub fn stop_output(&mut self) -> Result<(), AudioError> {
        self.output_state = AudioState::OutputClosed;
        Ok(())
    }

    /// Pause the output pipeline (OutputPlaying -> OutputPaused).
    pub fn pause_output(&mut self) -> Result<(), AudioError> {
        if self.output_state != AudioState::OutputPlaying {
            return Err(AudioError::InvalidStateTransition {
                from: self.output_state.to_string(),
                to: AudioState::OutputPaused.to_string(),
            });
        }
        self.output_state = AudioState::OutputPaused;
        Ok(())
    }

    /// Resume the output pipeline (OutputPaused -> OutputPlaying).
    pub fn resume_output(&mut self) -> Result<(), AudioError> {
        if self.output_state != AudioState::OutputPaused {
            return Err(AudioError::InvalidStateTransition {
                from: self.output_state.to_string(),
                to: AudioState::OutputPlaying.to_string(),
            });
        }
        self.output_state = AudioState::OutputPlaying;
        Ok(())
    }

    /// Process a single input frame, incrementing the frame counter.
    pub fn process_input_frame(&mut self, _frame: &AudioFrame) -> Result<(), AudioError> {
        if self.input_state != AudioState::InputStreaming {
            return Err(AudioError::InvalidStateTransition {
                from: self.input_state.to_string(),
                to: "processing".to_string(),
            });
        }
        self.frames_processed += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AudioFormat;

    fn make_frame() -> AudioFrame {
        AudioFrame {
            sequence_no: 0,
            sample_rate: 16_000,
            channels: 1,
            format: AudioFormat::I16,
            timestamp_ms: 0,
            data: vec![0u8; 320],
        }
    }

    #[test]
    fn test_pipeline_initial_state() {
        let p = AudioPipeline::new(true);
        assert_eq!(*p.input_state(), AudioState::InputClosed);
        assert_eq!(*p.output_state(), AudioState::OutputClosed);
        assert_eq!(p.frames_processed(), 0);
        assert!(p.vad_enabled());
    }

    #[test]
    fn test_pipeline_start_stop_input() {
        let mut p = AudioPipeline::new(false);
        p.start_input().unwrap();
        assert_eq!(*p.input_state(), AudioState::InputStreaming);
        p.stop_input().unwrap();
        assert_eq!(*p.input_state(), AudioState::InputClosed);
    }

    #[test]
    fn test_pipeline_start_stop_output() {
        let mut p = AudioPipeline::new(false);
        p.start_output().unwrap();
        assert_eq!(*p.output_state(), AudioState::OutputPlaying);
        p.stop_output().unwrap();
        assert_eq!(*p.output_state(), AudioState::OutputClosed);
    }

    #[test]
    fn test_pipeline_pause_resume_output() {
        let mut p = AudioPipeline::new(false);
        p.start_output().unwrap();
        p.pause_output().unwrap();
        assert_eq!(*p.output_state(), AudioState::OutputPaused);
        p.resume_output().unwrap();
        assert_eq!(*p.output_state(), AudioState::OutputPlaying);
    }

    #[test]
    fn test_pipeline_process_frame_increments_counter() {
        let mut p = AudioPipeline::new(false);
        p.start_input().unwrap();
        let frame = make_frame();
        p.process_input_frame(&frame).unwrap();
        p.process_input_frame(&frame).unwrap();
        p.process_input_frame(&frame).unwrap();
        assert_eq!(p.frames_processed(), 3);
    }

    #[test]
    fn test_pipeline_cannot_start_already_started() {
        let mut p = AudioPipeline::new(false);
        p.start_input().unwrap();
        let result = p.start_input();
        assert!(result.is_err());
    }
}
