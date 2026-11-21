//! Gaps of the translated lane: what counts as one, how long it is, the fade around it and the adaptive pre-roll.

use std::time::Duration;

use prxvc_media::{AudioFrame, MonoTime, SampleRate};

use super::*;

const RATE: SampleRate = SampleRate::HZ_48000;

fn ms(n: u64) -> usize {
    RATE.samples_in(Duration::from_millis(n))
}

thread_local! {
    static PHASE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A continuous 440 Hz tone across calls (the phase carries over).
fn tone(len: usize) -> AudioFrame {
    let start = PHASE.with(|p| p.replace(p.get() + len));
    let samples = (0..len)
        .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * (start + i) as f32 / 48_000.0).sin())
        .collect();
    AudioFrame::new(samples, RATE, MonoTime::ZERO, 0).unwrap()
}

fn mixer(adaptive: bool) -> Mixer {
    Mixer::new(MixerConfig {
        mode: MixMode::Translation,
        adaptive_preroll: adaptive,
        ..MixerConfig::default()
    })
}

fn render(m: &mut Mixer, n: usize) -> Vec<f32> {
    let mut out = vec![0.0; n];
    m.render(&mut out);
    out
}

#[test]
fn a_gap_is_counted_and_measured_from_running_dry_to_resuming() {
    let mut m = mixer(false);
    m.push_translated(&tone(ms(300))).unwrap();
    render(&mut m, ms(300) + ms(500)); // 500 ms of silence after the audio ran out
    m.push_translated(&tone(ms(200))).unwrap();
    render(&mut m, ms(100));
    let s = m.stats().translated;
    assert_eq!(s.underruns, 1);
    assert!(
        s.gap_samples.abs_diff(ms(500) as u64) <= 2,
        "gap {} samples",
        s.gap_samples
    );
    assert_eq!(s.worst_gap_samples, s.gap_samples);
}

#[test]
fn the_worst_gap_is_the_longest_one() {
    let mut m = mixer(false);
    for silence in [200, 700, 300] {
        m.push_translated(&tone(ms(200))).unwrap();
        render(&mut m, ms(200 + silence));
    }
    m.push_translated(&tone(ms(200))).unwrap();
    render(&mut m, ms(50));
    let s = m.stats().translated;
    assert_eq!(s.underruns, 3);
    assert!(s.worst_gap_samples.abs_diff(ms(700) as u64) <= 2);
    assert!(s.gap_samples.abs_diff(ms(1200) as u64) <= 6);
}

#[test]
fn an_end_that_arrives_after_the_queue_emptied_is_a_normal_end() {
    let mut m = mixer(false);
    m.push_translated(&tone(ms(300))).unwrap();
    render(&mut m, ms(450)); // dry for 150 ms, the end not yet known
    assert_eq!(m.stats().translated.underruns, 1, "provisionally a gap");
    m.end_translated().unwrap();
    let s = m.stats().translated;
    assert_eq!((s.underruns, s.gap_samples, s.worst_gap_samples), (0, 0, 0));
    // The next clause starts cleanly and nothing is counted later either.
    m.push_translated(&tone(ms(100))).unwrap();
    m.end_translated().unwrap();
    render(&mut m, ms(400));
    assert_eq!(m.stats().translated.underruns, 0);
}

#[test]
fn a_tail_that_comes_with_the_end_still_counts_the_gap_before_it() {
    let mut m = mixer(false);
    m.push_translated(&tone(ms(300))).unwrap();
    render(&mut m, ms(400));
    m.push_translated(&tone(ms(60))).unwrap(); // under the depth: waits
    m.end_translated().unwrap();
    render(&mut m, ms(200));
    let s = m.stats().translated;
    assert_eq!(s.underruns, 1);
    assert!(s.gap_samples.abs_diff(ms(100) as u64) <= 2);
}

#[test]
fn a_stream_that_may_still_grow_fades_out_over_the_longer_fade() {
    let mut m = mixer(false);
    m.push_translated(&tone(ms(300))).unwrap();
    let out = render(&mut m, ms(400));
    let end = ms(300);
    let peak = |a: usize, b: usize| out[a..b].iter().fold(0.0f32, |p, s| p.max(s.abs()));
    // Ten milliseconds before the end the gate is about half way down: well below the tone.
    assert!(peak(end - ms(11), end - ms(9)) < 0.35, "{}", peak(end - ms(11), end - ms(9)));
    assert!(peak(end - 10, end) < 0.03);
    assert!(out[end..].iter().all(|s| *s == 0.0));
    let step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    assert!(step < 0.07, "a smooth fade, not a cut: {step}");
}

