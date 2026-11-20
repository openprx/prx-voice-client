//! Tap stream-format validation and sample layout conversion: checks the format once at start and turns each callback's buffers into interleaved f32.

use crate::tap_types::TapError;

/// Largest channel count accepted.
pub const MAX_CHANNELS: u32 = 8;
/// Largest number of buffers inspected per callback.
pub const MAX_BUFFERS: usize = 8;

const FORMAT_LPCM: u32 = 0x6c70_636d; // 'lpcm'
const FLAG_IS_FLOAT: u32 = 1 << 0;
const FLAG_IS_BIG_ENDIAN: u32 = 1 << 1;
const FLAG_IS_PACKED: u32 = 1 << 3;
const FLAG_NON_INTERLEAVED: u32 = 1 << 5;

/// The tap's `AudioStreamBasicDescription` fields that matter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawFormat {
    /// Samples per second per channel.
    pub sample_rate: f64,
    /// `mFormatID`.
    pub format_id: u32,
    /// `mFormatFlags`.
    pub format_flags: u32,
    /// `mChannelsPerFrame`.
    pub channels: u32,
    /// `mBitsPerChannel`.
    pub bits_per_channel: u32,
    /// `mBytesPerFrame`.
    pub bytes_per_frame: u32,
}

/// A validated stream layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamLayout {
    /// Samples per second per channel.
    pub sample_rate: u32,
    /// Channels per frame.
    pub channels: u32,
    /// One buffer per channel (true) or one buffer holding all channels (false).
    pub non_interleaved: bool,
}

impl StreamLayout {
    /// Validate the format reported by the tap: packed little-endian float32
    /// linear PCM, 1 to [`MAX_CHANNELS`] channels, a plausible sample rate.
    ///
    /// # Errors
    /// [`TapError::UnsupportedFormat`] naming the first problem found.
    pub fn validate(f: &RawFormat) -> Result<Self, TapError> {
        let bad = |detail: String| Err(TapError::UnsupportedFormat { detail });
        if f.format_id != FORMAT_LPCM {
            return bad(format!("format id {:#x} is not linear PCM", f.format_id));
        }
        if f.format_flags & FLAG_IS_FLOAT == 0 || f.bits_per_channel != 32 {
            return bad(format!(
                "expected 32-bit float samples, got {} bits (flags {:#x})",
                f.bits_per_channel, f.format_flags
            ));
        }
        if f.format_flags & FLAG_IS_BIG_ENDIAN != 0 {
            return bad("big-endian samples".into());
        }
        if f.format_flags & FLAG_IS_PACKED == 0 {
            return bad("samples are not packed".into());
        }
        if f.channels == 0 || f.channels > MAX_CHANNELS {
            return bad(format!("{} channels (1 to {MAX_CHANNELS} supported)", f.channels));
        }
        if !(8_000.0..=384_000.0).contains(&f.sample_rate) {
            return bad(format!("sample rate {} Hz", f.sample_rate));
        }
        let non_interleaved = f.format_flags & FLAG_NON_INTERLEAVED != 0;
        let expected_bytes = if non_interleaved { 4 } else { 4 * f.channels };
        if f.bytes_per_frame != expected_bytes {
            return bad(format!(
                "{} bytes per frame, expected {expected_bytes}",
                f.bytes_per_frame
            ));
        }
        Ok(Self { sample_rate: f.sample_rate as u32, channels: f.channels, non_interleaved })
    }

    /// Pack into one word so the real-time callback can read it with a single
    /// atomic load while the supervisor installs the layout of a new tap.
    pub fn pack(&self) -> u64 {
        (u64::from(self.sample_rate) << 32) | (u64::from(self.channels) << 1) | u64::from(self.non_interleaved)
    }

    /// Inverse of [`pack`](Self::pack); `0` (nothing installed) gives `None`.
    pub fn unpack(word: u64) -> Option<Self> {
        let channels = ((word >> 1) & 0xff) as u32;
        (channels != 0).then_some(Self {
            sample_rate: (word >> 32) as u32,
            channels,
            non_interleaved: word & 1 == 1,
        })
    }

    /// Frames available in a callback's buffers, or `None` when the buffers do
    /// not have the validated shape (so nothing is read from them).
    pub fn frames_in(&self, buffers: &[InputBuffer<'_>]) -> Option<usize> {
        let ch = self.channels as usize;
        if self.non_interleaved {
            if buffers.len() < ch {
                return None;
            }
            let frames = buffers[0].samples.len();
            let ok = buffers[..ch].iter().all(|b| b.channels == 1 && b.samples.len() == frames);
            ok.then_some(frames)
        } else {
            let b = buffers.first()?;
            let ok = b.channels as usize == ch && b.samples.len() % ch == 0;
            ok.then_some(b.samples.len() / ch)
        }
    }

    /// Write `frames` frames starting at frame `start` into `out` as
    /// interleaved samples. `out` must hold `frames * channels` samples and the
    /// range must lie inside what [`frames_in`](Self::frames_in) returned.
    /// Performs no allocation.
    pub fn interleave_into(&self, buffers: &[InputBuffer<'_>], start: usize, frames: usize, out: &mut [f32]) {
        let ch = self.channels as usize;
        if self.non_interleaved {
            for c in 0..ch {
                let src = &buffers[c].samples[start..start + frames];
                for (f, &s) in src.iter().enumerate() {
                    out[f * ch + c] = s;
                }
            }
        } else {
            out[..frames * ch].copy_from_slice(&buffers[0].samples[start * ch..(start + frames) * ch]);
        }
    }
}

/// One callback buffer viewed as samples.
#[derive(Debug, Clone, Copy)]
pub struct InputBuffer<'a> {
    /// `mNumberChannels` of the buffer.
    pub channels: u32,
    /// The samples (channel-interleaved when `channels > 1`).
    pub samples: &'a [f32],
}

impl InputBuffer<'_> {
    /// An empty buffer, used to fill fixed-size arrays.
    pub const EMPTY: InputBuffer<'static> = InputBuffer { channels: 0, samples: &[] };
}

#[cfg(test)]
mod tests;
