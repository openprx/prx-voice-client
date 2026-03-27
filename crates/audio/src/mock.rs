use uuid::Uuid;

use crate::pipeline::{AudioInput, AudioOutput};
use crate::{AudioError, AudioFormat, AudioFrame, AudioInputFrame, AudioOutputFrame, AudioState};

// ---------------------------------------------------------------------------
// MockAudioInput
// ---------------------------------------------------------------------------

/// Mock audio input that generates silent frames for testing.
pub struct MockAudioInput {
    state: AudioState,
    session_id: Uuid,
    sample_rate: u32,
    channels: u16,
    sequence: u64,
}

impl MockAudioInput {
    /// Create a new mock audio input source.
    pub fn new(session_id: Uuid, sample_rate: u32, channels: u16) -> Self {
        Self {
            state: AudioState::InputClosed,
            session_id,
            sample_rate,
            channels,
            sequence: 0,
        }
    }
}

impl AudioInput for MockAudioInput {
    fn start(&mut self) -> Result<(), AudioError> {
        if self.state != AudioState::InputClosed {
            return Err(AudioError::InvalidStateTransition {
                from: self.state.to_string(),
                to: AudioState::InputStreaming.to_string(),
            });
        }
        self.state = AudioState::InputStreaming;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.state = AudioState::InputClosed;
        self.sequence = 0;
        Ok(())
    }

    fn read_frame(&mut self) -> Result<Option<AudioInputFrame>, AudioError> {
        if self.state != AudioState::InputStreaming {
            return Ok(None);
        }

        let bytes_per_sample = 2; // I16
        let frame_samples = self.sample_rate as usize / 50; // 20ms frame
        let data_len = frame_samples * self.channels as usize * bytes_per_sample;

        let frame = AudioInputFrame {
            session_id: self.session_id,
            frame: AudioFrame {
                sequence_no: self.sequence,
                sample_rate: self.sample_rate,
                channels: self.channels,
                format: AudioFormat::I16,
                timestamp_ms: self.sequence * 20,
                data: vec![0u8; data_len],
            },
        };
        self.sequence += 1;
        Ok(Some(frame))
    }

    fn state(&self) -> AudioState {
        self.state
    }
}

// ---------------------------------------------------------------------------
// MockAudioOutput
// ---------------------------------------------------------------------------

/// Mock audio output that discards frames for testing.
pub struct MockAudioOutput {
    state: AudioState,
    frames_written: u64,
}

impl MockAudioOutput {
    /// Create a new mock audio output sink.
    pub fn new() -> Self {
        Self {
            state: AudioState::OutputClosed,
            frames_written: 0,
        }
    }

    /// Number of frames successfully written.
    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }
}

impl Default for MockAudioOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioOutput for MockAudioOutput {
    fn start(&mut self) -> Result<(), AudioError> {
        if self.state != AudioState::OutputClosed {
            return Err(AudioError::InvalidStateTransition {
                from: self.state.to_string(),
                to: AudioState::OutputPlaying.to_string(),
            });
        }
        self.state = AudioState::OutputPlaying;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.state = AudioState::OutputClosed;
        Ok(())
    }

    fn write_frame(&mut self, _frame: &AudioOutputFrame) -> Result<(), AudioError> {
        if self.state != AudioState::OutputPlaying {
            return Err(AudioError::InvalidStateTransition {
                from: self.state.to_string(),
                to: "writing".to_string(),
            });
        }
        self.frames_written += 1;
        Ok(())
    }

    fn state(&self) -> AudioState {
        self.state
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output_frame() -> AudioOutputFrame {
        AudioOutputFrame {
            session_id: Uuid::nil(),
            frame: AudioFrame {
                sequence_no: 0,
                sample_rate: 16_000,
                channels: 1,
                format: AudioFormat::I16,
                timestamp_ms: 0,
                data: vec![0u8; 320],
            },
        }
    }

    #[test]
    fn test_mock_input_generates_frames() {
        let mut input = MockAudioInput::new(Uuid::nil(), 16_000, 1);
        input.start().unwrap();
        let frame = input.read_frame().unwrap();
        assert!(frame.is_some());
        let f = frame.unwrap();
        assert_eq!(f.frame.sequence_no, 0);
        assert_eq!(f.frame.sample_rate, 16_000);
        assert!(f.frame.data.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_mock_input_returns_none_when_stopped() {
        let mut input = MockAudioInput::new(Uuid::nil(), 16_000, 1);
        // Not started — should return None
        let frame = input.read_frame().unwrap();
        assert!(frame.is_none());
    }

    #[test]
    fn test_mock_output_counts_frames() {
        let mut output = MockAudioOutput::new();
        output.start().unwrap();
        let frame = make_output_frame();
        output.write_frame(&frame).unwrap();
        output.write_frame(&frame).unwrap();
        assert_eq!(output.frames_written(), 2);
    }

    #[test]
    fn test_mock_output_rejects_when_stopped() {
        let mut output = MockAudioOutput::new();
        let frame = make_output_frame();
        let result = output.write_frame(&frame);
        assert!(result.is_err());
    }
}
