//! Pitch-preserving time-scale modification (WSOLA) for speech, as a pure streaming processor.
//!
//! Waveform-similarity overlap-add: the input is cut into windowed frames of about 22 ms that are
//! laid on the output at a fixed hop (half a frame), but taken from the input at a hop `speed`
//! times larger. Each frame's position is moved within a small search range to the offset where it
//! best continues the previous frame (normalised cross-correlation of the overlap), so voiced
//! speech keeps its pitch period and joins without a click. At `speed == 1.0` the continuation is
//! exact and the output is the input, delayed by one frame.
//!
//! The processor never allocates after it was built beyond the growth of its two buffers, which are
//! bounded by a frame plus the search range plus one pushed block; it has no clock, no lock and no
//! logging, so it is safe to run where the mixer renders. Speed may change from one call to the
//! next (the analysis hop follows it); it is clamped to `1.0..=MAX_SPEED`.
//!
//! Streaming use: [`TimeScaler::process`] with each block of input, then [`TimeScaler::finish`] at
//! the end of the stream (the remainder is padded with silence and the last frame fades out).

/// The largest speed-up the processor accepts.
pub const MAX_SPEED: f32 = 2.0;

/// Window length in seconds.
const WINDOW_S: f32 = 0.022;
/// Search range on each side of the nominal position, in seconds.
const SEARCH_S: f32 = 0.006;

/// Candidates skipped by the coarse pass of the search.
const COARSE_STEP: usize = 4;
/// Samples skipped when the overlap is compared.
const SCORE_STRIDE: usize = 2;

/// Streaming WSOLA at one sample rate.
pub struct TimeScaler {
    win: usize,
    hop: usize,
    delta: usize,
    rise: Vec<f32>,
    fall: Vec<f32>,
    /// Input not yet dropped; `buf[0]` is input sample `origin`.
    buf: Vec<f32>,
    origin: usize,
    /// Nominal start of the next frame (input coordinates); `None` before the first frame.
    nominal: f64,
    /// Start of the previous frame; `None` before the first frame.
    prev: Option<usize>,
    /// The falling half of the previous windowed frame, still to be added to the next one.
    acc: Vec<f32>,
}

