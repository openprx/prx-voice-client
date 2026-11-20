//! Hardware-independent half of the audio output sinks: frame conversion to the device format and the queue the device callback drains.
//!
//! [`PlaybackSink`](crate::playback_sink::PlaybackSink) does the same work for
//! every device between "a frame arrives" and "the device callback asks for
//! samples":
//!
//! 1. [`FrameConverter`] resamples the mono frame to the **device rate** with
//!    the shared [`MonoResampler`] and fans the mono signal out to the **device
//!    channel count**.
//! 2. [`PlaybackBuffer`] holds the converted interleaved samples in a bounded
//!    lock-free ring shared with the device callback. It counts underruns and
//!    overruns, can be flushed, and signals when it has drained.
//!
//! [`OutputCore`] bundles the two. None of this touches audio hardware, so it
//! is unit-tested directly.

use std::sync::Arc;

use prxvc_media::AudioFrame;

use crate::playback_buffer::PlaybackBuffer;
use crate::resample::MonoResampler;
use crate::AudioError;

/// Default capacity of a playback buffer, in seconds of audio.
///
/// Assistant speech arrives faster than real time (the server sends a whole
/// sentence at once), so the bound must cover a long reply; it only exists to
/// keep a stalled device from growing memory without limit.
pub const DEFAULT_BUFFER_SECONDS: u32 = 30;

/// Counters describing how a playback buffer has behaved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutputStats {
    /// Times the device ran out of samples in the middle of a stream.
    /// Reaching the end of a stream announced with
    /// [`PlaybackBuffer::end_stream`] is not an underrun.
    pub underruns: u64,
    /// Times a write did not fit and had to be truncated.
    pub overruns: u64,
    /// Total samples (all channels) discarded by overruns.
    pub dropped_samples: u64,
}

/// Converts mono frames of any rate to interleaved f32 at a fixed device rate
/// and channel count.
pub struct FrameConverter {
    device_rate: u32,
    device_channels: u16,
    /// Resampler of the stream in progress, keyed by its source rate. Its
    /// filter history carries across `convert` calls so chunk boundaries are
    /// continuous. `None` when no resampled stream is active.
    active: Option<MonoResampler>,
}

impl FrameConverter {
    /// Create a converter for a device with the given rate and channel count.
    pub fn new(device_rate: u32, device_channels: u16) -> Result<Self, AudioError> {
        if device_rate == 0 || device_channels == 0 {
            return Err(AudioError::InitializationFailed(
                "device rate and channel count must be non-zero".to_owned(),
            ));
        }
        Ok(Self {
            device_rate,
            device_channels,
            active: None,
        })
    }

    /// Device sample rate in Hz.
    pub fn device_rate(&self) -> u32 {
        self.device_rate
    }

    /// Device channel count.
    pub fn device_channels(&self) -> u16 {
        self.device_channels
    }

    /// Convert one frame of a continuous stream to interleaved device-format
    /// samples.
    ///
    /// Over a whole stream (until [`finish`](Self::finish)) the output has
    /// `round(frames * device_rate / frame_rate)` frames of `device_channels`
    /// samples; individual calls lag by the resampler delay.
    pub fn convert(&mut self, frame: &AudioFrame) -> Result<Vec<f32>, AudioError> {
        let rate = frame.rate().hz();
        let mono = frame.samples();

        // A change of source rate ends the stream in progress: its tail is
        // emitted first, then a fresh resampler (or the bypass) takes over.
        let mut resampled = Vec::new();
        if self
            .active
            .as_ref()
            .is_some_and(|r| r.source_rate() != rate)
        {
            resampled = self.finish_active()?;
        }
        if rate == self.device_rate {
            resampled.extend_from_slice(mono);
        } else {
            if self.active.is_none() {
                self.active = Some(MonoResampler::new(rate, self.device_rate)?);
            }
            if let Some(r) = self.active.as_mut() {
                resampled.extend(r.stream_write(mono)?);
            }
        }
        Ok(self.fan_out(resampled))
    }

