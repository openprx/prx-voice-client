//! Speech start and silence tracking from per-frame voice probabilities (pipeline-design 3.2): the part of the segmenter that has no text.

use std::time::Duration;

use prxvc_media::MonoTime;

/// Where the segmenter is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// No segment is open. `above` is how long the frames before now have
    /// been above the threshold in a row, beginning at `first`.
    Idle {
        first: Option<MonoTime>,
        above: Duration,
    },
    /// A segment is open.
    InSpeech,
}

/// Tracks speech and silence with hysteresis.
///
/// Speech starts after `min_speech` of frames at or above `threshold`; the
/// reported onset is moved `pad` earlier so the recogniser sees the start of
/// the first word. Inside a segment a frame counts as speech while its
/// probability stays at or above `release`. Deciding when a pause ends a
/// clause or the segment is the commit policy's job, from [`Segmenter::silence`].
///
/// A start also needs a level: the frames that make it must have a mean power of at least `floor`
/// (`pipeline.min_speech_dbfs`), so that a voiced-looking sound far below any speech does not open
/// a segment and is never decoded. The default (-75 dBFS) is 10 dB below the quietest benchmark
/// speech measured (-65 dBFS); a hum is not below it (-19 to -28 dBFS), see the checklist.
#[derive(Clone, Debug)]
pub(crate) struct Segmenter {
    threshold: f32,
    release: f32,
    min_speech: Duration,
    pad: Duration,
    /// Least mean power (mean square, full scale 1.0) of the frames that start a segment; zero is off.
    floor: f32,
    /// Sum of the powers of the frames counted in `Mode::Idle::above`, and their number.
    idle_power: f64,
    idle_frames: u32,
    mode: Mode,
    require_silence: bool,
    now: MonoTime,
    last_speech_end: MonoTime,
}

impl Segmenter {
    pub(crate) fn new(
        threshold: f32,
        release: f32,
        min_speech: Duration,
        pad: Duration,
        floor_dbfs: f32,
    ) -> Self {
        Self {
            threshold,
            release: release.min(threshold),
            min_speech,
            pad,
            floor: floor_power(floor_dbfs),
            idle_power: 0.0,
            idle_frames: 0,
            mode: Mode::Idle {
                first: None,
                above: Duration::ZERO,
            },
            require_silence: false,
            now: MonoTime::ZERO,
            last_speech_end: MonoTime::ZERO,
        }
    }

    /// Feed the probability and the mean power of the frame `[ts, ts + dur)`. Returns the onset
    /// (padded) when speech starts with this frame.
    pub(crate) fn update(
        &mut self,
        probability: f32,
        power: f32,
        ts: MonoTime,
        dur: Duration,
    ) -> Option<MonoTime> {
        let end = ts.saturating_add(dur);
        self.now = end;
        match self.mode {
            Mode::InSpeech => {
                if probability >= self.release {
                    self.last_speech_end = end;
                }
                None
            }
            Mode::Idle { first, above } => {
                self.update_idle(probability, power, ts, dur, first, above)
            }
        }
    }

    fn update_idle(
        &mut self,
        probability: f32,
        power: f32,
        ts: MonoTime,
        dur: Duration,
        first: Option<MonoTime>,
        above: Duration,
    ) -> Option<MonoTime> {
        if self.require_silence {
            if probability < self.threshold {
                self.require_silence = false;
            }
            return None;
        }
        if probability < self.threshold {
            self.forget_candidate();
            return None;
        }
        let first = first.unwrap_or(ts);
        let above = above + dur;
        self.idle_power += f64::from(power);
        self.idle_frames += 1;
        if above < self.min_speech {
            self.mode = Mode::Idle {
                first: Some(first),
                above,
            };
            return None;
        }
        if self.idle_power / f64::from(self.idle_frames.max(1)) < f64::from(self.floor) {
            // Voiced to the VAD but too quiet to be speech: start counting again.
            self.forget_candidate();
            return None;
        }
        self.mode = Mode::InSpeech;
        self.last_speech_end = ts.saturating_add(dur);
        let padded = first.as_nanos().saturating_sub(duration_nanos(self.pad));
        Some(MonoTime::from_nanos(padded))
    }

