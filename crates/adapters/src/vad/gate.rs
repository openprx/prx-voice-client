//! Speech-start and speech-end events from per-frame speech probabilities.
//!
//! The rules are those of the benchmark's `VadGate` (bench/asr `engines.py`), which the pipeline
//! design takes as its own (PD-3.2): speech starts after `min_speech` of frames at or above
//! `threshold` and begins `pad` before the first of them; below `neg_threshold` a frame counts as
//! silence; the segment ends after `min_silence` of silence (`long_silence` once it is longer than
//! `long_after`) or at `max_segment`, and ends `pad` after the last voiced frame.

use std::time::Duration;

use prxvc_media::MonoTime;

/// Thresholds and durations of the gate. The defaults are the reference application's values
/// (`[pipeline]` in the configuration: `vad_threshold`, `min_speech_ms`, `pad_ms`,
/// `segment_end_pause_ms`, `long_segment_after_s`, `long_segment_pause_ms`, `max_segment_s`).
#[derive(Clone, Debug, PartialEq)]
pub struct GateParams {
    /// Probability at or above which a frame counts as speech.
    pub threshold: f32,
    /// Probability below which a frame inside a segment counts as silence.
    pub neg_threshold: f32,
    /// Speech shorter than this does not start a segment.
    pub min_speech: Duration,
    /// Audio kept before the first and after the last voiced frame.
    pub pad: Duration,
    /// Silence that ends a segment.
    pub min_silence: Duration,
    /// After a segment has run this long the shorter `long_silence` ends it.
    pub long_after: Duration,
    /// Segment-ending silence of a long segment.
    pub long_silence: Duration,
    /// A segment is ended at this length.
    pub max_segment: Duration,
}

impl Default for GateParams {
    fn default() -> Self {
        Self {
            threshold: 0.4,
            neg_threshold: 0.25,
            min_speech: Duration::from_millis(160),
            pad: Duration::from_millis(200),
            min_silence: Duration::from_millis(600),
            long_after: Duration::from_secs(8),
            long_silence: Duration::from_millis(300),
            max_segment: Duration::from_secs(25),
        }
    }
}

/// What the gate reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VadEvent {
    /// A segment began.
    SpeechStart {
        /// Onset of the segment including the padding.
        at: MonoTime,
        /// The end of the frame that confirmed the start (when the event could first be known).
        detected_at: MonoTime,
    },
    /// The segment ended.
    SpeechEnd {
        /// Onset of the segment (the same as the matching start).
        start: MonoTime,
        /// End of the segment including the padding.
        end: MonoTime,
        /// The end of the frame that confirmed the end.
        detected_at: MonoTime,
        /// Whether the segment was cut at `max_segment` instead of ending in silence.
        forced: bool,
    },
}

#[derive(Clone, Copy, Debug)]
enum State {
    Idle {
        /// Start and count of the consecutive frames at or above the threshold.
        candidate: Option<(MonoTime, u32)>,
    },
    Speaking {
        start: MonoTime,
        last_voiced: MonoTime,
        silent_frames: u32,
    },
}

/// The event logic; one per direction.
#[derive(Clone, Debug)]
pub struct SpeechGate {
    params: GateParams,
    frame: Duration,
    state: State,
}

impl SpeechGate {
    /// A gate for frames of duration `frame`.
    pub fn new(params: GateParams, frame: Duration) -> Self {
        Self {
            params,
            frame: frame.max(Duration::from_micros(1)),
            state: State::Idle { candidate: None },
        }
    }

    /// The parameters in use.
    pub fn params(&self) -> &GateParams {
        &self.params
    }

    /// Whether a segment is open.
    pub fn is_speaking(&self) -> bool {
        matches!(self.state, State::Speaking { .. })
    }

    /// Forget everything (the start of a new call).
    pub fn reset(&mut self) {
        self.state = State::Idle { candidate: None };
    }

    fn min_speech_frames(&self) -> u32 {
        let n = self
            .params
            .min_speech
            .as_nanos()
            .div_ceil(self.frame.as_nanos());
        u32::try_from(n).unwrap_or(u32::MAX).max(1)
    }

