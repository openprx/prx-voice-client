//! Capture-format to ASR-format conversion.
//!
//! The Core Audio tap (and most system audio) delivers **interleaved float32 at
//! the device's native rate** — typically 48 kHz stereo. ASR engines
//! (SenseVoice, Moonshine, Qwen3-ASR) want **16 kHz mono float32**. This module
//! does the two conversions, streaming:
//!
//!   1. downmix interleaved N-channel → mono (channel average)
//!   2. resample source-rate → 16 kHz (rubato sinc, when rates differ)
//!
//! Pure DSP, platform-independent: the same path serves the Windows backend.

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use crate::{AudioError, AudioFormat, AudioFrame};

/// Target sample rate expected by the ASR stage.
pub const ASR_SAMPLE_RATE: u32 = 16_000;

/// Number of source-rate mono frames fed to the resampler per processing block.
const RESAMPLER_CHUNK: usize = 1024;

/// Samples of reflected context added on each side of a block in
/// [`MonoResampler::convert_block`] (half the sinc length).
const EDGE_PAD: usize = 128;

/// Average interleaved float32 channels down to a mono sample vector.
///
/// Returns [`AudioError::FormatMismatch`] if the frame is not float32, since the
/// tap path is always F32. `channels == 0` yields an empty vector.
pub fn downmix_to_mono(frame: &AudioFrame) -> Result<Vec<f32>, AudioError> {
    if frame.format != AudioFormat::F32 {
        return Err(AudioError::FormatMismatch {
            expected: "F32".to_owned(),
            got: format!("{:?}", frame.format),
        });
    }
    let channels = frame.channels.max(1) as usize;
    let sample_size = std::mem::size_of::<f32>();
    let total = frame.data.len() / sample_size;
    let frames = total / channels;

    let mut mono = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut acc = 0.0f32;
        for c in 0..channels {
            let idx = (f * channels + c) * sample_size;
            let bytes = [
                frame.data[idx],
                frame.data[idx + 1],
                frame.data[idx + 2],
                frame.data[idx + 3],
            ];
            acc += f32::from_le_bytes(bytes);
        }
        mono.push(acc / channels as f32);
    }
    Ok(mono)
}

/// Streaming mono converter between any two sample rates.
///
/// Two ways to use it:
///
/// * [`push_frame`](Self::push_frame) / [`push_samples`](Self::push_samples):
///   continuous capture-style streaming. It buffers across calls and emits
///   whatever samples are ready, so the output lags the input by the filter
///   delay and a partial block.
/// * [`convert_block`](Self::convert_block): self-contained conversion of one
///   block. The output has exactly `round(len * target / source)` samples and
///   no delay, which is what a playback sink needs when it is handed whole
///   frames (one second in gives one second out).
///
/// When both rates are equal the resampler is bypassed.
pub struct MonoResampler {
    /// `None` when the rates are equal (bypass).
    resampler: Option<SincFixedIn<f32>>,
    /// Mono samples at the source rate, awaiting a full resampler chunk.
    pending: Vec<f32>,
    source_rate: u32,
    target_rate: u32,
    /// Continuous-stream bookkeeping for the `stream_*` methods.
    stream: StreamState,
}

/// Position of a continuous stream fed through [`MonoResampler::stream_write`].
#[derive(Debug, Clone, Copy, Default)]
struct StreamState {
    /// Source samples accepted so far.
    in_total: u64,
    /// Output samples emitted so far (after `skip_left` ones were dropped).
    out_total: u64,
    /// Leading filter output still to be dropped (see [`lead_shift`]).
    skip_left: usize,
    /// Whether the stream's alignment shift was applied.
    started: bool,
}

/// Output samples by which the filter output must be moved earlier (positive)
/// or later (negative) to be aligned with the input.
///
/// Measured on the `rubato` sinc resampler used here: its output lags the
/// input by `ratio - 1` output samples (an impulse at input sample `n` lands at
/// `n * ratio + ratio - 1`), and it has no other start-up delay. The shift is
/// rounded to whole samples, so the residual misalignment is at most half an
/// output sample (none at the integer ratios 2 and 3 of 24 kHz and 16 kHz to
/// 48 kHz).
fn lead_shift(ratio: f64) -> i64 {
    (ratio - 1.0).round() as i64
}

