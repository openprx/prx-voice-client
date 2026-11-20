//! Incremental voice-activity detection over a growing buffer.
//!
//! The Whisper VAD binding only offers whole-buffer calls
//! (`segments_from_samples`), and every call starts from a fresh model state.
//! Re-running it over the whole utterance every few seconds makes the cost of
//! a pass grow with the utterance. [`IncrementalVad`] instead submits only the
//! audio that has not been examined yet plus a short context before it, and
//! carries the detected segments across passes:
//!
//! - segments that ended before the window are final and kept as they are;
//! - segments reaching into the window are cut at the window start and
//!   replaced by what the window shows, so a segment that started earlier
//!   keeps its original start;
//! - segments separated by less than the merge gap are joined, so speech that
//!   spans a pass boundary stays one segment.
//!
//! The work per pass is therefore bounded by the new audio plus the context,
//! independent of how long the utterance has been going on.

use crate::AudioError;

/// Sample rate the VAD works at.
pub const VAD_SAMPLE_RATE: usize = 16_000;

/// A detected speech segment, in samples from the start of the buffer.
pub type SpeechSegment = (usize, usize);

/// Whole-buffer voice-activity detector.
pub trait VadBackend {
    /// Speech segments of `samples` as `(start, end)` in seconds relative to
    /// the first sample.
    fn segments(&mut self, samples: &[f32]) -> Result<Vec<(f32, f32)>, AudioError>;
}

/// Runs a [`VadBackend`] over only the new part of a growing buffer.
#[derive(Debug, Clone)]
pub struct IncrementalVad {
    /// Audio before the unexamined region that is resubmitted with it.
    context: usize,
    /// Segments closer than this are one segment.
    merge_gap: usize,
    /// Samples of the buffer examined so far.
    examined: usize,
    /// Segments found so far, ordered and non-overlapping.
    segments: Vec<SpeechSegment>,
    /// Samples submitted to the backend by the latest pass.
    last_pass_samples: usize,
    /// Samples submitted to the backend since the last reset.
    total_submitted: usize,
}

impl IncrementalVad {
    /// `context_ms` should cover the VAD's silence and padding windows;
    /// `merge_gap_ms` is the silence that separates two segments.
    pub fn new(context_ms: u32, merge_gap_ms: u32) -> Self {
        let to_samples = |ms: u32| ms as usize * VAD_SAMPLE_RATE / 1000;
        Self {
            context: to_samples(context_ms),
            merge_gap: to_samples(merge_gap_ms),
            examined: 0,
            segments: Vec::new(),
            last_pass_samples: 0,
            total_submitted: 0,
        }
    }

    /// Forget everything: the buffer starts over.
    pub fn reset(&mut self) {
        self.examined = 0;
        self.segments.clear();
        self.last_pass_samples = 0;
        self.total_submitted = 0;
    }

    /// Samples of the buffer that have been examined.
    pub fn examined(&self) -> usize {
        self.examined
    }

    /// Samples the latest pass submitted to the backend.
    pub fn last_pass_samples(&self) -> usize {
        self.last_pass_samples
    }

    /// Samples submitted to the backend since the last reset.
    pub fn total_submitted(&self) -> usize {
        self.total_submitted
    }

