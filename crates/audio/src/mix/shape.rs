//! What the translated lane does to the speech it queues so that a backlog is caught up without dropping anything: silence is trimmed from both ends of a clause, and while a backlog exists the clause is played faster with its pitch kept.
//!
//! * **Trim**: a synthesised clause starts and ends with silence (the synthesiser's lead-in and
//!   fade-out), which adds up to a pause of several hundred milliseconds between two clauses.
//!   The silence at the start of a clause is cut to `lead_keep`, the silence at its end to
//!   `trail_keep`; while a backlog exists ([`Pace::tight`]) both are cut to a third of that.
//! * **Speed**: with [`Pace::speed`] above 1 the queued speech goes through
//!   [`crate::timescale::TimeScaler`] (WSOLA) before it is queued.
//!
//! Both are decided by the pipeline, which knows the backlog, and applied per clause as it is queued.

/// How the lane shapes a clause. Silence below `threshold` (linear amplitude) counts as silence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeConfig {
    /// Silence kept at the start of a clause, in samples.
    pub lead_keep: usize,
    /// Silence kept at the end of a clause, in samples.
    pub trail_keep: usize,
    /// Peak amplitude below which a sample is silence.
    pub threshold: f32,
    /// The fastest the lane plays.
    pub max_speed: f32,
}

/// What the pipeline asks of the lane for the clauses it queues from now on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pace {
    /// Playback speed; 1.0 is normal.
    pub speed: f32,
    /// A backlog exists: pauses are cut short.
    pub tight: bool,
}

impl Default for Pace {
    fn default() -> Self {
        Self {
            speed: 1.0,
            tight: false,
        }
    }
}

/// Samples of silence at the start of `samples`.
pub fn leading_silence(samples: &[f32], threshold: f32) -> usize {
    samples
        .iter()
        .position(|s| s.abs() >= threshold)
        .unwrap_or(samples.len())
}

/// Samples of silence at the end of `samples`.
pub fn trailing_silence(samples: &[f32], threshold: f32) -> usize {
    samples
        .iter()
        .rev()
        .position(|s| s.abs() >= threshold)
        .unwrap_or(samples.len())
}

impl ShapeConfig {
    /// The silence kept at the start under `pace`.
    pub fn lead(&self, pace: Pace) -> usize {
        if pace.tight {
            self.lead_keep / 3
        } else {
            self.lead_keep
        }
    }

    /// The silence kept at the end under `pace`.
    pub fn trail(&self, pace: Pace) -> usize {
        if pace.tight {
            self.trail_keep / 3
        } else {
            self.trail_keep
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_measured_from_both_ends() {
        let x = [0.0, 0.001, 0.0, 0.3, -0.5, 0.0, 0.002, 0.0];
        assert_eq!(leading_silence(&x, 0.01), 3);
        assert_eq!(trailing_silence(&x, 0.01), 3);
        assert_eq!(leading_silence(&[0.0; 5], 0.01), 5);
        assert_eq!(trailing_silence(&[0.0; 5], 0.01), 5);
        assert_eq!(leading_silence(&[], 0.01), 0);
    }

    #[test]
    fn a_backlog_cuts_the_kept_silence_to_a_third() {
        let c = ShapeConfig {
            lead_keep: 3_000,
            trail_keep: 6_000,
            threshold: 0.01,
            max_speed: 1.25,
        };
        let tight = Pace {
            speed: 1.0,
            tight: true,
        };
        assert_eq!(
            (c.lead(Pace::default()), c.trail(Pace::default())),
            (3_000, 6_000)
        );
        assert_eq!((c.lead(tight), c.trail(tight)), (1_000, 2_000));
    }
}