impl MonoResampler {
    /// Build a converter from `source_rate` to `target_rate`.
    ///
    /// # Errors
    /// [`AudioError::InitializationFailed`] if a rate is zero or the
    /// resampler cannot be built.
    pub fn new(source_rate: u32, target_rate: u32) -> Result<Self, AudioError> {
        if source_rate == 0 || target_rate == 0 {
            return Err(AudioError::InitializationFailed(
                "sample rate is zero".to_owned(),
            ));
        }
        let resampler = if source_rate == target_rate {
            None
        } else {
            let ratio = target_rate as f64 / source_rate as f64;
            let params = SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 256,
                window: WindowFunction::BlackmanHarris2,
            };
            let r = SincFixedIn::<f32>::new(ratio, 2.0, params, RESAMPLER_CHUNK, 1)
                .map_err(|e| AudioError::InitializationFailed(format!("resampler init: {e}")))?;
            Some(r)
        };
        Ok(Self {
            resampler,
            pending: Vec::new(),
            source_rate,
            target_rate,
            stream: StreamState::default(),
        })
    }

    /// Source sample rate this converter was built for.
    pub fn source_rate(&self) -> u32 {
        self.source_rate
    }

    /// Target sample rate this converter produces.
    pub fn target_rate(&self) -> u32 {
        self.target_rate
    }

    /// Convert one captured frame (downmix, then resample), returning any
    /// samples now ready. May return an empty vector while input accumulates.
    ///
    /// # Errors
    /// Propagates [`downmix_to_mono`] format errors and resampler failures.
    pub fn push_frame(&mut self, frame: &AudioFrame) -> Result<Vec<f32>, AudioError> {
        let mono = downmix_to_mono(frame)?;
        self.push_samples(&mono)
    }

    /// Streaming conversion of mono samples at the source rate.
    pub fn push_samples(&mut self, mono: &[f32]) -> Result<Vec<f32>, AudioError> {
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(mono.to_vec());
        };
        self.pending.extend_from_slice(mono);
        let mut out = Vec::new();
        loop {
            let needed = resampler.input_frames_next();
            if self.pending.len() < needed {
                break;
            }
            let chunk: Vec<f32> = self.pending.drain(..needed).collect();
            let resampled = resampler
                .process(&[chunk], None)
                .map_err(|e| AudioError::InitializationFailed(format!("resample: {e}")))?;
            if let Some(channel0) = resampled.into_iter().next() {
                out.extend_from_slice(&channel0);
            }
        }
        Ok(out)
    }

    /// Drop all stream state: filter history, partial input and position.
    /// The next [`stream_write`](Self::stream_write) starts a new stream.
    pub fn reset_stream(&mut self) {
        if let Some(r) = self.resampler.as_mut() {
            r.reset();
        }
        self.pending.clear();
        self.stream = StreamState::default();
    }

    /// Run the filter over every full chunk in `pending` and append the
    /// result to `out`.
    fn drain_stream_chunks(&mut self, out: &mut Vec<f32>) -> Result<(), AudioError> {
        loop {
            let Some(needed) = self.resampler.as_ref().map(|r| r.input_frames_next()) else {
                return Ok(());
            };
            if self.pending.len() < needed {
                return Ok(());
            }
            let chunk: Vec<f32> = self.pending.drain(..needed).collect();
            self.stream_process(chunk, out)?;
        }
    }

    fn stream_process(&mut self, chunk: Vec<f32>, out: &mut Vec<f32>) -> Result<(), AudioError> {
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(());
        };
        let resampled = resampler
            .process(&[chunk], None)
            .map_err(|e| AudioError::InitializationFailed(format!("resample: {e}")))?;
        if let Some(channel0) = resampled.into_iter().next() {
            let skip = self.stream.skip_left.min(channel0.len());
            self.stream.skip_left -= skip;
            let kept = &channel0[skip..];
            self.stream.out_total += kept.len() as u64;
            out.extend_from_slice(kept);
        }
        Ok(())
    }

    /// Continuous streaming conversion: successive calls form one stream whose
    /// filter history and fractional position carry over, so chunk boundaries
    /// are invisible. The output is the input signal re-sampled in time (not
    /// shifted: an impulse at input sample `n` lands at output sample
    /// `n * target / source`, see `stream_keeps_the_time_alignment`), but it lags
    /// the input by up to one internal block plus the filter look-ahead until
    /// [`stream_finish`](Self::stream_finish) drains the tail.
    ///
    /// The alignment shift is [`lead_shift`]. (An earlier version dropped
    /// `rubato`'s reported `output_delay`, which is not a delay of its output;
    /// that cut about 8 ms off the start and moved the whole stream earlier by
    /// as much.)
    pub fn stream_write(&mut self, mono: &[f32]) -> Result<Vec<f32>, AudioError> {
        if self.resampler.is_none() {
            return Ok(mono.to_vec());
        }
        let mut out = Vec::new();
        if !self.stream.started {
            self.stream.started = true;
            let shift = lead_shift(self.target_rate as f64 / self.source_rate as f64);
            self.stream.skip_left = shift.max(0) as usize;
            // A negative shift delays the stream by leading silence.
            out.resize(shift.unsigned_abs() as usize * usize::from(shift < 0), 0.0);
            self.stream.out_total += out.len() as u64;
        }
        self.pending.extend_from_slice(mono);
        self.stream.in_total += mono.len() as u64;
        self.drain_stream_chunks(&mut out)?;
        Ok(out)
    }

    /// End the current stream: flush the delayed tail so that the whole stream
    /// has exactly `round(total_in * target / source)` samples, then reset.
    pub fn stream_finish(&mut self) -> Result<Vec<f32>, AudioError> {
        if self.resampler.is_none() || self.stream.in_total == 0 {
            self.reset_stream();
            return Ok(Vec::new());
        }
        let ratio = self.target_rate as f64 / self.source_rate as f64;
        let expected = (self.stream.in_total as f64 * ratio).round() as u64;
        let before = self.stream.out_total;
        let mut out = Vec::new();
        while self.stream.out_total < expected {
            let needed = match self.resampler.as_ref() {
                Some(r) => r.input_frames_next(),
                None => break,
            };
            let mut chunk: Vec<f32> = self
                .pending
                .drain(..needed.min(self.pending.len()))
                .collect();
            chunk.resize(needed, 0.0);
            self.stream_process(chunk, &mut out)?;
        }
        let want = expected.saturating_sub(before) as usize;
        out.truncate(want);
        self.reset_stream();
        Ok(out)
    }

    /// Convert one self-contained block of mono samples.
    ///
    /// The result has exactly `round(len * target / source)` samples and
    /// is time-aligned with the input. The block is treated as isolated: the
    /// filter state is reset before and after and the edges are extended by
    /// mirror reflection, so consecutive blocks are not sample-exact across
    /// their boundary (inaudible for speech-length blocks).
    ///
    /// Discards any partial input left by the streaming methods.
    pub fn convert_block(&mut self, mono: &[f32]) -> Result<Vec<f32>, AudioError> {
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(mono.to_vec());
        };
        if mono.is_empty() {
            return Ok(Vec::new());
        }
        self.pending.clear();
        self.stream = StreamState::default();
        resampler.reset();

        let ratio = self.target_rate as f64 / self.source_rate as f64;
        let expected = (mono.len() as f64 * ratio).round() as usize;

        // Extend both ends by mirror reflection (value-continuous and bounded),
        // so a block that starts or stops at a non-zero level does not look
        // like a step to the filter and ring at its edges.
        let pad = EDGE_PAD.min(mono.len() - 1);
        let mut ext: Vec<f32> = Vec::with_capacity(mono.len() + 2 * pad);
        ext.extend((1..=pad).rev().map(|k| mono[k]));
        ext.extend_from_slice(mono);
        ext.extend((1..=pad).map(|k| mono[mono.len() - 1 - k]));
        let head_skip = (pad as f64 * ratio).round() as usize;
        let shift = lead_shift(ratio);
        let start = (head_skip as i64 + shift).max(0) as usize;
        let need = start + expected;

        let mut out: Vec<f32> = Vec::with_capacity(need + RESAMPLER_CHUNK * 4);
        let mut consumed = 0usize;
        // Feed the block, then zeros, until the delayed tail has come out.
        while out.len() < need {
            let needed = resampler.input_frames_next();
            let mut chunk = vec![0.0f32; needed];
            let take = needed.min(ext.len() - consumed);
            chunk[..take].copy_from_slice(&ext[consumed..consumed + take]);
            consumed += take;
            let resampled = resampler
                .process(&[chunk], None)
                .map_err(|e| AudioError::InitializationFailed(format!("resample: {e}")))?;
            if let Some(channel0) = resampled.into_iter().next() {
                out.extend_from_slice(&channel0);
            }
        }
        resampler.reset();

        out.drain(..start);
        out.truncate(expected);
        Ok(out)
    }
}

