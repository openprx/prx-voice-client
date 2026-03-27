use std::collections::VecDeque;

use crate::{AudioError, AudioFrame};

/// Fixed-capacity ring buffer for audio frames.
pub struct AudioBuffer {
    frames: VecDeque<AudioFrame>,
    capacity: usize,
}

impl AudioBuffer {
    /// Create a new audio buffer with the given maximum capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push a frame into the buffer, returning `BufferOverflow` if full.
    pub fn push(&mut self, frame: AudioFrame) -> Result<(), AudioError> {
        if self.frames.len() >= self.capacity {
            return Err(AudioError::BufferOverflow);
        }
        self.frames.push_back(frame);
        Ok(())
    }

    /// Pop the oldest frame from the buffer, or `None` if empty.
    pub fn pop(&mut self) -> Option<AudioFrame> {
        self.frames.pop_front()
    }

    /// Number of frames currently in the buffer.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Whether the buffer is at capacity.
    pub fn is_full(&self) -> bool {
        self.frames.len() >= self.capacity
    }

    /// Remove all frames from the buffer.
    pub fn clear(&mut self) {
        self.frames.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AudioFormat;

    fn make_frame(seq: u64) -> AudioFrame {
        AudioFrame {
            sequence_no: seq,
            sample_rate: 16_000,
            channels: 1,
            format: AudioFormat::I16,
            timestamp_ms: seq * 20,
            data: vec![0u8; 320],
        }
    }

    #[test]
    fn test_buffer_push_pop() {
        let mut buf = AudioBuffer::new(4);
        buf.push(make_frame(0)).unwrap();
        buf.push(make_frame(1)).unwrap();
        assert_eq!(buf.len(), 2);

        let f = buf.pop().unwrap();
        assert_eq!(f.sequence_no, 0);
        let f = buf.pop().unwrap();
        assert_eq!(f.sequence_no, 1);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_buffer_overflow() {
        let mut buf = AudioBuffer::new(2);
        buf.push(make_frame(0)).unwrap();
        buf.push(make_frame(1)).unwrap();
        assert!(buf.is_full());

        let result = buf.push(make_frame(2));
        assert!(result.is_err());
    }

    #[test]
    fn test_buffer_clear() {
        let mut buf = AudioBuffer::new(4);
        buf.push(make_frame(0)).unwrap();
        buf.push(make_frame(1)).unwrap();
        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_buffer_empty_pop_returns_none() {
        let mut buf = AudioBuffer::new(4);
        assert!(buf.pop().is_none());
    }
}