    /// Forget a half-detected onset.
    fn forget_candidate(&mut self) {
        self.mode = Mode::Idle {
            first: None,
            above: Duration::ZERO,
        };
        self.idle_power = 0.0;
        self.idle_frames = 0;
    }

    /// Silence since the last speech frame; zero outside a segment.
    pub(crate) fn silence(&self) -> Duration {
        match self.mode {
            Mode::InSpeech => self.now.saturating_since(self.last_speech_end),
            Mode::Idle { .. } => Duration::ZERO,
        }
    }

    /// End of the last speech frame.
    pub(crate) fn last_speech_end(&self) -> MonoTime {
        self.last_speech_end
    }

    /// The segment ended normally: look for the next speech.
    pub(crate) fn end_segment(&mut self) {
        self.forget_candidate();
    }

    /// The segment was cancelled while speech may still go on: ignore speech
    /// until a frame below the threshold has been seen.
    pub(crate) fn abandon(&mut self) {
        self.end_segment();
        self.require_silence = true;
    }

    /// Audio was lost: a half-detected onset is forgotten.
    pub(crate) fn discontinuity(&mut self) {
        if matches!(self.mode, Mode::Idle { .. }) {
            self.end_segment();
        }
    }

    /// Forget everything (the start of a call or a resumed stream).
    pub(crate) fn reset(&mut self) {
        self.end_segment();
        self.require_silence = false;
    }
}

/// Mean power (mean square, full scale 1.0) of a level in dBFS; a level of -100 dBFS or less turns the floor off.
fn floor_power(dbfs: f32) -> f32 {
    if dbfs <= -100.0 {
        0.0
    } else {
        10f32.powf(dbfs / 10.0)
    }
}