    /// Feed the probability of the frame that starts at `frame_start`.
    pub fn push(&mut self, probability: f32, frame_start: MonoTime) -> Option<VadEvent> {
        let frame_end = frame_start.saturating_add(self.frame);
        let p = &self.params;
        match self.state {
            State::Idle { candidate } => {
                if probability >= p.threshold {
                    let (since, count) = match candidate {
                        Some((since, count)) => (since, count + 1),
                        None => (frame_start, 1),
                    };
                    if count >= self.min_speech_frames() {
                        let at = MonoTime::from_nanos(
                            since.as_nanos().saturating_sub(duration_nanos(p.pad)),
                        );
                        self.state = State::Speaking {
                            start: at,
                            last_voiced: frame_end,
                            silent_frames: 0,
                        };
                        return Some(VadEvent::SpeechStart {
                            at,
                            detected_at: frame_end,
                        });
                    }
                    self.state = State::Idle {
                        candidate: Some((since, count)),
                    };
                } else if probability < p.neg_threshold {
                    self.state = State::Idle { candidate: None };
                }
                None
            }
            State::Speaking {
                start,
                last_voiced,
                silent_frames,
            } => {
                let (last_voiced, silent_frames) = if probability >= p.neg_threshold {
                    (frame_end, 0)
                } else {
                    (last_voiced, silent_frames + 1)
                };
                let length = frame_end.saturating_since(start);
                let limit = if length > p.long_after {
                    p.long_silence
                } else {
                    p.min_silence
                };
                let silence = self.frame.saturating_mul(silent_frames);
                let forced = length >= p.max_segment;
                if silence >= limit || forced {
                    let end = last_voiced.saturating_add(p.pad).min(frame_end);
                    self.state = State::Idle { candidate: None };
                    return Some(VadEvent::SpeechEnd {
                        start,
                        end,
                        detected_at: frame_end,
                        forced: forced && silence < limit,
                    });
                }
                self.state = State::Speaking {
                    start,
                    last_voiced,
                    silent_frames,
                };
                None
            }
        }
    }

    /// The input ended: close an open segment at the last voiced frame plus padding.
    pub fn flush(&mut self, now: MonoTime) -> Option<VadEvent> {
        let State::Speaking {
            start, last_voiced, ..
        } = self.state
        else {
            self.state = State::Idle { candidate: None };
            return None;
        };
        self.state = State::Idle { candidate: None };
        Some(VadEvent::SpeechEnd {
            start,
            end: last_voiced.saturating_add(self.params.pad).min(now),
            detected_at: now,
            forced: false,
        })
    }
}