    /// Examine the part of `audio` not seen yet and return all segments of
    /// the buffer so far. `audio` must only grow between calls (or be
    /// followed by [`reset`](Self::reset) when it is cleared).
    ///
    /// # Errors
    /// Propagates a backend failure; the pass is then retried in full next
    /// time.
    pub fn examine<B: VadBackend>(
        &mut self,
        backend: &mut B,
        audio: &[f32],
    ) -> Result<&[SpeechSegment], AudioError> {
        if audio.len() < self.examined {
            // The buffer was cleared behind our back.
            self.reset();
        }
        if audio.len() == self.examined {
            self.last_pass_samples = 0;
            return Ok(&self.segments);
        }

        let window_start = self.examined.saturating_sub(self.context);
        let window = &audio[window_start..];
        let found = backend.segments(window)?;
        self.last_pass_samples = window.len();
        self.total_submitted += window.len();

        let to_abs = |secs: f32| {
            (window_start + (secs.max(0.0) as f64 * VAD_SAMPLE_RATE as f64).round() as usize)
                .min(audio.len())
        };
        let fresh: Vec<SpeechSegment> = found
            .into_iter()
            .map(|(t0, t1)| (to_abs(t0), to_abs(t1)))
            .filter(|(s, e)| e > s)
            .collect();

        // Old segments that reach into the window are superseded by what the
        // window shows; keep only the part before it. A segment that started
        // earlier thereby keeps its start and is joined to its continuation
        // by the merge below.
        let mut all: Vec<SpeechSegment> = std::mem::take(&mut self.segments)
            .into_iter()
            .map(|(start, end)| (start, end.min(window_start)))
            .filter(|(start, end)| end > start)
            .collect();
        all.extend(fresh);
        all.sort_unstable();
        let mut merged: Vec<SpeechSegment> = Vec::with_capacity(all.len());
        for seg in all {
            match merged.last_mut() {
                Some(prev) if seg.0 <= prev.1 + self.merge_gap => prev.1 = prev.1.max(seg.1),
                _ => merged.push(seg),
            }
        }
        self.segments = merged;
        self.examined = audio.len();
        Ok(&self.segments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK: usize = 512;
    const MIN_SPEECH: usize = 250 * 16;
    const MIN_SILENCE: usize = 500 * 16;

    /// Energy detector with the segmentation rules of the real one: 512
    /// sample chunks, silence of at least `MIN_SILENCE` splits, speech
    /// shorter than `MIN_SPEECH` is dropped. Records every call.
    #[derive(Default)]
    struct EnergyVad {
        calls: Vec<usize>,
    }

    impl VadBackend for EnergyVad {
        fn segments(&mut self, samples: &[f32]) -> Result<Vec<(f32, f32)>, AudioError> {
            self.calls.push(samples.len());
            let flags: Vec<bool> = samples
                .chunks_exact(CHUNK)
                .map(|c| c.iter().map(|x| x * x).sum::<f32>() / CHUNK as f32 > 0.01)
                .collect();
            let mut out: Vec<(usize, usize)> = Vec::new();
            let mut i = 0;
            while i < flags.len() {
                if !flags[i] {
                    i += 1;
                    continue;
                }
                let start = i;
                let mut end = i + 1;
                let mut j = end;
                while j < flags.len() {
                    if flags[j] {
                        end = j + 1;
                    } else if (j - end + 1) * CHUNK >= MIN_SILENCE {
                        break;
                    }
                    j += 1;
                }
                if (end - start) * CHUNK >= MIN_SPEECH {
                    out.push((start * CHUNK, end * CHUNK));
                }
                i = end;
            }
            Ok(out
                .into_iter()
                .map(|(s, e)| (s as f32 / 16_000.0, e as f32 / 16_000.0))
                .collect())
        }
    }

    /// `(start_ms, end_ms, loud)` runs rendered as a 16 kHz signal.
    fn signal(runs: &[(u32, bool)]) -> Vec<f32> {
        let mut out = Vec::new();
        for &(ms, loud) in runs {
            let n = ms as usize * 16;
            out.extend((0..n).map(|i| if loud { if i % 2 == 0 { 0.5 } else { -0.5 } } else { 0.0 }));
        }
        out
    }

    fn run_incremental(audio: &[f32], step: usize) -> (Vec<SpeechSegment>, EnergyVad, IncrementalVad) {
        let mut backend = EnergyVad::default();
        let mut vad = IncrementalVad::new(1_000, 500);
        let mut segs = Vec::new();
        let mut end = step;
        while end < audio.len() + step {
            let upto = end.min(audio.len());
            segs = vad.examine(&mut backend, &audio[..upto]).unwrap().to_vec();
            end += step;
        }
        (segs, backend, vad)
    }

    fn whole(audio: &[f32]) -> Vec<SpeechSegment> {
        EnergyVad::default()
            .segments(audio)
            .unwrap()
            .into_iter()
            .map(|(a, b)| ((a * 16_000.0).round() as usize, (b * 16_000.0).round() as usize))
            .collect()
    }

    fn assert_close(a: &[SpeechSegment], b: &[SpeechSegment]) {
        assert_eq!(a.len(), b.len(), "segment count: {a:?} vs {b:?}");
        for (x, y) in a.iter().zip(b) {
            assert!(x.0.abs_diff(y.0) <= 2 * CHUNK && x.1.abs_diff(y.1) <= 2 * CHUNK, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn segments_match_the_whole_buffer_result() {
        let audio = signal(&[
            (1_500, false),
            (3_100, true), // spans two 2 s passes
            (900, false),  // a gap long enough to split
            (700, true),
            (200, false), // too short to split
            (600, true),
            (4_000, false),
            (1_200, true),
            (2_000, false),
        ]);
        let (inc, _, _) = run_incremental(&audio, 2 * 16_000);
        let reference = whole(&audio);
        assert_eq!(reference.len(), 3);
        assert_close(&inc, &reference);
    }

    #[test]
    fn speech_crossing_a_pass_boundary_keeps_its_original_start() {
        let audio = signal(&[(500, false), (5_000, true), (1_000, false)]);
        let (inc, _, _) = run_incremental(&audio, 2 * 16_000);
        assert_eq!(inc.len(), 1, "{inc:?}");
        assert!(inc[0].0.abs_diff(500 * 16) <= 2 * CHUNK, "{inc:?}");
        assert!(inc[0].1.abs_diff(5_500 * 16) <= 2 * CHUNK, "{inc:?}");
    }

    #[test]
    fn work_per_pass_does_not_grow_with_the_utterance() {
        // 40 s of continuous speech examined every 2 s.
        let audio = signal(&[(40_000, true)]);
        let step = 2 * 16_000;
        let (inc, backend, vad) = run_incremental(&audio, step);
        assert_eq!(inc.len(), 1);

        let bound = step + 16_000; // new audio + 1 s of context
        assert!(
            backend.calls.iter().all(|&n| n <= bound),
            "every pass is bounded by new audio plus context: {:?}",
            backend.calls
        );
        let last = *backend.calls.last().unwrap();
        assert_eq!(backend.calls[5], last, "late passes cost the same as early ones");
        assert!(last < audio.len() / 10);
        assert!(vad.total_submitted() < audio.len() * 2, "total work is linear");
        assert_eq!(vad.examined(), audio.len());
    }

    #[test]
    fn nothing_new_means_no_backend_call() {
        let audio = signal(&[(3_000, true)]);
        let mut backend = EnergyVad::default();
        let mut vad = IncrementalVad::new(1_000, 500);
        vad.examine(&mut backend, &audio).unwrap();
        vad.examine(&mut backend, &audio).unwrap();
        assert_eq!(backend.calls.len(), 1);
        assert_eq!(vad.last_pass_samples(), 0);
    }

    #[test]
    fn reset_starts_over() {
        let audio = signal(&[(3_000, true)]);
        let mut backend = EnergyVad::default();
        let mut vad = IncrementalVad::new(1_000, 500);
        vad.examine(&mut backend, &audio).unwrap();
        vad.reset();
        assert_eq!(vad.examined(), 0);
        let segs = vad.examine(&mut backend, &audio[..16_000]).unwrap().to_vec();
        assert_eq!(backend.calls, vec![audio.len(), 16_000]);
        assert_eq!(segs.len(), 1);
    }
}
