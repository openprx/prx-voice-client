//! Mock capture endpoint that produces silent frames, for tests that need a running source without a device.

use std::time::Duration;

use prxvc_media::{AudioFrame, MonoTime, SampleRate};
use prxvc_ports::{AudioSource, EndpointError, EndpointHealth, SourceInfo};

/// Mock audio source that delivers one silent 20 ms frame per `read`.
pub struct MockAudioSource {
    rate: SampleRate,
    running: bool,
    sequence: u64,
}

impl MockAudioSource {
    /// A source of mono silence at `rate`.
    pub fn new(rate: SampleRate) -> Self {
        Self {
            rate,
            running: false,
            sequence: 0,
        }
    }
}

impl AudioSource for MockAudioSource {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        if self.running {
            return Err(EndpointError::Internal(
                "source is already started".to_owned(),
            ));
        }
        self.running = true;
        Ok(SourceInfo {
            rate: self.rate,
            device: "mock".to_owned(),
        })
    }

    fn read(
        &mut self,
        out: &mut Vec<AudioFrame>,
        _max_wait: Duration,
    ) -> Result<(), EndpointError> {
        if !self.running {
            return Err(EndpointError::NotStarted);
        }
        let samples = self.rate.samples_in(Duration::from_millis(20));
        let ts = MonoTime::from_millis(self.sequence * 20);
        let frame = AudioFrame::new(vec![0.0; samples], self.rate, ts, self.sequence)
            .map_err(|e| EndpointError::Internal(e.to_string()))?;
        self.sequence += 1;
        out.push(frame);
        Ok(())
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth {
            running: self.running,
            frames: self.sequence,
            ..EndpointHealth::default()
        }
    }

    fn stop(&mut self) {
        self.running = false;
        self.sequence = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_source_generates_frames_once_started() {
        let mut source = MockAudioSource::new(SampleRate::HZ_16000);
        let mut out = Vec::new();
        assert_eq!(
            source.read(&mut out, Duration::ZERO),
            Err(EndpointError::NotStarted)
        );
        source.start().unwrap();
        source.read(&mut out, Duration::ZERO).unwrap();
        let f = &out[0];
        assert_eq!((f.seq(), f.rate(), f.len()), (0, SampleRate::HZ_16000, 320));
        assert!(f.samples().iter().all(|&s| s == 0.0));
    }

    #[test]
    fn mock_source_restarts_from_sequence_zero() {
        let mut source = MockAudioSource::new(SampleRate::HZ_16000);
        source.start().unwrap();
        let mut out = Vec::new();
        source.read(&mut out, Duration::ZERO).unwrap();
        source.stop();
        assert!(!source.health().running);
        source.start().unwrap();
        out.clear();
        source.read(&mut out, Duration::ZERO).unwrap();
        assert_eq!(out[0].seq(), 0);
    }
}
