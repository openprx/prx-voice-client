//! Hardware-independent half of the audio output sinks.
//!
//! [`CpalAudioOutput`](crate::cpal_output::CpalAudioOutput) and
//! [`BlackHoleSink`](crate::blackhole_sink::BlackHoleSink) both do the same
//! work between "a frame arrives" and "the device callback asks for samples":
//!
//! 1. [`FrameConverter`] decodes the frame (F32 / I16 / U8), collapses it to
//!    mono, resamples it to the **device rate** with the shared
//!    [`MonoResampler`], and fans the mono signal out to the **device channel
//!    count**.
//! 2. [`PlaybackBuffer`] holds the converted interleaved samples in a bounded
//!    queue shared with the device callback. It counts underruns and overruns,
//!    can be flushed, and signals when it has drained.
//!
//! [`OutputCore`] bundles the two. None of this touches audio hardware, so it
//! is unit-tested directly.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::resample::MonoResampler;
use crate::{AudioError, AudioFormat, AudioFrame};

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

#[derive(Default)]
struct BufferState {
    queue: VecDeque<f32>,
    /// Samples were queued since the buffer last drained.
    active: bool,
    /// The writer announced that no more samples are coming for now.
    ending: bool,
}

/// Bounded queue of interleaved device-rate samples shared between the
/// writer and the device callback.
pub struct PlaybackBuffer {
    state: parking_lot::Mutex<BufferState>,
    drained: parking_lot::Condvar,
    capacity: usize,
    underruns: AtomicU64,
    overruns: AtomicU64,
    dropped: AtomicU64,
}

impl PlaybackBuffer {
    /// Create a buffer holding at most `capacity_samples` samples.
    pub fn new(capacity_samples: usize) -> Self {
        Self {
            state: parking_lot::Mutex::new(BufferState::default()),
            drained: parking_lot::Condvar::new(),
            capacity: capacity_samples.max(1),
            underruns: AtomicU64::new(0),
            overruns: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    /// Maximum number of samples the buffer holds.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Queue samples. Whatever does not fit is dropped (newest first) and
    /// counted as an overrun. Returns the number of samples accepted.
    pub fn push(&self, samples: &[f32]) -> usize {
        let mut st = self.state.lock();
        let room = self.capacity.saturating_sub(st.queue.len());
        let accepted = samples.len().min(room);
        if accepted < samples.len() {
            self.overruns.fetch_add(1, Ordering::Relaxed);
            self.dropped
                .fetch_add((samples.len() - accepted) as u64, Ordering::Relaxed);
        }
        if accepted > 0 {
            st.queue.extend(&samples[..accepted]);
            st.active = true;
            // New data starts a new stream.
            st.ending = false;
        }
        accepted
    }

    /// Fill `out` from the queue, padding with silence. Called from the
    /// device callback.
    pub fn pull(&self, out: &mut [f32]) {
        let mut st = self.state.lock();
        let n = out.len().min(st.queue.len());
        for (slot, sample) in out.iter_mut().zip(st.queue.drain(..n)) {
            *slot = sample;
        }
        out[n..].fill(0.0);
        if st.queue.is_empty() && st.active {
            if n < out.len() && !st.ending {
                self.underruns.fetch_add(1, Ordering::Relaxed);
            }
            st.active = false;
            st.ending = false;
            self.drained.notify_all();
        }
    }

    /// Tell the buffer that the current stream has no more samples, so that
    /// running dry afterwards is a normal end and not an underrun.
    pub fn end_stream(&self) {
        let mut st = self.state.lock();
        if st.queue.is_empty() {
            st.active = false;
            st.ending = false;
            self.drained.notify_all();
        } else {
            st.ending = true;
        }
    }

    /// Discard everything queued and wake anyone waiting for the drain.
    /// Returns the number of samples discarded.
    pub fn flush(&self) -> usize {
        let mut st = self.state.lock();
        let discarded = st.queue.len();
        st.queue.clear();
        st.active = false;
        st.ending = false;
        self.drained.notify_all();
        discarded
    }

    /// Samples currently queued (all channels).
    pub fn buffered_samples(&self) -> usize {
        self.state.lock().queue.len()
    }

    /// Whether nothing is queued.
    pub fn is_drained(&self) -> bool {
        self.state.lock().queue.is_empty()
    }

    /// Block until the buffer is empty or `timeout` passes. Returns whether
    /// it is drained.
    pub fn wait_drained(&self, timeout: Duration) -> bool {
        let mut st = self.state.lock();
        if st.queue.is_empty() {
            return true;
        }
        let _ = self
            .drained
            .wait_while_for(&mut st, |s| !s.queue.is_empty(), timeout);
        st.queue.is_empty()
    }

    /// Snapshot of the counters.
    pub fn stats(&self) -> OutputStats {
        OutputStats {
            underruns: self.underruns.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
            dropped_samples: self.dropped.load(Ordering::Relaxed),
        }
    }
}

/// Decode a frame's payload to interleaved f32 in `[-1.0, 1.0]`.
pub fn decode_samples(frame: &AudioFrame) -> Vec<f32> {
    match frame.format {
        AudioFormat::F32 => frame
            .data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
        AudioFormat::I16 => frame
            .data
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / i16::MAX as f32)
            .collect(),
        AudioFormat::U8 => frame
            .data
            .iter()
            .map(|&b| (b as f32 - 128.0) / 128.0)
            .collect(),
    }
}

/// Converts frames of any rate / channel count / format to interleaved f32 at
/// a fixed device rate and channel count.
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
        if frame.sample_rate == 0 {
            return Err(AudioError::FormatMismatch {
                expected: "non-zero sample rate".to_owned(),
                got: "0 Hz".to_owned(),
            });
        }
        let src_channels = frame.channels.max(1) as usize;
        let decoded = decode_samples(frame);
        let mono: Vec<f32> = if src_channels == 1 {
            decoded
        } else {
            decoded
                .chunks_exact(src_channels)
                .map(|f| f.iter().sum::<f32>() / src_channels as f32)
                .collect()
        };

