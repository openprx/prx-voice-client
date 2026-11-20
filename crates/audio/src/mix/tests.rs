//! Mixer tests on sine inputs: expected mix per mode, gains in dB, mode switches without a step, the jitter buffer, rate alignment and long-run drift.

use std::f32::consts::PI;

use prxvc_media::MonoTime;

use super::*;
use crate::playback_sink::PlaybackSink;

const FS: f32 = 48_000.0;
const EPS: f32 = 1e-4;

/// A sine stream cut into frames with exact timestamps, like a capture.
struct SineStream {
    rate: SampleRate,
    hz: f32,
    amp: f32,
    pos: usize,
    seq: u64,
}

impl SineStream {
    fn new(rate: u32, hz: f32, amp: f32) -> Self {
        Self {
            rate: SampleRate::new(rate).unwrap(),
            hz,
            amp,
            pos: 0,
            seq: 0,
        }
    }

    fn frame(&mut self, len: usize) -> AudioFrame {
        let r = self.rate.hz() as f32;
        let samples = (0..len)
            .map(|i| self.amp * (2.0 * PI * self.hz * (self.pos + i) as f32 / r).sin())
            .collect();
        let ts = MonoTime::ZERO.saturating_add(self.rate.duration_of(self.pos));
        self.pos += len;
        self.seq += 1;
        AudioFrame::new(samples, self.rate, ts, self.seq).unwrap()
    }

    /// Skip `len` samples without producing them (a lost span).
    fn skip(&mut self, len: usize) {
        self.pos += len;
    }

    fn samples_in(&self, ms: u64) -> usize {
        self.rate.samples_in(Duration::from_millis(ms))
    }
}

fn ideal(hz: f32, amp: f32, n: usize) -> f32 {
    amp * (2.0 * PI * hz * n as f32 / FS).sin()
}

fn render(mixer: &mut Mixer, n: usize) -> Vec<f32> {
    let mut out = vec![0.0; n];
    mixer.render(&mut out);
    out
}

fn max_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

/// The most a sine of this frequency and amplitude moves between samples.
fn tone_step(hz: f32, amp: f32) -> f32 {
    amp * 2.0 * (PI * hz / FS).sin()
}

fn config(mode: MixMode) -> MixerConfig {
    MixerConfig {
        mode,
        ..MixerConfig::default()
    }
}

fn push_all(mixer: &mut Mixer, translated: &mut SineStream, original: &mut SineStream, ms: u64) {
    mixer
        .push_translated(&translated.frame(translated.samples_in(ms)))
        .unwrap();
    mixer
        .push_original(&original.frame(original.samples_in(ms)))
        .unwrap();
}

#[test]
fn db_gains_convert_to_linear_factors() {
    assert!((db_to_linear(0.0) - 1.0).abs() < 1e-6);
    assert!((db_to_linear(-6.0206) - 0.5).abs() < 1e-4);
    assert!((db_to_linear(6.0206) - 2.0).abs() < 1e-3);
    assert!((db_to_linear(-20.0) - 0.1).abs() < 1e-6);
    assert!((db_to_linear(-12.0) - 0.251_188_64).abs() < 1e-6);
}