/// Streaming converter: interleaved float32 [`AudioFrame`]s of arbitrary size →
/// 16 kHz mono float32 samples, ready for ASR.
///
/// Feed captured frames with [`push_frame`](Self::push_frame); it buffers across
/// calls and emits whatever 16 kHz samples are ready. When the source is already
/// 16 kHz it bypasses the resampler (downmix only). This is a thin wrapper over
/// [`MonoResampler`] with the target fixed to [`ASR_SAMPLE_RATE`].
pub struct MonoResampler16k {
    inner: MonoResampler,
}

impl MonoResampler16k {
    /// Build a converter for the given source sample rate.
    ///
    /// # Errors
    /// [`AudioError::InitializationFailed`] if the resampler cannot be built
    /// (e.g. an invalid source rate).
    pub fn new(source_rate: u32) -> Result<Self, AudioError> {
        Ok(Self {
            inner: MonoResampler::new(source_rate, ASR_SAMPLE_RATE)?,
        })
    }

    /// Source sample rate this converter was built for.
    pub fn source_rate(&self) -> u32 {
        self.inner.source_rate()
    }

    /// Convert one captured frame, returning any 16 kHz mono samples now ready.
    ///
    /// May return an empty vector when not enough input has accumulated yet.
    ///
    /// # Errors
    /// Propagates [`downmix_to_mono`] format errors and resampler failures.
    pub fn push_frame(&mut self, frame: &AudioFrame) -> Result<Vec<f32>, AudioError> {
        self.inner.push_frame(frame)
    }
}