        // A change of source rate ends the stream in progress: its tail is
        // emitted first, then a fresh resampler (or the bypass) takes over.
        let mut resampled = Vec::new();
        if self
            .active
            .as_ref()
            .is_some_and(|r| r.source_rate() != frame.sample_rate)
        {
            resampled = self.finish_active()?;
        }
        if frame.sample_rate == self.device_rate {
            resampled.extend(mono);
        } else {
            if self.active.is_none() {
                self.active = Some(MonoResampler::new(frame.sample_rate, self.device_rate)?);
            }
            if let Some(r) = self.active.as_mut() {
                resampled.extend(r.stream_write(&mono)?);
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
    pub fn write(&mut self, frame: &AudioFrame) -> Result<(), AudioError> {
        let samples = self.converter.convert(frame)?;
        self.buffer.push(&samples);
        Ok(())
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
    use super::*;

    fn mono_i16_frame(rate: u32, samples: usize) -> AudioFrame {
        let mut data = Vec::with_capacity(samples * 2);
        for i in 0..samples {
            let v = ((2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin()
                * 0.5
                * i16::MAX as f32) as i16;
            data.extend_from_slice(&v.to_le_bytes());
        }
        AudioFrame {
            sequence_no: 0,
            sample_rate: rate,
            channels: 1,
            format: AudioFormat::I16,
            timestamp_ms: 0,
            data,
        }
    }

    #[test]
    fn one_second_of_16k_mono_becomes_48000_stereo_frames() {
        let mut core = OutputCore::with_default_capacity(48_000, 2).unwrap();
        core.write(&mono_i16_frame(16_000, 16_000)).unwrap();
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 48_000 * 2);
    }

    #[test]
    fn chunked_stream_has_exact_total_length_after_end_stream() {
        let mut core = OutputCore::with_default_capacity(48_000, 2).unwrap();
        // Five 200 ms chunks of 24 kHz audio.
        for _ in 0..5 {
            core.write(&mono_i16_frame(24_000, 4_800)).unwrap();
        }
        let before_end = core.buffer().buffered_samples();
        assert!(before_end <= 48_000 * 2);
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 48_000 * 2);
        // The next stream starts clean and is exact again.
        core.write(&mono_i16_frame(16_000, 1_600)).unwrap();
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 48_000 * 2 + 4_800 * 2);
    }

    #[test]
    fn source_rate_change_flushes_the_old_tail() {
        let mut core = OutputCore::with_default_capacity(48_000, 1).unwrap();
        core.write(&mono_i16_frame(16_000, 3_200)).unwrap();
        core.write(&mono_i16_frame(24_000, 4_800)).unwrap();
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 9_600 + 9_600);
    }

