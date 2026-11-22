//! Per-callback sample handling of the process tap: reads the installed layout, counts frames, interleaves samples and detects exact digital silence.
//!
//! Real-time file: see the convention in the crate docs. Everything here runs
//! on the Core Audio IO thread, so nothing allocates, locks, logs or sends; the
//! functions only read the borrowed callback buffers and write into slices they
//! are given. The validation done once at start lives in [`crate::tap_format`].

use crate::tap_format::{InputBuffer, StreamLayout};

impl StreamLayout {
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
            let ok = buffers[..ch]
                .iter()
                .all(|b| b.channels == 1 && b.samples.len() == frames);
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
    pub fn interleave_into(
        &self,
        buffers: &[InputBuffer<'_>],
        start: usize,
        frames: usize,
        out: &mut [f32],
    ) {
        let ch = self.channels as usize;
        if self.non_interleaved {
            for c in 0..ch {
                let src = &buffers[c].samples[start..start + frames];
                for (f, &s) in src.iter().enumerate() {
                    out[f * ch + c] = s;
                }
            }
        } else {
            out[..frames * ch]
                .copy_from_slice(&buffers[0].samples[start * ch..(start + frames) * ch]);
        }
    }
}

/// Whether any sample of the buffers differs from exact digital zero.
///
/// A tap whose application has no audio-capture consent delivers buffers of
/// exact `0.0`; real audio, even a quiet room, is practically never exactly
/// zero over a whole callback. Negative zero counts as zero.
#[cfg(any(target_os = "macos", test))]
pub fn any_nonzero(buffers: &[InputBuffer<'_>]) -> bool {
    buffers.iter().any(|b| b.samples.iter().any(|&s| s != 0.0))
}