fn duration_nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(32);

    fn at(frame: u64) -> MonoTime {
        MonoTime::from_nanos(frame * 32_000_000)
    }

    /// Run the gate over `probs`, returning every event with the index of the frame that caused it.
    fn run(params: GateParams, probs: &[f32]) -> Vec<(usize, VadEvent)> {
        let mut gate = SpeechGate::new(params, FRAME);
        probs
            .iter()
            .enumerate()
            .filter_map(|(i, &p)| gate.push(p, at(i as u64)).map(|e| (i, e)))
            .collect()
    }

    fn seq(parts: &[(f32, usize)]) -> Vec<f32> {
        parts.iter().flat_map(|&(p, n)| vec![p; n]).collect()
    }

    fn ms(frames: u64) -> u64 {
        frames * 32
    }

    #[test]
    fn silence_and_noise_make_no_events() {
        assert!(
            run(
                GateParams::default(),
                &seq(&[(0.0, 100), (0.3, 100), (0.05, 100)])
            )
            .is_empty()
        );
    }

    #[test]
    fn speech_shorter_than_the_minimum_is_ignored() {
        // Four frames are 128 ms, under the 160 ms minimum (five frames).
        let probs = seq(&[(0.0, 10), (0.9, 4), (0.0, 40)]);
        assert!(run(GateParams::default(), &probs).is_empty());
    }

    #[test]
    fn a_gap_in_the_onset_restarts_the_count() {
        // 3 + 3 voiced frames split by a frame below the end threshold: never five in a row.
        let probs = seq(&[(0.9, 3), (0.1, 1), (0.9, 3), (0.0, 40)]);
        assert!(run(GateParams::default(), &probs).is_empty());
        // A frame between the two thresholds does not break the run.
        let probs = seq(&[(0.9, 3), (0.3, 1), (0.9, 2), (0.0, 40)]);
        assert_eq!(run(GateParams::default(), &probs).len(), 2);
    }

    #[test]
    fn start_and_end_follow_the_rules_with_padding() {
        // Silence 10 frames, speech 30 frames (frames 10..40), silence after.
        let probs = seq(&[(0.0, 10), (0.9, 30), (0.0, 40)]);
        let events = run(GateParams::default(), &probs);
        assert_eq!(events.len(), 2);
        let (i, start) = events[0];
        // Confirmed by the fifth voiced frame (index 14), reported at the end of that frame.
        assert_eq!(i, 14);
        let VadEvent::SpeechStart {
            at: onset,
            detected_at,
        } = start
        else {
            panic!("expected a start");
        };
        // Onset is the first voiced frame (10) minus 200 ms padding.
        assert_eq!(onset.as_nanos(), at(10).as_nanos() - 200_000_000);
        assert_eq!(detected_at, at(15));
        let (j, end) = events[1];
        let VadEvent::SpeechEnd {
            start: s,
            end: e,
            detected_at,
            forced,
        } = end
        else {
            panic!("expected an end");
        };
        assert_eq!(s, onset);
        assert!(!forced);
        // 600 ms of silence is 19 frames (608 ms): the 19th silent frame is index 40 + 18.
        assert_eq!(j, 40 + 18);
        assert_eq!(detected_at, at(59));
        // Last voiced frame 39 ends at frame 40; the end is that plus the padding.
        assert_eq!(e.as_nanos(), at(40).as_nanos() + 200_000_000);
    }

    #[test]
    fn a_short_pause_inside_speech_does_not_end_it() {
        let probs = seq(&[(0.9, 20), (0.0, 10), (0.9, 20), (0.0, 40)]);
        let events = run(GateParams::default(), &probs);
        assert_eq!(events.len(), 2);
        let VadEvent::SpeechEnd { end, .. } = events[1].1 else {
            panic!("expected an end");
        };
        assert!(end.as_nanos() > at(50).as_nanos());
    }

    #[test]
    fn a_long_segment_ends_on_the_shorter_pause() {
        // 9 s of speech (282 frames), then 300 ms (10 frames) of silence ends it.
        let probs = seq(&[(0.9, 282), (0.0, 12)]);
        let events = run(GateParams::default(), &probs);
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[1].0,
            282 + 9,
            "10 silent frames are 320 ms >= 300 ms"
        );
    }

    #[test]
    fn a_segment_is_cut_at_the_maximum_length() {
        let params = GateParams {
            max_segment: Duration::from_secs(2),
            ..GateParams::default()
        };
        // Speech never stops: the first segment is cut at 2 s from its (padded) start.
        let probs = seq(&[(0.9, 200)]);
        let events = run(params, &probs);
        let ends: Vec<_> = events
            .iter()
            .filter_map(|(_, e)| match e {
                VadEvent::SpeechEnd {
                    forced, start, end, ..
                } => Some((*forced, *start, *end)),
                VadEvent::SpeechStart { .. } => None,
            })
            .collect();
        assert!(!ends.is_empty());
        assert!(ends[0].0, "marked as forced");
        let length = ends[0].2.saturating_since(ends[0].1);
        assert!(length <= Duration::from_millis(2000 + 32), "{length:?}");
        // The segment after the cut starts again.
        assert!(matches!(events[2].1, VadEvent::SpeechStart { .. }));
    }

    #[test]
    fn flush_closes_an_open_segment_and_reset_forgets_it() {
        let mut gate = SpeechGate::new(GateParams::default(), FRAME);
        for i in 0..20 {
            gate.push(0.9, at(i));
        }
        assert!(gate.is_speaking());
        let Some(VadEvent::SpeechEnd { end, forced, .. }) = gate.flush(at(21)) else {
            panic!("expected an end");
        };
        assert!(!forced);
        assert_eq!(end, at(21), "padding is capped at the end of the audio");
        assert!(!gate.is_speaking());
        for i in 0..20 {
            gate.push(0.9, at(i));
        }
        gate.reset();
        assert!(!gate.is_speaking());
        assert_eq!(gate.flush(at(30)), None);
    }

    #[test]
    fn durations_round_up_to_whole_frames() {
        assert_eq!(ms(5), 160);
        let params = GateParams {
            min_speech: Duration::from_millis(161),
            ..GateParams::default()
        };
        // 161 ms needs six 32 ms frames.
        let probs = seq(&[(0.9, 5), (0.0, 40)]);
        assert!(run(params.clone(), &probs).is_empty());
        let probs = seq(&[(0.9, 6), (0.0, 40)]);
        assert_eq!(run(params, &probs).len(), 2);
    }
}