    #[test]
    fn core_flush_resets_resampler_state() {
        let mut core = OutputCore::with_default_capacity(48_000, 1).unwrap();
        core.write(&mono_i16_frame(16_000, 800)).unwrap();
        core.flush();
        core.write(&mono_i16_frame(16_000, 1_600)).unwrap();
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 4_800);
    }

    #[test]
    fn one_second_of_24k_mono_becomes_48000_stereo_frames() {
        let mut core = OutputCore::with_default_capacity(48_000, 2).unwrap();
        core.write(&mono_i16_frame(24_000, 24_000)).unwrap();
        core.end_stream().unwrap();
        assert_eq!(core.buffer().buffered_samples(), 48_000 * 2);
    }

    #[test]
    fn mono_is_fanned_out_to_every_device_channel() {
        let mut conv = FrameConverter::new(16_000, 3).unwrap();
        let out = conv.convert(&mono_i16_frame(16_000, 100)).unwrap();
        assert_eq!(out.len(), 300);
        for f in out.chunks_exact(3) {
            assert_eq!(f[0], f[1]);
            assert_eq!(f[1], f[2]);
        }
    }

    #[test]
    fn stereo_input_is_downmixed_before_fan_out() {
        let mut data = Vec::new();
        for _ in 0..10 {
            data.extend_from_slice(&1.0f32.to_le_bytes());
            data.extend_from_slice(&(-0.5f32).to_le_bytes());
        }
        let frame = AudioFrame {
            sequence_no: 0,
            sample_rate: 16_000,
            channels: 2,
            format: AudioFormat::F32,
            timestamp_ms: 0,
            data,
        };
        let mut conv = FrameConverter::new(16_000, 2).unwrap();
        let out = conv.convert(&frame).unwrap();
        assert_eq!(out.len(), 20);
        assert!(out.iter().all(|v| (*v - 0.25).abs() < 1e-6));
    }

    #[test]
    fn matching_rate_skips_resampling() {
        let mut conv = FrameConverter::new(16_000, 1).unwrap();
        let out = conv.convert(&mono_i16_frame(16_000, 320)).unwrap();
        assert_eq!(out.len(), 320);
    }

    #[test]
    fn zero_rate_frame_is_rejected() {
        let mut conv = FrameConverter::new(48_000, 2).unwrap();
        let mut frame = mono_i16_frame(16_000, 10);
        frame.sample_rate = 0;
        assert!(conv.convert(&frame).is_err());
        assert!(FrameConverter::new(0, 2).is_err());
        assert!(FrameConverter::new(48_000, 0).is_err());
    }

    #[test]
    fn flush_empties_the_buffer_and_signals_drained() {
        let mut core = OutputCore::with_default_capacity(48_000, 2).unwrap();
        core.write(&mono_i16_frame(16_000, 16_000)).unwrap();
        core.end_stream().unwrap();
        assert!(!core.buffer().is_drained());
        let discarded = core.flush();
        assert_eq!(discarded, 96_000);
        assert_eq!(core.buffer().buffered_samples(), 0);
        assert!(core.buffer().is_drained());
        assert!(core.buffer().wait_drained(Duration::from_millis(1)));
    }

    #[test]
    fn overrun_drops_the_excess_and_counts_it() {
        let buf = PlaybackBuffer::new(10);
        assert_eq!(buf.push(&[0.1; 8]), 8);
        assert_eq!(buf.push(&[0.2; 8]), 2);
        assert_eq!(buf.buffered_samples(), 10);
        let stats = buf.stats();
        assert_eq!(stats.overruns, 1);
        assert_eq!(stats.dropped_samples, 6);
        assert_eq!(stats.underruns, 0);
        // A write that fits does not count.
        let mut out = [0.0; 10];
        buf.pull(&mut out);
        buf.push(&[0.3; 5]);
        assert_eq!(buf.stats().overruns, 1);
    }

    #[test]
    fn running_dry_mid_stream_is_an_underrun() {
        let buf = PlaybackBuffer::new(100);
        buf.push(&[0.5; 4]);
        let mut out = [1.0; 8];
        buf.pull(&mut out);
        assert_eq!(&out[..4], &[0.5; 4]);
        assert_eq!(&out[4..], &[0.0; 4], "padded with silence");
        assert_eq!(buf.stats().underruns, 1);
        assert!(buf.is_drained());
        // Idle silence afterwards is not counted.
        buf.pull(&mut out);
        assert_eq!(buf.stats().underruns, 1);
    }

    #[test]
    fn announced_end_of_stream_is_not_an_underrun() {
        let buf = PlaybackBuffer::new(100);
        buf.push(&[0.5; 4]);
        buf.end_stream();
        let mut out = [0.0; 8];
        buf.pull(&mut out);
        assert_eq!(buf.stats().underruns, 0);
        assert!(buf.is_drained());

        // An exact fit is not an underrun either.
        buf.push(&[0.5; 8]);
        buf.pull(&mut out);
        assert_eq!(buf.stats().underruns, 0);
    }

    #[test]
    fn wait_drained_wakes_when_the_callback_empties_the_buffer() {
        let buf = Arc::new(PlaybackBuffer::new(100));
        buf.push(&[0.5; 16]);
        assert!(!buf.wait_drained(Duration::from_millis(5)), "times out while data is queued");

        let consumer = {
            let buf = Arc::clone(&buf);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                let mut out = [0.0; 16];
                buf.pull(&mut out);
            })
        };
        assert!(buf.wait_drained(Duration::from_secs(5)));
        consumer.join().unwrap();
    }
}