#[test]
fn each_mode_outputs_the_expected_mix() {
    let (t_amp, o_amp) = (0.4, 0.3);
    let cfg = MixerConfig {
        translated_gain_db: -6.0,
        original_gain_db: -12.0,
        passthrough_gain_db: -3.0,
        ..MixerConfig::default()
    };
    let expect = [
        (MixMode::Translation, db_to_linear(-6.0), 0.0),
        (MixMode::Both, db_to_linear(-6.0), db_to_linear(-12.0)),
        (MixMode::Original, 0.0, db_to_linear(-3.0)),
    ];
    for (mode, gt, go) in expect {
        let mut mixer = Mixer::new(MixerConfig { mode, ..cfg });
        let (mut t, mut o) = (
            SineStream::new(48_000, 440.0, t_amp),
            SineStream::new(48_000, 1_000.0, o_amp),
        );
        push_all(&mut mixer, &mut t, &mut o, 1_000);
        let out = render(&mut mixer, 24_000);
        // The lanes fade in over the first 5 ms.
        for (n, got) in out.iter().enumerate().skip(480) {
            let want = gt * ideal(440.0, t_amp, n) + go * ideal(1_000.0, o_amp, n);
            assert!(
                (got - want).abs() < EPS,
                "{mode:?} sample {n}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn gains_in_db_set_the_level_of_each_input() {
    let cases = [(-20.0, 0.1), (-6.0206, 0.5), (0.0, 1.0)];
    for (db, factor) in cases {
        let mut mixer = Mixer::new(MixerConfig {
            translated_gain_db: db,
            ..config(MixMode::Translation)
        });
        let mut t = SineStream::new(48_000, 440.0, 0.5);
        mixer.push_translated(&t.frame(48_000)).unwrap();
        let out = render(&mut mixer, 24_000);
        let peak = out[480..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5 * factor).abs() < 1e-3, "{db} dB: peak {peak}");
    }
    // Original gain applies in Both only; Original uses the passthrough gain.
    let mut mixer = Mixer::new(MixerConfig {
        original_gain_db: -20.0,
        passthrough_gain_db: -6.0206,
        ..config(MixMode::Both)
    });
    let mut o = SineStream::new(48_000, 440.0, 0.5);
    mixer.push_original(&o.frame(96_000)).unwrap();
    let both = render(&mut mixer, 24_000);
    mixer.set_mode(MixMode::Original);
    let alone = render(&mut mixer, 24_000);
    let peak = |s: &[f32]| s[2_000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!((peak(&both) - 0.05).abs() < 1e-3);
    assert!((peak(&alone) - 0.25).abs() < 1e-3);
}

#[test]
fn the_sum_is_clamped() {
    let mut mixer = Mixer::new(MixerConfig {
        original_gain_db: 0.0,
        ..config(MixMode::Both)
    });
    let (mut t, mut o) = (
        SineStream::new(48_000, 440.0, 0.9),
        SineStream::new(48_000, 440.0, 0.9),
    );
    push_all(&mut mixer, &mut t, &mut o, 500);
    let out = render(&mut mixer, 12_000);
    assert!(out.iter().all(|s| s.abs() <= 1.0));
    assert!(out.iter().any(|s| s.abs() == 1.0));
}

/// Windowed RMS (1 ms windows) of the lowest window in `samples`.
fn min_window_rms(samples: &[f32]) -> f32 {
    samples
        .windows(48)
        .step_by(48)
        .map(|w| (w.iter().map(|s| s * s).sum::<f32>() / 48.0).sqrt())
        .fold(f32::MAX, f32::min)
}

#[test]
fn a_mode_switch_never_exceeds_the_step_a_tone_allows_and_never_goes_silent() {
    let (t_hz, o_hz, t_amp, o_amp) = (300.0, 1_700.0, 0.4, 0.3);
    let modes = [MixMode::Translation, MixMode::Both, MixMode::Original];
    for from in modes {
        for to in modes.into_iter().filter(|m| *m != from) {
            let cfg = config(from);
            let mut mixer = Mixer::new(cfg);
            let (mut t, mut o) = (
                SineStream::new(48_000, t_hz, t_amp),
                SineStream::new(48_000, o_hz, o_amp),
            );
            push_all(&mut mixer, &mut t, &mut o, 1_000);
            let mut out = render(&mut mixer, 4_800);
            // Switch at a position that is not a block boundary.
            out.extend(render(&mut mixer, 777));
            let switch_at = out.len();
            mixer.set_mode(to);
            out.extend(render(&mut mixer, 9_600));

            let (gt_from, go_from) = gains_of(&cfg, from);
            let (gt_to, go_to) = gains_of(&cfg, to);
            // Output step <= tone step of each input at its larger gain, plus the
            // steepest gain slope (pi/2 over the crossfade) times the amplitude.
            let slope = (PI / 2.0) / 960.0;
            let bound = tone_step(t_hz, t_amp) * gt_from.max(gt_to)
                + tone_step(o_hz, o_amp) * go_from.max(go_to)
                + t_amp * (gt_to - gt_from).abs() * slope
                + o_amp * (go_to - go_from).abs() * slope
                + EPS;
            let steady = &out[480..switch_at];
            let after = &out[switch_at..];
            assert!(max_step(steady) <= bound, "{from:?}->{to:?} steady");
            assert!(
                max_step(&out[480..]) <= bound,
                "{from:?}->{to:?}: step {} > {bound}",
                max_step(&out[480..])
            );
            // The crossfade and what follows is never silent.
            assert!(
                min_window_rms(&after[..960]) > 0.04,
                "{from:?}->{to:?} silent window"
            );
        }
    }
}

#[test]
fn identical_inputs_crossfade_to_exactly_the_same_signal() {
    let cfg = MixerConfig {
        original_gain_db: 0.0,
        ..config(MixMode::Translation)
    };
    let mut mixer = Mixer::new(cfg);
    let (mut t, mut o) = (
        SineStream::new(48_000, 440.0, 0.5),
        SineStream::new(48_000, 440.0, 0.5),
    );
    push_all(&mut mixer, &mut t, &mut o, 1_000);
    let mut out = render(&mut mixer, 2_400);
    mixer.set_mode(MixMode::Original);
    out.extend(render(&mut mixer, 4_800));
    // The two gains sum to one at every sample, so the output is the tone.
    for (n, got) in out.iter().enumerate().skip(480) {
        assert!((got - ideal(440.0, 0.5, n)).abs() < EPS, "sample {n}");
    }
}

#[test]
fn a_switch_during_a_crossfade_continues_from_the_gains_reached() {
    let (hz, amp) = (500.0, 0.5);
    let mut mixer = Mixer::new(config(MixMode::Translation));
    let (mut t, mut o) = (
        SineStream::new(48_000, hz, amp),
        SineStream::new(48_000, 2_000.0, amp),
    );
    push_all(&mut mixer, &mut t, &mut o, 1_000);
    let mut out = render(&mut mixer, 2_400);
    mixer.set_mode(MixMode::Original);
    out.extend(render(&mut mixer, 300));
    mixer.set_mode(MixMode::Translation);
    out.extend(render(&mut mixer, 300));
    mixer.set_mode(MixMode::Both);
    out.extend(render(&mut mixer, 4_800));
    let slope = (PI / 2.0) / 960.0;
    let bound = tone_step(hz, amp) + tone_step(2_000.0, amp) + 2.0 * amp * slope + EPS;
    assert!(max_step(&out[480..]) <= bound);
}

/// Real-time pace in 10 ms ticks: every tick renders 10 ms, then `pushes(tick)`
/// chunks of 10 ms arrive.
fn run_paced(
    mixer: &mut Mixer,
    stream: &mut SineStream,
    ticks: usize,
    pushes: impl Fn(usize) -> usize,
) -> Vec<f32> {
    let chunk = stream.samples_in(10);
    let mut out = Vec::new();
    for tick in 0..ticks {
        out.extend(render(mixer, 480));
        for _ in 0..pushes(tick) {
            mixer.push_translated(&stream.frame(chunk)).unwrap();
        }
    }
    out
}

#[test]
fn the_jitter_buffer_absorbs_a_late_chunk_without_a_gap() {
    let mut mixer = Mixer::new(config(MixMode::Translation));
    let mut t = SineStream::new(48_000, 440.0, 0.5);
    // 150 ms prebuffer, then one chunk per tick; nothing arrives during ticks
    // 20..30 (100 ms late), then the backlog arrives at once.
    let chunk = t.samples_in(10);
    for _ in 0..15 {
        mixer.push_translated(&t.frame(chunk)).unwrap();
    }
    let out = run_paced(&mut mixer, &mut t, 60, |tick| match tick {
        20..30 => 0,
        30 => 11,
        _ => 1,
    });
    assert_eq!(mixer.stats().translated.underruns, 0);
    // The output is the unbroken tone from the end of the 5 ms fade-in.
    for (n, got) in out.iter().enumerate().skip(480) {
        assert!(
            (got - ideal(440.0, 0.5, n)).abs() < EPS,
            "sample {n}: gap or glitch"
        );
    }
}

#[test]
fn a_starved_jitter_buffer_reports_an_underrun_and_fades_out_without_a_step() {
    let mut mixer = Mixer::new(config(MixMode::Translation));
    let mut t = SineStream::new(48_000, 440.0, 0.5);
    mixer.push_translated(&t.frame(t.samples_in(300))).unwrap();
    let out = render(&mut mixer, 48_000);
    let stats = mixer.stats().translated;
    assert_eq!(stats.underruns, 1);
    // 300 ms of tone, then silence; the end is smooth.
    assert!(out[14_400..].iter().all(|s| *s == 0.0));
    assert!(max_step(&out) <= tone_step(440.0, 0.5) + EPS);
    // The last 5 ms are faded: the output before the end is below the tone.
    let tail_peak = out[14_400 - 10..14_400]
        .iter()
        .fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(tail_peak < 0.03, "tail peak {tail_peak}");
}

#[test]
fn playout_restarts_only_when_the_jitter_buffer_has_refilled() {
    let mut mixer = Mixer::new(config(MixMode::Translation));
    let mut t = SineStream::new(48_000, 440.0, 0.5);
    // Less than the 150 ms depth: nothing plays yet.
    mixer.push_translated(&t.frame(t.samples_in(100))).unwrap();
    assert!(render(&mut mixer, 2_400).iter().all(|s| *s == 0.0));
    mixer.push_translated(&t.frame(t.samples_in(60))).unwrap();
    let out = render(&mut mixer, 2_400);
    assert!(out.iter().any(|s| s.abs() > 0.1));
    assert!(max_step(&out) <= tone_step(440.0, 0.5) + EPS);
    // After an underrun the same rule applies again.
    render(&mut mixer, 12_000);
    assert_eq!(mixer.stats().translated.underruns, 1);
    mixer.push_translated(&t.frame(t.samples_in(100))).unwrap();
    assert!(render(&mut mixer, 2_400).iter().all(|s| *s == 0.0));
}

#[test]
fn an_announced_end_of_stream_plays_out_and_is_not_an_underrun() {
    let mut mixer = Mixer::new(config(MixMode::Translation));
    let mut t = SineStream::new(48_000, 440.0, 0.5);
    // Shorter than the jitter depth: plays anyway once the end is announced.
    mixer.push_translated(&t.frame(t.samples_in(60))).unwrap();
    mixer.end_translated().unwrap();
    let out = render(&mut mixer, 12_000);
    assert_eq!(mixer.stats().translated.underruns, 0);
    assert!(out[..2_880].iter().any(|s| s.abs() > 0.4));
    assert!(out[2_880..].iter().all(|s| *s == 0.0));
    assert!(max_step(&out) <= tone_step(440.0, 0.5) + EPS);
    // A second utterance behaves the same.
    mixer.push_translated(&t.frame(t.samples_in(60))).unwrap();
    mixer.end_translated().unwrap();
    render(&mut mixer, 12_000);
    assert_eq!(mixer.stats().translated.underruns, 0);
}

#[test]
fn flushing_fades_out_the_last_milliseconds() {
    let mut mixer = Mixer::new(config(MixMode::Translation));
    let mut t = SineStream::new(48_000, 440.0, 0.5);
    mixer
        .push_translated(&t.frame(t.samples_in(1_000)))
        .unwrap();
    let mut out = render(&mut mixer, 4_800);
    mixer.flush_translated();
    out.extend(render(&mut mixer, 4_800));
    assert!(out[4_800 + 240..].iter().all(|s| *s == 0.0));
    assert!(max_step(&out[480..]) <= tone_step(440.0, 0.5) + EPS);
    assert_eq!(mixer.stats().translated.underruns, 0);
    assert_eq!(mixer.stats().translated.queued_samples, 0);
}

/// Deterministic pseudo-random sizes (no extra dependency).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, lo: usize, hi: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        lo + ((self.0 >> 33) as usize) % (hi - lo + 1)
    }
}

/// Feed `stream` into one lane with random chunk sizes, always staying
/// `lead_ms` ahead of the output, and render `seconds` in 10 ms blocks.
fn long_run(rate: u32, to_original: bool, seconds: usize, seed: u64) -> Vec<f32> {
    let mode = if to_original {
        MixMode::Original
    } else {
        MixMode::Translation
    };
    let mut mixer = Mixer::new(config(mode));
    let mut stream = SineStream::new(rate, 440.0, 0.5);
    let mut rng = Lcg(seed);
    let (lo, hi) = (stream.samples_in(3), stream.samples_in(40));
    let mut out = Vec::new();
    while out.len() < seconds * 48_000 {
        // Keep 400 ms of input ahead of the output (the resampler holds back up to 64 ms).
        while stream.pos * 48_000 / rate as usize <= out.len() + 19_200 {
            let frame = stream.frame(rng.next(lo, hi));
            if to_original {
                mixer.push_original(&frame).unwrap();
            } else {
                mixer.push_translated(&frame).unwrap();
            }
        }
        out.extend(render(&mut mixer, 480));
    }
    let stats = mixer.stats();
    assert_eq!(
        (stats.translated.underruns, stats.original.underruns),
        (0, 0),
        "{rate} Hz starved"
    );
    out
}

#[test]
fn inputs_at_16_24_and_48_khz_come_out_aligned() {
    for rate in [16_000, 24_000, 48_000] {
        for to_original in [false, true] {
            let out = long_run(rate, to_original, 1, 7);
            // Sample n of the stream is output sample n: the resampler's delay is removed.
            let worst = out
                .iter()
                .enumerate()
                .skip(480)
                .map(|(n, got)| (got - ideal(440.0, 0.5, n)).abs())
                .fold(0.0f32, f32::max);
            assert!(
                worst < 5e-3,
                "{rate} Hz original={to_original}: worst error {worst}"
            );
        }
    }
}

#[test]
fn a_translated_and_an_original_input_at_different_rates_add_in_phase() {
    let mut mixer = Mixer::new(MixerConfig {
        original_gain_db: 0.0,
        ..config(MixMode::Both)
    });
    let (mut t, mut o) = (
        SineStream::new(16_000, 440.0, 0.25),
        SineStream::new(24_000, 440.0, 0.25),
    );
    push_all(&mut mixer, &mut t, &mut o, 1_000);
    let out = render(&mut mixer, 24_000);
    let worst = out
        .iter()
        .enumerate()
        .skip(480)
        .map(|(n, got)| (got - ideal(440.0, 0.5, n)).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 5e-3, "worst error {worst}");
}

#[test]
fn a_long_run_with_random_chunk_sizes_does_not_drift() {
    // A one-sample drift at 440 Hz is 0.058 rad of phase, over 10 times the bound.
    for (rate, seed) in [(16_000, 11), (24_000, 12), (48_000, 13)] {
        let out = long_run(rate, false, 12, seed);
        let window = 48_000;
        let errors: Vec<f32> = out
            .chunks_exact(window)
            .enumerate()
            .map(|(w, chunk)| {
                chunk
                    .iter()
                    .enumerate()
                    .map(|(i, got)| (got - ideal(440.0, 0.5, w * window + i)).abs())
                    .fold(0.0f32, f32::max)
            })
            .collect();
        for (w, e) in errors.iter().enumerate().skip(1) {
            assert!(*e < 5e-3, "{rate} Hz window {w}: error {e}");
        }
    }
}

#[test]
fn a_lost_span_of_the_original_becomes_silence_of_the_right_length() {
    let mut mixer = Mixer::new(config(MixMode::Original));
    let mut o = SineStream::new(48_000, 440.0, 0.5);
    mixer.push_original(&o.frame(4_800)).unwrap();
    // 100 ms of the capture never arrives.
    o.skip(4_800);
    mixer.push_original(&o.frame(9_600)).unwrap();
    let out = render(&mut mixer, 19_200);
    let stats = mixer.stats().original;
    assert_eq!(stats.filled_samples, 4_800);
    // The tone after the hole is in phase with the ideal timeline.
    // (The last 5 ms are the fade of the queue running dry.)
    for (n, got) in out.iter().enumerate().take(18_900).skip(9_600 + 480) {
        assert!((got - ideal(440.0, 0.5, n)).abs() < 1e-3, "sample {n}");
    }
    assert!(out[4_800..9_600].iter().all(|s| s.abs() < 1e-6));
}

#[test]
fn a_repeated_frame_is_trimmed_and_a_huge_gap_restarts() {
    let mut mixer = Mixer::new(config(MixMode::Original));
    let mut o = SineStream::new(48_000, 440.0, 0.5);
    let first = o.frame(4_800);
    mixer.push_original(&first).unwrap();
    mixer.push_original(&first).unwrap();
    assert_eq!(mixer.stats().original.trimmed_samples, 4_800);
    assert_eq!(mixer.stats().original.queued_samples, 4_800);
    o.skip(48_000 * 10);
    mixer.push_original(&o.frame(480)).unwrap();
    assert_eq!(mixer.stats().original.filled_samples, 0);
    assert_eq!(mixer.stats().original.queued_samples, 4_800 + 480);
}

#[test]
fn a_full_queue_drops_the_excess_and_counts_it() {
    let mut mixer = Mixer::new(MixerConfig {
        max_buffer: Duration::from_secs(1),
        ..config(MixMode::Translation)
    });
    let mut t = SineStream::new(48_000, 440.0, 0.5);
    mixer.push_translated(&t.frame(72_000)).unwrap();
    let stats = mixer.stats().translated;
    assert_eq!(
        (stats.queued_samples, stats.dropped_samples),
        (48_000, 24_000)
    );
}

#[test]
fn a_change_of_input_rate_starts_a_new_stream() {
    let mut mixer = Mixer::new(config(MixMode::Translation));
    let mut a = SineStream::new(16_000, 440.0, 0.5);
    let mut b = SineStream::new(24_000, 440.0, 0.5);
    mixer.push_translated(&a.frame(16_000)).unwrap();
    mixer.push_translated(&b.frame(24_000)).unwrap();
    mixer.end_translated().unwrap();
    // 1 s at each rate: the first stream's tail was flushed at the change, the second at the end.
    assert_eq!(mixer.stats().translated.queued_samples, 96_000);
}

#[test]
fn feed_fills_a_sink_to_the_requested_depth_with_the_rendered_audio() {
    let mut mixer = Mixer::new(config(MixMode::Translation));
    let mut t = SineStream::new(24_000, 440.0, 0.5);
    mixer.push_translated(&t.frame(24_000)).unwrap();
    let mut sink = PlaybackSink::detached(48_000, 1);
    let written = mixer.feed(&mut sink, Duration::from_millis(200)).unwrap();
    assert_eq!(written, 9_600);
    assert_eq!(sink.queued(), Duration::from_millis(200));
    // Nothing more is rendered while the sink holds enough.
    assert_eq!(
        mixer.feed(&mut sink, Duration::from_millis(200)).unwrap(),
        0
    );
    let mut played = vec![0.0; 9_600];
    sink.buffer().pull(&mut played);
    for (n, got) in played.iter().enumerate().skip(480) {
        assert!((got - ideal(440.0, 0.5, n)).abs() < 5e-3, "sample {n}");
    }
    assert_eq!(mixer.output_time(), MonoTime::from_millis(200));
    assert_eq!(mixer.stats().rendered_samples, 9_600);
}