fn duration_nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(32);

    fn seg() -> Segmenter {
        Segmenter::new(
            0.4,
            0.25,
            Duration::from_millis(160),
            Duration::from_millis(200),
            -55.0,
        )
    }

    /// The power of a frame at -20 dBFS: speech level.
    const LOUD: f32 = 0.01;

    fn run(s: &mut Segmenter, probs: &[f32]) -> Vec<(usize, MonoTime)> {
        let mut starts = Vec::new();
        for (i, p) in probs.iter().enumerate() {
            let ts = MonoTime::from_nanos(i as u64 * 32_000_000);
            if let Some(onset) = s.update(*p, LOUD, ts, FRAME) {
                starts.push((i, onset));
            }
        }
        starts
    }

    #[test]
    fn speech_starts_after_the_minimum_and_the_onset_is_padded() {
        let mut s = seg();
        let mut probs = vec![0.0; 10];
        probs.extend([0.9; 8]);
        let starts = run(&mut s, &probs);
        assert_eq!(starts.len(), 1);
        let (index, onset) = starts[0];
        assert_eq!(index, 14, "five frames of 32 ms reach 160 ms");
        let first = MonoTime::from_nanos(10 * 32_000_000);
        assert_eq!(onset, MonoTime::from_nanos(first.as_nanos() - 200_000_000));
    }

    #[test]
    fn a_blip_shorter_than_the_minimum_starts_nothing() {
        let mut s = seg();
        assert!(run(&mut s, &[0.0, 0.9, 0.9, 0.0, 0.9, 0.9, 0.0, 0.0]).is_empty());
    }

    #[test]
    fn voiced_frames_below_the_level_floor_start_nothing_and_louder_ones_after_them_do() {
        let mut s = seg();
        let quiet = 3e-7; // -65 dBFS
        let mut starts = 0;
        for i in 0..10u64 {
            let ts = MonoTime::from_nanos(i * 32_000_000);
            starts += usize::from(s.update(0.9, quiet, ts, FRAME).is_some());
        }
        assert_eq!(
            starts, 0,
            "voiced to the VAD, but far too quiet to be speech"
        );
        for i in 10..16u64 {
            let ts = MonoTime::from_nanos(i * 32_000_000);
            starts += usize::from(s.update(0.9, LOUD, ts, FRAME).is_some());
        }
        assert_eq!(starts, 1, "the frames that follow are counted afresh");
    }

    #[test]
    fn the_floor_is_the_mean_of_the_frames_that_make_the_start_and_a_level_of_minus_100_turns_it_off()
     {
        let mut s = seg();
        // Four frames at -70 dBFS and one at -20: the mean power is above -55 dBFS.
        let powers = [1e-7, 1e-7, 1e-7, 1e-7, 0.01];
        let mut started = false;
        for (i, p) in powers.iter().enumerate() {
            let ts = MonoTime::from_nanos(i as u64 * 32_000_000);
            started |= s.update(0.9, *p, ts, FRAME).is_some();
        }
        assert!(started);
        let mut off = Segmenter::new(
            0.4,
            0.25,
            Duration::from_millis(160),
            Duration::ZERO,
            -100.0,
        );
        let mut started = false;
        for i in 0..6u64 {
            started |= off
                .update(0.9, 0.0, MonoTime::from_nanos(i * 32_000_000), FRAME)
                .is_some();
        }
        assert!(started);
    }

    #[test]
    fn onset_never_goes_below_the_origin() {
        let mut s = seg();
        let starts = run(&mut s, &[0.9; 6]);
        assert_eq!(starts[0].1, MonoTime::ZERO);
    }

    #[test]
    fn hysteresis_keeps_a_dip_between_the_thresholds_inside_speech() {
        let mut s = seg();
        run(&mut s, &[0.9; 6]);
        let base = 6;
        for i in 0..3 {
            s.update(
                0.3,
                LOUD,
                MonoTime::from_nanos((base + i) * 32_000_000),
                FRAME,
            );
        }
        assert_eq!(
            s.silence(),
            Duration::ZERO,
            "0.3 is above the release level"
        );
        for i in 3..6 {
            s.update(
                0.1,
                LOUD,
                MonoTime::from_nanos((base + i) * 32_000_000),
                FRAME,
            );
        }
        assert_eq!(s.silence(), 3 * FRAME);
    }

    #[test]
    fn silence_is_measured_from_the_end_of_the_last_speech_frame() {
        let mut s = seg();
        run(&mut s, &[0.9; 6]);
        assert_eq!(s.last_speech_end(), MonoTime::from_nanos(6 * 32_000_000));
        s.update(0.0, LOUD, MonoTime::from_nanos(6 * 32_000_000), FRAME);
        s.update(0.0, LOUD, MonoTime::from_nanos(7 * 32_000_000), FRAME);
        assert_eq!(s.silence(), Duration::from_millis(64));
    }

    #[test]
    fn after_end_segment_new_speech_starts_a_new_segment() {
        let mut s = seg();
        run(&mut s, &[0.9; 6]);
        s.end_segment();
        let mut again = Vec::new();
        for i in 0..6u64 {
            if let Some(o) = s.update(
                0.9,
                LOUD,
                MonoTime::from_nanos((20 + i) * 32_000_000),
                FRAME,
            ) {
                again.push(o);
            }
        }
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn an_abandoned_segment_ignores_speech_until_a_silent_frame() {
        let mut s = seg();
        run(&mut s, &[0.9; 6]);
        s.abandon();
        let mut starts = 0;
        for i in 0..10u64 {
            starts += usize::from(
                s.update(
                    0.9,
                    LOUD,
                    MonoTime::from_nanos((10 + i) * 32_000_000),
                    FRAME,
                )
                .is_some(),
            );
        }
        assert_eq!(starts, 0, "the same utterance goes on and stays ignored");
        s.update(0.0, LOUD, MonoTime::from_nanos(30 * 32_000_000), FRAME);
        for i in 0..6u64 {
            starts += usize::from(
                s.update(
                    0.9,
                    LOUD,
                    MonoTime::from_nanos((31 + i) * 32_000_000),
                    FRAME,
                )
                .is_some(),
            );
        }
        assert_eq!(starts, 1);
    }
}
