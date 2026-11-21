//! The adaptive pre-roll of the translated lane: how much speech must be queued before a clause starts playing so that it can finish without a gap.
//!
//! Synthesis streams a clause in chunks. If it produces audio `p` times as fast as it is played
//! (`p` is the real-time factor of the producer seen from the listener: audio seconds per second
//! of waiting), a clause of length `T` that starts with `B` seconds queued runs dry unless
//! `B >= T * (1 - p)`. The model learns `p` and the typical `T` from the streams it has seen:
//!
//! * `p` is measured on each stream as the audio that arrived after its first chunk divided by
//!   the time between its first and last chunk; a stream that arrived in one burst (a clause that
//!   was synthesised while another played) says nothing and is ignored;
//! * `p` falls fast (a slower producer is believed at once) and recovers slowly;
//! * `T` is a moving average of the lengths of finished streams.
//!
//! * The longest wait between two chunks of a stream (a stall of the producer, such as a busy GPU
//!   or the translation of the next piece) rises the stall estimate at once and decays by a third
//!   with each clause. A clause that starts with the stall estimate queued rides out a stall of
//!   that length even when the producer is fast on average.
//!
//! The pre-roll is at least the fixed jitter buffer, grows when `p` (with a safety margin) is below
//! 1 or when the producer stalls, and never exceeds `max_depth` (a stall is covered only up to
//! `max_stall`): when even that cannot prevent a gap, the lane fades out and in instead.

/// Settings of the model, in output samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrerollConfig {
    /// The most pre-roll ever asked for.
    pub max_depth: usize,
    /// Margin on the measured rate: the producer is assumed to be this many times slower.
    pub safety: f32,
    /// The length of a clause before any has been seen.
    pub default_len: usize,
    /// Shortest span between a stream's first and last chunk that tells anything about the rate.
    pub min_span: u64,
    /// The longest stall the pre-roll covers.
    pub max_stall: usize,
}

/// What the lane has learned about its producer.
#[derive(Debug, Clone)]
pub struct PrerollModel {
    cfg: PrerollConfig,
    base: usize,
    rate: Option<f32>,
    len: Option<f32>,
    stall: f32,
}

const RATE_FLOOR: f32 = 0.2;
const RATE_CEIL: f32 = 8.0;

impl PrerollModel {
    /// A model that asks for `base` until it has seen a slow producer.
    pub fn new(cfg: PrerollConfig, base: usize) -> Self {
        Self {
            cfg,
            base,
            rate: None,
            len: None,
            stall: 0.0,
        }
    }

    /// One finished stream: `total` samples arrived, `after_first` of them after the first chunk,
    /// over `span` samples of the lane's clock, in `pushes` chunks, the longest wait between two
    /// of them being `longest_wait` samples.
    pub fn observe(
        &mut self,
        total: u64,
        after_first: u64,
        span: u64,
        pushes: u32,
        longest_wait: u64,
    ) {
        self.stall = (self.stall * 0.67).max(longest_wait as f32);
        if total > 0 {
            let t = total as f32;
            self.len = Some(self.len.map_or(t, |l| 0.7 * l + 0.3 * t));
        }
        if pushes < 3 || span < self.cfg.min_span || after_first == 0 {
            return;
        }
        let p = (after_first as f32 / span as f32).clamp(RATE_FLOOR, RATE_CEIL);
        self.rate = Some(match self.rate {
            None => p,
            Some(r) if p < r => 0.5 * r + 0.5 * p,
            Some(r) => 0.8 * r + 0.2 * p,
        });
    }

    /// The measured real-time factor of the producer, if any.
    pub fn rate(&self) -> Option<f32> {
        self.rate
    }

    /// Pre-roll for a clause that starts now.
    pub fn depth_for_new_stream(&self) -> usize {
        self.need(self.len.unwrap_or(self.cfg.default_len as f32))
    }

    /// Pre-roll for resuming after a gap, `played` samples into the stream: what is still to come
    /// is the typical length less what was played, and never less than a quarter of it.
    pub fn depth_to_resume(&self, played: u64) -> usize {
        let typical = self.len.unwrap_or(self.cfg.default_len as f32);
        self.need((typical - played as f32).max(typical * 0.25))
    }