/// Wrap 16 kHz mono float32 samples into an [`AudioFrame`] for downstream stages.
pub fn mono_16k_frame(sequence_no: u64, samples: &[f32]) -> AudioFrame {
    let mut data = Vec::with_capacity(std::mem::size_of_val(samples));
    for s in samples {
        data.extend_from_slice(&s.to_le_bytes());
    }
    AudioFrame {
        sequence_no,
        sample_rate: ASR_SAMPLE_RATE,
        channels: 1,
        format: AudioFormat::F32,
        timestamp_ms: 0,
        data,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo_frame(samples_lr: &[(f32, f32)], rate: u32) -> AudioFrame {
        let mut data = Vec::new();
        for (l, r) in samples_lr {
            data.extend_from_slice(&l.to_le_bytes());
            data.extend_from_slice(&r.to_le_bytes());
        }
        AudioFrame {
            sequence_no: 0,
            sample_rate: rate,
            channels: 2,
            format: AudioFormat::F32,
            timestamp_ms: 0,
            data,
        }
    }

    #[test]
    fn downmix_averages_channels() {
        let frame = stereo_frame(&[(1.0, -1.0), (0.5, 0.5), (0.0, 1.0)], 48_000);
        let mono = downmix_to_mono(&frame).unwrap();
        assert_eq!(mono, vec![0.0, 0.5, 0.5]);
    }

    #[test]
    fn downmix_rejects_non_f32() {
        let frame = AudioFrame {
            sequence_no: 0,
            sample_rate: 16_000,
            channels: 1,
            format: AudioFormat::I16,
            timestamp_ms: 0,
            data: vec![0u8; 4],
        };
        assert!(downmix_to_mono(&frame).is_err());
    }

    #[test]
    fn bypass_when_already_16k() {
        let mut conv = MonoResampler16k::new(16_000).unwrap();
        assert!(conv.inner.resampler.is_none());
        let frame = stereo_frame(&[(0.2, 0.4), (0.6, 0.8)], 16_000);
        let out = conv.push_frame(&frame).unwrap();
        let expected = [0.3f32, 0.7f32];
        assert_eq!(out.len(), expected.len());
        for (got, want) in out.iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    #[test]
    fn resamples_48k_to_16k_roughly_one_third() {
        let mut conv = MonoResampler16k::new(48_000).unwrap();
        // Feed ~4 resampler chunks worth of 48k mono-as-stereo silence-ish ramp.
        let mut produced = 0usize;
        for _ in 0..8 {
            let block: Vec<(f32, f32)> = (0..1024)
                .map(|i| (i as f32 / 1024.0, i as f32 / 1024.0))
                .collect();
            let frame = stereo_frame(&block, 48_000);
            produced += conv.push_frame(&frame).unwrap().len();
        }
        // 8*1024 input @48k -> ~ (8*1024)/3 output @16k; allow generous slack for
        // sinc warm-up / internal buffering.
        let expected = (8 * 1024) / 3;
        assert!(
            produced > expected / 2,
            "produced {produced}, expected ~{expected}"
        );
    }

    fn sine(rate: u32, hz: f32, seconds: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    fn zero_crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
    }

    #[test]
    fn convert_block_yields_exact_length() {
        for (src, dst) in [
            (16_000, 48_000),
            (24_000, 48_000),
            (22_050, 48_000),
            (48_000, 16_000),
        ] {
            let mut r = MonoResampler::new(src, dst).unwrap();
            let out = r.convert_block(&sine(src, 440.0, 1.0)).unwrap();
            assert_eq!(out.len(), dst as usize, "{src} -> {dst}");
            // Repeated use must give the same length (state is reset).
            let again = r.convert_block(&sine(src, 440.0, 0.02)).unwrap();
            assert_eq!(again.len(), (dst as f64 * 0.02).round() as usize);
        }
    }

    #[test]
    fn convert_block_preserves_pitch_and_level() {
        let mut r = MonoResampler::new(16_000, 48_000).unwrap();
        let out = r.convert_block(&sine(16_000, 440.0, 1.0)).unwrap();
        // 440 Hz for one second: 440 rising zero crossings, edges included.
        let zc = zero_crossings(&out);
        assert!((439..=441).contains(&zc), "zero crossings {zc}");
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.03, "peak {peak}");
    }

    #[test]
    fn convert_block_has_no_ringing_at_a_non_zero_edge() {
        // A block that ends mid-wave must not gain high-frequency edge noise.
        let mut r = MonoResampler::new(16_000, 48_000).unwrap();
        let out = r.convert_block(&sine(16_000, 440.0, 0.2)).unwrap();
        let tail = &out[out.len() - 60..];
        let max_step = tail
            .windows(2)
            .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        // A 440 Hz wave of amplitude 0.5 at 48 kHz moves at most ~0.03 per sample.
        assert!(max_step < 0.05, "edge step {max_step}");
    }

    #[test]
    fn convert_block_bypass_and_empty() {
        let mut same = MonoResampler::new(16_000, 16_000).unwrap();
        assert_eq!(same.convert_block(&[0.1, 0.2]).unwrap(), vec![0.1, 0.2]);
        let mut up = MonoResampler::new(16_000, 48_000).unwrap();
        assert!(up.convert_block(&[]).unwrap().is_empty());
        assert!(MonoResampler::new(0, 48_000).is_err());
        assert!(MonoResampler::new(16_000, 0).is_err());
    }

    /// Whole-stream conversion through the stateful path, fed in `chunks`.
    /// Position of the largest absolute sample.
    fn peak_position(x: &[f32]) -> usize {
        x.iter()
            .enumerate()
            .fold(
                (0, 0.0f32),
                |a, (i, v)| if v.abs() > a.1 { (i, v.abs()) } else { a },
            )
            .0
    }

    #[test]
    fn stream_keeps_the_time_alignment() {
        // An impulse at input sample 4000 must land at 4000 * target / source,
        // for streams (any chunking) and for blocks, up and down.
        for (src, dst) in [
            (16_000u32, 48_000u32),
            (24_000, 48_000),
            (44_100, 48_000),
            (48_000, 16_000),
        ] {
            let mut input = vec![0.0f32; 16_000];
            input[4_000] = 1.0;
            let want = (4_000.0 * dst as f64 / src as f64).round() as usize;

            let mut streamed = MonoResampler::new(src, dst).unwrap();
            let mut out = Vec::new();
            for chunk in input.chunks(333) {
                out.extend(streamed.stream_write(chunk).unwrap());
            }
            out.extend(streamed.stream_finish().unwrap());
            let at = peak_position(&out);
            assert!(
                at.abs_diff(want) <= 1,
                "{src}->{dst} stream: peak at {at}, want {want}"
            );

            let block = MonoResampler::new(src, dst)
                .unwrap()
                .convert_block(&input)
                .unwrap();
            let at = peak_position(&block);
            assert!(
                at.abs_diff(want) <= 1,
                "{src}->{dst} block: peak at {at}, want {want}"
            );
        }
    }

    fn stream_convert(r: &mut MonoResampler, x: &[f32], chunks: &[usize]) -> Vec<f32> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        let mut i = 0usize;
        while pos < x.len() {
            let n = chunks[i % chunks.len()].min(x.len() - pos);
            out.extend(r.stream_write(&x[pos..pos + n]).unwrap());
            pos += n;
            i += 1;
        }
        out.extend(r.stream_finish().unwrap());
        out
    }

    const IRREGULAR: [usize; 7] = [3200, 1, 777, 4801, 2, 1500, 333];

    #[test]
    fn chunked_stream_matches_one_block_and_has_no_clicks() {
        let mut worst = 0.0f32;
        for (src, dst) in [(16_000u32, 48_000u32), (24_000, 48_000), (22_050, 48_000)] {
            let x = sine(src, 440.0, 1.0);
            let mut whole_r = MonoResampler::new(src, dst).unwrap();
            let whole = stream_convert(&mut whole_r, &x, &[x.len()]);
            let mut chunk_r = MonoResampler::new(src, dst).unwrap();
            let chunked = stream_convert(&mut chunk_r, &x, &IRREGULAR);

            let expected = (x.len() as f64 * dst as f64 / src as f64).round() as usize;
            assert!(
                whole.len().abs_diff(expected) <= 1,
                "{src}: whole {}",
                whole.len()
            );
            assert!(
                chunked.len().abs_diff(whole.len()) <= 1,
                "{src}: len differs"
            );

            let n = whole.len().min(chunked.len());
            let edge = 600;
            let diff = (edge..n - edge)
                .map(|k| (whole[k] - chunked[k]).abs())
                .fold(0.0f32, f32::max);
            worst = worst.max(diff);
            eprintln!("{src} -> {dst}: max chunked-vs-whole diff {diff:e}");
            assert!(diff < 1e-3, "{src}: max diff {diff}");

            // Click detector: 0.5 * 2*pi*440/dst is the largest step of the
            // sine; allow the same precision bound on top.
            let max_step_allowed = 0.5 * 2.0 * std::f32::consts::PI * 440.0 / dst as f32 + 1e-3;
            let max_step = chunked[edge..n - edge]
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max);
            assert!(
                max_step <= max_step_allowed,
                "{src}: jump {max_step} > {max_step_allowed}"
            );
        }
        eprintln!("worst chunked-vs-whole diff {worst:e}");
    }

    #[test]
    fn stream_length_is_exact_for_odd_chunking() {
        for (src, dst) in [(16_000u32, 48_000u32), (24_000, 48_000), (22_050, 48_000)] {
            for total in [1usize, 100, 1023, 1024, 5000, 12_345] {
                let x = sine(src, 440.0, total as f32 / src as f32);
                let mut r = MonoResampler::new(src, dst).unwrap();
                let out = stream_convert(&mut r, &x, &IRREGULAR);
                let expected = (x.len() as f64 * dst as f64 / src as f64).round() as usize;
                assert!(
                    out.len().abs_diff(expected) <= 1,
                    "{src} total {total}: {} vs {expected}",
                    out.len()
                );
            }
        }
    }

    #[test]
    fn stream_reset_starts_a_clean_stream() {
        let x = sine(16_000, 440.0, 0.3);
        let mut fresh = MonoResampler::new(16_000, 48_000).unwrap();
        let reference = stream_convert(&mut fresh, &x, &IRREGULAR);

        // Leave history and a partial block behind, then reset.
        let mut r = MonoResampler::new(16_000, 48_000).unwrap();
        r.stream_write(&sine(16_000, 1_000.0, 0.05)).unwrap();
        r.reset_stream();
        let after_reset = stream_convert(&mut r, &x, &IRREGULAR);
        assert_eq!(after_reset, reference);

        // stream_finish also resets, so the same instance can be reused.
        let again = stream_convert(&mut r, &x, &IRREGULAR);
        assert_eq!(again, reference);
    }
}
