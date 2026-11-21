//! A microphone (or any [`AudioSource`]) delivered at the rate a consumer needs, restamped on the consumer's clock.
//!
//! The assistant's recognition stage takes 16 kHz frames (the speech detector's rate) while a
//! microphone delivers its device rate. This wrapper converts with the workspace resampler and
//! stamps each frame with the time it was captured on the injected clock, the same rule the call
//! pump applies to the audio it hands the translation pipeline: the frame ends now and is
//! never stamped before the end of the previous one.

use std::sync::Arc;
use std::time::Duration;

use prxvc_media::{AudioFrame, Clock, MonoTime, SampleRate};
use prxvc_ports::{AudioSource, EndpointError, EndpointHealth, SourceInfo};

use crate::resample::MonoResampler;

/// An [`AudioSource`] whose frames are at `target`, whatever the inner source delivers.
pub struct ResampledSource<S: AudioSource> {
    inner: S,
    target: SampleRate,
    clock: Arc<dyn Clock>,
    resampler: Option<(u32, MonoResampler)>,
    next_ts: MonoTime,
    seq: u64,
    scratch: Vec<AudioFrame>,
}

impl<S: AudioSource> ResampledSource<S> {
    /// Deliver `inner`'s frames at `target`, stamped on `clock`.
    pub fn new(inner: S, target: SampleRate, clock: Arc<dyn Clock>) -> Self {
        Self {
            inner,
            target,
            clock,
            resampler: None,
            next_ts: MonoTime::ZERO,
            seq: 0,
            scratch: Vec::new(),
        }
    }

    fn convert(&mut self, frame: &AudioFrame) -> Result<Vec<f32>, EndpointError> {
        if frame.rate() == self.target {
            return Ok(frame.samples().to_vec());
        }
        let rate = frame.rate().hz();
        if self.resampler.as_ref().is_none_or(|(r, _)| *r != rate) {
            let made = MonoResampler::new(rate, self.target.hz())
                .map_err(|e| EndpointError::Internal(format!("cannot resample {rate} Hz: {e}")))?;
            self.resampler = Some((rate, made));
        }
        let Some((_, resampler)) = self.resampler.as_mut() else {
            return Ok(Vec::new());
        };
        resampler
            .stream_write(frame.samples())
            .map_err(|e| EndpointError::Internal(format!("resampling failed: {e}")))
    }
}

impl<S: AudioSource> AudioSource for ResampledSource<S> {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        let info = self.inner.start()?;
        Ok(SourceInfo {
            rate: self.target,
            device: info.device,
        })
    }

    fn read(&mut self, out: &mut Vec<AudioFrame>, max_wait: Duration) -> Result<(), EndpointError> {
        let mut frames = std::mem::take(&mut self.scratch);
        frames.clear();
        let result = self.inner.read(&mut frames, max_wait);
        for frame in frames.drain(..) {
            let samples = self.convert(&frame)?;
            if samples.is_empty() {
                continue;
            }
            let duration = self.target.duration_of(samples.len());
            let now = self.clock.now();
            let back = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
            let ts = MonoTime::from_nanos(now.as_nanos().saturating_sub(back)).max(self.next_ts);
            self.next_ts = ts.saturating_add(duration);
            let restamped = AudioFrame::new(samples, self.target, ts, self.seq)
                .map_err(|e| EndpointError::Internal(format!("bad frame: {e}")))?;
            self.seq += 1;
            out.push(restamped.with_discontinuity(frame.discontinuity()));
        }
        self.scratch = frames;
        result
    }

    fn health(&self) -> EndpointHealth {
        self.inner.health()
    }

    fn stop(&mut self) {
        self.inner.stop();
    }
}

#[cfg(test)]
mod tests {
    use prxvc_media::ManualClock;
    use prxvc_ports::fakes::FakeAudioSource;

    use super::*;

    fn frames(rate: SampleRate, count: usize, len: usize) -> Vec<AudioFrame> {
        (0..count)
            .map(|i| {
                AudioFrame::new(vec![0.1; len], rate, MonoTime::ZERO, i as u64).expect("frame")
            })
            .collect()
    }

    #[test]
    fn a_48k_source_is_delivered_at_16k_and_the_device_is_kept() {
        let inner = FakeAudioSource::new(
            SampleRate::HZ_48000,
            frames(SampleRate::HZ_48000, 20, 1_440),
        );
        let clock = ManualClock::new();
        let mut src = ResampledSource::new(inner, SampleRate::HZ_16000, Arc::new(clock.clone()));
        let info = src.start().expect("starts");
        assert_eq!(info.rate, SampleRate::HZ_16000);
        assert_eq!(info.device, "fake-source");
        let mut total = 0;
        let mut out = Vec::new();
        for _ in 0..20 {
            clock.advance(Duration::from_millis(30));
            src.read(&mut out, Duration::ZERO).expect("read");
        }
        for f in &out {
            assert_eq!(f.rate(), SampleRate::HZ_16000);
            total += f.len();
        }
        let expected = 20 * 480;
        assert!(
            total.abs_diff(expected) <= 480,
            "about a third of the samples remain: {total} of {expected}"
        );
    }

    #[test]
    fn frames_are_restamped_in_order_on_the_clock_and_numbered() {
        let inner =
            FakeAudioSource::new(SampleRate::HZ_16000, frames(SampleRate::HZ_16000, 5, 512));
        let clock = ManualClock::new();
        let mut src = ResampledSource::new(inner, SampleRate::HZ_16000, Arc::new(clock.clone()));
        src.start().expect("starts");
        let mut out = Vec::new();
        for _ in 0..5 {
            clock.advance(Duration::from_millis(32));
            src.read(&mut out, Duration::ZERO).expect("read");
        }
        assert_eq!(out.len(), 5);
        for pair in out.windows(2) {
            assert!(
                pair[1].ts() >= pair[0].end_ts(),
                "a frame never starts before the previous one ends"
            );
            assert_eq!(pair[1].seq(), pair[0].seq() + 1);
        }
        assert_eq!(
            out[0].ts(),
            MonoTime::ZERO,
            "the first frame ends at the clock's first reading"
        );
        assert_eq!(
            out[0].len(),
            512,
            "a source already at the target rate is passed through"
        );
    }

    #[test]
    fn a_failing_inner_source_reports_its_error_and_stop_reaches_it() {
        let inner =
            FakeAudioSource::new(SampleRate::HZ_16000, frames(SampleRate::HZ_16000, 3, 512))
                .failing_read_at(
                    1,
                    EndpointError::DeviceLost {
                        device: "usb".into(),
                    },
                );
        let mut src =
            ResampledSource::new(inner, SampleRate::HZ_16000, Arc::new(ManualClock::new()));
        src.start().expect("starts");
        let mut out = Vec::new();
        assert!(src.read(&mut out, Duration::ZERO).is_ok());
        assert_eq!(
            src.read(&mut out, Duration::ZERO),
            Err(EndpointError::DeviceLost {
                device: "usb".into()
            })
        );
        src.stop();
        assert!(!src.health().running);
        assert_eq!(
            src.read(&mut out, Duration::ZERO),
            Err(EndpointError::NotStarted)
        );
    }
}