    /// End the stream in progress: returns the delayed tail as interleaved
    /// device samples (empty when nothing is pending) and resets the state.
    pub fn finish(&mut self) -> Result<Vec<f32>, AudioError> {
        let tail = self.finish_active()?;
        Ok(self.fan_out(tail))
    }

    /// Discard the stream in progress, including its filter history.
    pub fn reset(&mut self) {
        self.active = None;
    }

    fn finish_active(&mut self) -> Result<Vec<f32>, AudioError> {
        match self.active.take() {
            Some(mut r) => r.stream_finish(),
            None => Ok(Vec::new()),
        }
    }

    fn fan_out(&self, resampled: Vec<f32>) -> Vec<f32> {
        let channels = self.device_channels as usize;
        if channels == 1 {
            return resampled;
        }
        let mut out = Vec::with_capacity(resampled.len() * channels);
        for sample in resampled {
            out.extend(std::iter::repeat_n(sample, channels));
        }
        out
    }
}

/// What queueing one frame produced, in interleaved device samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Queued {
    /// Samples the conversion yielded for the frame (the resampler may hold
    /// some back until the stream ends).
    pub produced: usize,
    /// Samples that fit into the queue; the rest were dropped.
    pub accepted: usize,
}

/// Converter plus buffer: everything a sink does except talking to a device.
pub struct OutputCore {
    converter: FrameConverter,
    buffer: Arc<PlaybackBuffer>,
}

impl OutputCore {
    /// Create a core for a device with the given format, buffering at most
    /// `buffer_seconds` of audio.
    pub fn new(
        device_rate: u32,
        device_channels: u16,
        buffer_seconds: u32,
    ) -> Result<Self, AudioError> {
        let converter = FrameConverter::new(device_rate, device_channels)?;
        let capacity = device_rate as usize * device_channels as usize * buffer_seconds as usize;
        Ok(Self {
            converter,
            buffer: Arc::new(PlaybackBuffer::new(capacity)),
        })
    }

    /// Core with the default capacity ([`DEFAULT_BUFFER_SECONDS`]).
    pub fn with_default_capacity(
        device_rate: u32,
        device_channels: u16,
    ) -> Result<Self, AudioError> {
        Self::new(device_rate, device_channels, DEFAULT_BUFFER_SECONDS)
    }

    /// Convert `frame` and queue it for the device.
    pub fn write(&mut self, frame: &AudioFrame) -> Result<Queued, AudioError> {
        let samples = self.converter.convert(frame)?;
        let accepted = self.buffer.push(&samples);
        Ok(Queued {
            produced: samples.len(),
            accepted,
        })
    }

    /// Announce the end of the current stream: the resampler tail is queued
    /// (so no audio is lost) and the buffer is told that running dry is normal.
    pub fn end_stream(&mut self) -> Result<(), AudioError> {
        let tail = self.converter.finish()?;
        self.buffer.push(&tail);
        self.buffer.end_stream();
        Ok(())
    }

    /// Discard everything queued and the resampler state (barge-in / stop).
    /// Returns the number of queued samples discarded.
    pub fn flush(&mut self) -> usize {
        self.converter.reset();
        self.buffer.flush()
    }

    /// The buffer shared with the device callback.
    pub fn buffer(&self) -> &Arc<PlaybackBuffer> {
        &self.buffer
    }

    /// Device sample rate in Hz.
    pub fn device_rate(&self) -> u32 {
        self.converter.device_rate()
    }

    /// Device channel count.
    pub fn device_channels(&self) -> u16 {
        self.converter.device_channels()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use prxvc_media::{MonoTime, SampleRate};

    use super::*;

    fn tone_frame(rate: u32, samples: usize) -> AudioFrame {
        let data = (0..samples)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.5)
            .collect();
        AudioFrame::new(data, SampleRate::new(rate).unwrap(), MonoTime::ZERO, 0).unwrap()
    }