    fn need(&self, remaining: f32) -> usize {
        let cap = self.cfg.max_depth.max(self.base);
        let stall = (self.stall as usize).min(self.cfg.max_stall);
        let slow = self.rate.map_or(0, |rate| {
            let p = rate / self.cfg.safety.max(1.0);
            if p >= 1.0 {
                0
            } else {
                ((1.0 - p) * remaining) as usize
            }
        });
        slow.max(stall).max(self.base).min(cap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: PrerollConfig = PrerollConfig {
        max_depth: 96_000,
        safety: 1.2,
        default_len: 96_000,
        min_span: 4_800,
        max_stall: 28_800,
    };

    #[test]
    fn a_fast_producer_keeps_the_fixed_depth() {
        let mut m = PrerollModel::new(CFG, 7_200);
        assert_eq!(m.depth_for_new_stream(), 7_200, "nothing learned yet");
        // 3 s of audio arrived in 1 s of lane time after the first chunk.
        m.observe(144_000, 120_000, 48_000, 20, 1_000);
        assert!(m.rate().unwrap() > 2.0);
        assert_eq!(m.depth_for_new_stream(), 7_200);
    }

    #[test]
    fn a_slow_producer_asks_for_what_the_clause_needs() {
        let mut m = PrerollModel::new(CFG, 7_200);
        // 2 s of audio over 2.5 s: p = 0.8; with the margin 0.667; typical clause 2 s.
        m.observe(96_000, 80_000, 100_000, 10, 1_000);
        let d = m.depth_for_new_stream();
        let expected = ((1.0 - 0.8 / 1.2) * 96_000.0) as usize;
        assert!(d.abs_diff(expected) <= 1, "{d} vs {expected}");
        assert!(d > 7_200);
    }

    #[test]
    fn the_depth_is_capped_and_a_burst_teaches_nothing() {
        let mut m = PrerollModel::new(CFG, 7_200);
        m.observe(960_000, 900_000, 4_000_000, 50, 1_000); // p = 0.225, a 20 s clause
        assert_eq!(m.depth_for_new_stream(), CFG.max_depth);
        let mut b = PrerollModel::new(CFG, 7_200);
        b.observe(96_000, 48_000, 0, 2, 0); // arrived in one burst
        assert_eq!(b.rate(), None);
        assert_eq!(b.depth_for_new_stream(), 7_200);
    }

    #[test]
    fn a_slowdown_is_believed_at_once_and_a_recovery_slowly() {
        let mut m = PrerollModel::new(CFG, 7_200);
        m.observe(96_000, 90_000, 30_000, 10, 1_000); // p = 3
        m.observe(96_000, 40_000, 80_000, 10, 1_000); // p = 0.5
        let after_slow = m.rate().unwrap();
        assert!((after_slow - 1.75).abs() < 0.01, "{after_slow}");
        m.observe(96_000, 90_000, 30_000, 10, 1_000); // p = 3 again
        let after_fast = m.rate().unwrap();
        assert!(after_fast < 2.1, "recovery is slow: {after_fast}");
    }

    #[test]
    fn a_stall_of_a_fast_producer_raises_the_depth_and_decays_with_the_next_clauses() {
        let mut m = PrerollModel::new(CFG, 7_200);
        m.observe(96_000, 90_000, 30_000, 10, 1_000);
        assert_eq!(m.depth_for_new_stream(), 7_200, "short waits are covered by the base");
        m.observe(96_000, 90_000, 30_000, 10, 20_000); // one wait of 420 ms
        assert_eq!(m.depth_for_new_stream(), 20_000);
        m.observe(96_000, 90_000, 30_000, 10, 1_000);
        assert_eq!(m.depth_for_new_stream(), 13_400);
        for _ in 0..6 {
            m.observe(96_000, 90_000, 30_000, 10, 1_000);
        }
        assert_eq!(m.depth_for_new_stream(), 7_200);
        // A very long stall is covered only up to the cap.
        m.observe(96_000, 90_000, 30_000, 10, 90_000);
        assert_eq!(m.depth_for_new_stream(), CFG.max_stall);
    }

    #[test]
    fn resuming_asks_for_less_the_further_the_stream_is() {
        let mut m = PrerollModel::new(CFG, 7_200);
        m.observe(96_000, 80_000, 100_000, 10, 1_000);
        let early = m.depth_to_resume(0);
        let late = m.depth_to_resume(80_000);
        assert!(late < early);
        assert!(late >= 7_200);
    }
}