impl TimeScaler {
    /// A processor for audio at `rate` Hz.
    pub fn new(rate: u32) -> Self {
        let r = rate as f32;
        let win = ((r * WINDOW_S) as usize).max(16) & !1;
        let hop = win / 2;
        let delta = ((r * SEARCH_S) as usize).max(1);
        // Periodic Hann halves: rise[i] + fall[i] == 1.
        let rise: Vec<f32> = (0..hop)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::PI * i as f32 / hop as f32).cos())
            .collect();
        let fall: Vec<f32> = rise.iter().map(|w| 1.0 - w).collect();
        Self {
            win,
            hop,
            delta,
            rise,
            fall,
            buf: Vec::with_capacity(win * 4 + 4096),
            origin: 0,
            nominal: 0.0,
            prev: None,
            acc: vec![0.0; hop],
        }
    }

    /// Samples the processor holds back before it can emit (one frame plus the search range).
    pub fn latency(&self) -> usize {
        self.win + self.delta
    }

    /// Forget everything: the next input starts a new stream.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.origin = 0;
        self.nominal = 0.0;
        self.prev = None;
        self.acc.fill(0.0);
    }

    /// Whether anything of a stream is held (a frame still to be completed).
    pub fn is_empty(&self) -> bool {
        self.prev.is_none() && self.buf.is_empty()
    }

    /// Take `input`, append what can be produced at `speed` to `out`.
    pub fn process(&mut self, input: &[f32], speed: f32, out: &mut Vec<f32>) {
        self.buf.extend_from_slice(input);
        self.run(speed.clamp(1.0, MAX_SPEED), false, out);
    }

    /// End of the stream: produce the rest (silence pads the last frame) and reset.
    pub fn finish(&mut self, speed: f32, out: &mut Vec<f32>) {
        if self.buf.is_empty() && self.prev.is_none() {
            return;
        }
        self.run(speed.clamp(1.0, MAX_SPEED), true, out);
        if self.prev.is_some() {
            // The falling half of the last frame ends the stream.
            out.extend_from_slice(&self.acc);
        }
        self.reset();
    }

    fn sample(&self, abs: usize) -> f32 {
        self.buf.get(abs.wrapping_sub(self.origin)).copied().unwrap_or(0.0)
    }

    fn input_end(&self) -> usize {
        self.origin + self.buf.len()
    }

    fn run(&mut self, speed: f32, finishing: bool, out: &mut Vec<f32>) {
        let (win, hop, delta) = (self.win, self.hop, self.delta);
        loop {
            let Some(prev) = self.prev else {
                // The first frame: its first half is output as it is, its second half waits.
                if self.buf.len() < win && !finishing {
                    break;
                }
                if self.buf.is_empty() {
                    break;
                }
                for i in 0..hop {
                    out.push(self.sample(i));
                }
                for i in 0..hop {
                    self.acc[i] = self.sample(hop + i) * self.fall[i];
                }
                self.prev = Some(0);
                self.nominal = f64::from(speed) * hop as f64;
                continue;
            };
            let nominal = self.nominal.round() as usize;
            let lo = nominal.saturating_sub(delta).max(self.origin);
            let hi = nominal + delta;
            if !finishing && hi + win > self.input_end() {
                break;
            }
            if finishing && nominal >= self.input_end() {
                break;
            }
            let start = self.best_start(prev, lo, hi);
            for i in 0..hop {
                out.push(self.acc[i] + self.rise[i] * self.sample(start + i));
            }
            for i in 0..hop {
                self.acc[i] = self.fall[i] * self.sample(start + hop + i);
            }
            self.prev = Some(start);
            self.nominal += f64::from(speed) * hop as f64;
            self.drop_consumed();
        }
    }

    /// The start in `lo..=hi` whose first `hop` samples best continue the previous frame.
    ///
    /// The search is coarse to fine so that the cost stays far below the period of a sink: every
    /// second sample of the overlap is compared, at every `COARSE_STEP`th candidate first, then at
    /// every candidate within `COARSE_STEP - 1` of the best one. Speech is smooth at this rate, so
    /// the joins are as good as with the full search (see the tests) at an eighth of the work.
    fn best_start(&self, prev: usize, lo: usize, hi: usize) -> usize {
        let template_at = prev + self.hop;
        let score = |c: usize| -> f32 {
            let (mut dot, mut norm) = (0.0f32, 1e-9f32);
            let mut i = 0;
            while i < self.hop {
                let x = self.sample(c + i);
                dot += self.sample(template_at + i) * x;
                norm += x * x;
                i += SCORE_STRIDE;
            }
            dot / norm.sqrt()
        };
        let mut best = (f32::MIN, lo.max(template_at.min(hi)));
        let mut c = lo;
        while c <= hi {
            let s = score(c);
            if s > best.0 {
                best = (s, c);
            }
            c += COARSE_STEP;
        }
        let (from, to) = (best.1.saturating_sub(COARSE_STEP - 1).max(lo), (best.1 + COARSE_STEP - 1).min(hi));
        for c in from..=to {
            let s = score(c);
            if s > best.0 {
                best = (s, c);
            }
        }
        best.1
    }

    /// Drop input nobody will read again.
    fn drop_consumed(&mut self) {
        let keep_from = self
            .prev
            .map_or(self.origin, |p| p + self.hop)
            .min((self.nominal.round() as usize).saturating_sub(self.delta));
        if keep_from > self.origin + 2048 {
            // While a stream is finishing, the frame position can already lie past the end of the
            // input (the rest is read as silence): never cut more than is held.
            let cut = (keep_from - self.origin).min(self.buf.len());
            self.buf.drain(..cut);
            self.origin += cut;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(hz: f32, secs: f32) -> Vec<f32> {
        (0..(RATE as f32 * secs) as usize)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / RATE as f32).sin())
            .collect()
    }

    /// A voiced-speech-like signal: a 120 Hz pulse train shaped by three formants and a slow envelope.
    fn voiced(secs: f32) -> Vec<f32> {
        (0..(RATE as f32 * secs) as usize)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let env = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 3.0 * t).sin().abs();
                let tone: f32 = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0]
                    .iter()
                    .map(|h| (2.0 * std::f32::consts::PI * 120.0 * h * t).sin() / h)
                    .sum();
                0.3 * env * tone
            })
            .collect()
    }

    fn run(input: &[f32], speed: f32, chunk: usize) -> Vec<f32> {
        let mut s = TimeScaler::new(RATE);
        let mut out = Vec::new();
        for c in input.chunks(chunk) {
            s.process(c, speed, &mut out);
        }
        s.finish(speed, &mut out);
        out
    }

    fn crossings_per_second(x: &[f32]) -> f32 {
        let n = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        n as f32 / (x.len() as f32 / RATE as f32)
    }

    fn max_step(x: &[f32]) -> f32 {
        x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn at_speed_one_the_output_is_the_input() {
        let input = voiced(0.6);
        let out = run(&input, 1.0, 480);
        assert!(out.len().abs_diff(input.len()) <= 1_100, "{} vs {}", out.len(), input.len());
        for (i, (a, b)) in input.iter().zip(&out).enumerate().take(input.len() - 1_100) {
            assert!((a - b).abs() < 1e-4, "sample {i}: {a} vs {b}");
        }
    }

    #[test]
    fn a_speed_up_shortens_the_audio_by_the_factor_and_keeps_the_pitch() {
        for speed in [1.1f32, 1.25, 1.5] {
            let input = sine(440.0, 2.0);
            let out = run(&input, speed, 960);
            let expected = input.len() as f32 / speed;
            assert!(
                (out.len() as f32 - expected).abs() < 2_500.0,
                "speed {speed}: {} samples, expected about {expected}",
                out.len()
            );
            let hz = crossings_per_second(&out[2_000..out.len() - 2_000]);
            assert!((hz - 440.0).abs() < 8.0, "speed {speed}: pitch {hz} Hz");
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!((0.45..=0.55).contains(&peak), "amplitude kept: {peak}");
        }
    }

    #[test]
    fn the_join_is_smooth_for_a_voiced_signal() {
        let input = voiced(2.0);
        let out = run(&input, 1.25, 700);
        let limit = max_step(&input) * 1.6 + 0.02;
        assert!(max_step(&out) < limit, "step {} against the input's {}", max_step(&out), max_step(&input));
    }

    #[test]
    fn the_result_does_not_depend_on_how_the_input_is_chunked() {
        let input = voiced(1.0);
        let a = run(&input, 1.25, 480);
        let b = run(&input, 1.25, 7_000);
        let c = run(&input, 1.25, 33);
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn changing_the_speed_in_the_middle_stays_smooth() {
        let input = voiced(2.0);
        let mut s = TimeScaler::new(RATE);
        let mut out = Vec::new();
        for (i, c) in input.chunks(960).enumerate() {
            let speed = 1.0 + 0.25 * ((i / 20) % 2) as f32;
            s.process(c, speed, &mut out);
        }
        s.finish(1.0, &mut out);
        assert!(max_step(&out) < max_step(&input) * 1.6 + 0.02);
        assert!(out.len() < input.len());
    }

    #[test]
    fn silence_stays_silent_and_a_short_stream_is_flushed() {
        let out = run(&vec![0.0; 24_000], 1.25, 480);
        assert!(out.iter().all(|s| s.abs() < 1e-6));
        let short = run(&sine(300.0, 0.01), 1.25, 100);
        assert!(!short.is_empty(), "a stream shorter than a frame still comes out");
        assert!(short.iter().any(|s| s.abs() > 0.1));
        let mut s = TimeScaler::new(RATE);
        let mut out = Vec::new();
        s.finish(1.25, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn reset_starts_a_new_stream_and_the_speed_is_clamped() {
        let mut s = TimeScaler::new(RATE);
        let mut out = Vec::new();
        s.process(&sine(440.0, 0.2), 9.0, &mut out);
        assert!(!s.is_empty());
        s.reset();
        assert!(s.is_empty());
        let a = run(&sine(440.0, 0.5), 5.0, 480);
        let b = run(&sine(440.0, 0.5), MAX_SPEED, 480);
        assert_eq!(a, b);
        let slow = run(&sine(440.0, 0.5), 0.5, 480);
        let one = run(&sine(440.0, 0.5), 1.0, 480);
        assert_eq!(slow, one, "slowing down is not offered");
    }

    #[test]
    fn the_buffers_stay_bounded_on_a_long_stream() {
        let mut s = TimeScaler::new(RATE);
        let mut out = Vec::new();
        let block = sine(200.0, 0.01);
        for _ in 0..3_000 {
            out.clear();
            s.process(&block, 1.25, &mut out);
            assert!(s.buf.len() < 20_000, "{}", s.buf.len());
        }
    }

    /// Small deterministic generator for the randomised tests.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> usize {
            (self.next() % n) as usize
        }
    }

    #[test]
    fn no_stream_length_chunking_or_speed_makes_finish_panic() {
        // A stream that ends while the frame position is near the end of a buffer that is still
        // longer than the 2048 samples kept back used to cut past the end of the buffer.
        let mut rng = Lcg(7);
        for round in 0..60 {
            let mut s = TimeScaler::new(RATE);
            let mut out = Vec::new();
            let total = rng.below(30_000);
            let speed = 1.0 + rng.below(1_000) as f32 / 1_000.0;
            let mut left = total;
            while left > 0 {
                let n = (1 + rng.below(9_000)).min(left);
                let block: Vec<f32> = (0..n).map(|i| 0.4 * ((i + round) as f32 * 0.07).sin()).collect();
                s.process(&block, speed, &mut out);
                left -= n;
            }
            s.finish(speed, &mut out);
            assert!(s.is_empty());
            assert!(out.len() <= total + 2_200, "round {round}: {} out of {total}", out.len());
        }
    }

    /// Time the processor takes for the block sizes a pump hands it. Run in release:
    /// `cargo test -p prxvc-audio --release --lib timescale::tests::iteration_time -- --ignored --nocapture`
    #[test]
    #[ignore = "a timing measurement for release builds"]
    fn iteration_time() {
        for (label, ms) in [("one 10 ms block", 10usize), ("a 100 ms chunk", 100), ("a 1 s chunk", 1000), ("a 4 s chunk", 4000)] {
            let input = voiced(ms as f32 / 1000.0);
            let mut worst = std::time::Duration::ZERO;
            let mut total = std::time::Duration::ZERO;
            let runs = 20;
            for _ in 0..runs {
                let mut s = TimeScaler::new(RATE);
                let mut out = Vec::new();
                let t = std::time::Instant::now();
                s.process(&input, 1.25, &mut out);
                s.finish(1.25, &mut out);
                let d = t.elapsed();
                worst = worst.max(d);
                total += d;
            }
            println!("{label}: mean {:?}, worst {:?}", total / runs, worst);
        }
    }
}