    #[test]
    fn one_second_of_16k_mono_becomes_48000_stereo_frames() {
        let mut core = OutputCore::with_default_capacity(48_000, 2).unwrap();
        core.write(&tone_frame(16_000, 16_000)).unwrap();
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 48_000 * 2);
    }

    #[test]
    fn chunked_stream_has_exact_total_length_after_end_stream() {
        let mut core = OutputCore::with_default_capacity(48_000, 2).unwrap();
        // Five 200 ms chunks of 24 kHz audio.
        for _ in 0..5 {
            core.write(&tone_frame(24_000, 4_800)).unwrap();
        }
        let before_end = core.buffer().buffered_samples();
        assert!(before_end <= 48_000 * 2);
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 48_000 * 2);
        // The next stream starts clean and is exact again.
        core.write(&tone_frame(16_000, 1_600)).unwrap();
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 48_000 * 2 + 4_800 * 2);
    }

    #[test]
    fn source_rate_change_flushes_the_old_tail() {
        let mut core = OutputCore::with_default_capacity(48_000, 1).unwrap();
        core.write(&tone_frame(16_000, 3_200)).unwrap();
        core.write(&tone_frame(24_000, 4_800)).unwrap();
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 9_600 + 9_600);
    }

    #[test]
    fn core_flush_resets_resampler_state() {
        let mut core = OutputCore::with_default_capacity(48_000, 1).unwrap();
        core.write(&tone_frame(16_000, 800)).unwrap();
        core.flush();
        core.write(&tone_frame(16_000, 1_600)).unwrap();
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 4_800);
    }

    #[test]
    fn one_second_of_24k_mono_becomes_48000_stereo_frames() {
        let mut core = OutputCore::with_default_capacity(48_000, 2).unwrap();
        core.write(&tone_frame(24_000, 24_000)).unwrap();
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 48_000 * 2);
    }

    #[test]
    fn mono_is_fanned_out_to_every_device_channel() {
        let mut conv = FrameConverter::new(16_000, 3).unwrap();
        let out = conv.convert(&tone_frame(16_000, 100)).unwrap();
        assert_eq!(out.len(), 300);
        for f in out.chunks_exact(3) {
            assert_eq!(f[0], f[1]);
            assert_eq!(f[1], f[2]);
        }
    }

    #[test]
    fn matching_rate_skips_resampling() {
        let mut conv = FrameConverter::new(16_000, 1).unwrap();
        let out = conv.convert(&tone_frame(16_000, 320)).unwrap();
        assert_eq!(out.len(), 320);
    }

    #[test]
    fn a_device_format_of_zero_is_rejected() {
        assert!(FrameConverter::new(0, 2).is_err());
        assert!(FrameConverter::new(48_000, 0).is_err());
    }

    #[test]
    fn flush_empties_the_buffer_and_signals_drained() {
        let mut core = OutputCore::with_default_capacity(48_000, 2).unwrap();
        core.write(&tone_frame(16_000, 16_000)).unwrap();
        core.end_stream().unwrap();
        assert!(!core.buffer().is_drained());
        let discarded = core.flush();
        assert_eq!(discarded, 96_000);
        assert_eq!(core.buffer().buffered_samples(), 0);
        assert!(core.buffer().is_drained());
        assert!(core.buffer().wait_drained(Duration::from_millis(1)));
    }

    #[test]
    fn write_reports_what_fit_into_a_full_queue() {
        let mut core = OutputCore::new(16_000, 1, 1).unwrap();
        let first = core.write(&tone_frame(16_000, 12_000)).unwrap();
        assert_eq!(first, Queued { produced: 12_000, accepted: 12_000 });
        let second = core.write(&tone_frame(16_000, 12_000)).unwrap();
        assert_eq!(second, Queued { produced: 12_000, accepted: 4_000 });
        assert_eq!(core.buffer().stats().dropped_samples, 8_000);
    }
}