/// Play a clause of `clause_ms` that a producer delivers at `rate` times real time, in ticks of
/// 10 ms, then announce its end and let it play out. Returns the gaps counted for it.
fn clause(m: &mut Mixer, clause_ms: usize, rate: f32) -> u64 {
    let before = m.stats().translated.underruns;
    let per_tick = (480.0 * rate) as usize;
    let total = ms(clause_ms as u64);
    let mut sent = 0;
    while sent < total {
        let n = per_tick.min(total - sent);
        m.push_translated(&tone(n)).unwrap();
        sent += n;
        render(m, 480);
    }
    m.end_translated().unwrap();
    render(m, total + ms(2_000));
    m.stats().translated.underruns - before
}

#[test]
fn the_pre_roll_adapts_to_a_producer_slower_than_real_time() {
    let mut adaptive = mixer(true);
    let mut fixed = mixer(false);
    // The first clause teaches the model; both gap.
    assert!(clause(&mut adaptive, 3_000, 0.6) >= 1);
    assert!(clause(&mut fixed, 3_000, 0.6) >= 1);
    let wanted = adaptive.stats().translated.start_depth_samples;
    assert!(wanted > ms(150), "the depth grew: {wanted}");
    assert!(wanted <= ms(1_500), "and is capped: {wanted}");
    // The second clause starts with what it needs: no gap at all.
    assert_eq!(clause(&mut adaptive, 3_000, 0.6), 0);
    assert!(clause(&mut fixed, 3_000, 0.6) >= 1, "the fixed depth gaps again");
}

#[test]
fn a_fast_producer_keeps_the_fixed_pre_roll() {
    let mut m = mixer(true);
    for _ in 0..3 {
        assert_eq!(clause(&mut m, 2_000, 3.0), 0);
    }
    assert_eq!(m.stats().translated.start_depth_samples, ms(150));
}

#[test]
fn a_gap_that_cannot_be_avoided_is_still_a_fade() {
    // A producer at a quarter of real time: the cap cannot cover it, so gaps occur, smoothly.
    let mut m = mixer(true);
    let mut all = Vec::new();
    let per_tick = 120;
    for _ in 0..400 {
        m.push_translated(&tone(per_tick)).unwrap();
        all.extend(render(&mut m, 480));
    }
    m.end_translated().unwrap();
    all.extend(render(&mut m, ms(3_000)));
    assert!(m.stats().translated.underruns >= 1);
    let step = all.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    // A 440 Hz tone at 0.5 moves 0.03 per sample at most; a cut would be a jump of up to 0.5.
    assert!(step < 0.07, "no hard cut: {step}");
}

/// A clause of `clause_ms` produced at 1.5 times real time with one stall of `stall_ms` in the middle.
fn clause_with_stall(m: &mut Mixer, clause_ms: usize, stall_ms: usize) -> u64 {
    let before = m.stats().translated.underruns;
    let total = ms(clause_ms as u64);
    let per_tick = 720; // 1.5 times real time
    let (mut sent, mut stalled) = (0, false);
    while sent < total {
        if !stalled && sent > total / 2 {
            stalled = true;
            render(m, ms(stall_ms as u64));
        }
        let n = per_tick.min(total - sent);
        m.push_translated(&tone(n)).unwrap();
        sent += n;
        render(m, 480);
    }
    m.end_translated().unwrap();
    render(m, total + ms(2_000));
    m.stats().translated.underruns - before
}

#[test]
fn a_stall_teaches_the_lane_to_ride_out_the_next_one() {
    let mut m = mixer(true);
    assert_eq!(clause_with_stall(&mut m, 3_000, 0), 0);
    assert!(clause_with_stall(&mut m, 3_000, 800) >= 1, "the first stall is a gap");
    assert!(m.stats().translated.start_depth_samples > ms(150));
    assert_eq!(clause_with_stall(&mut m, 3_000, 800), 0, "the second is covered");
}
