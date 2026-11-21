//! Randomised interleavings of everything a pump does to the mixer: the pump thread must never panic, whatever the order of pushes, ends, flushes, pace changes, mode switches and renders.

use std::time::Duration;

use prxvc_media::{AudioFrame, MonoTime, SampleRate};

use super::*;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> usize {
        (self.next() % n) as usize
    }
}

fn frame(rng: &mut Lcg, rate: SampleRate, phase: usize) -> AudioFrame {
    let len = 1 + rng.below(rate.samples_in(Duration::from_millis(250)) as u64);
    // Speech-like bursts with silence at both ends, so that trimming has work to do.
    let lead = rng.below(len as u64 / 2 + 1);
    let tail = rng.below(len as u64 / 2 + 1);
    let samples = (0..len)
        .map(|i| {
            if i < lead || i + tail >= len {
                0.0
            } else {
                0.4 * ((phase + i) as f32 * 0.05).sin()
            }
        })
        .collect();
    AudioFrame::new(samples, rate, MonoTime::ZERO, 0).expect("frame")
}

fn pace(rng: &mut Lcg) -> Pace {
    Pace {
        speed: [0.5, 1.0, 1.1, 1.25, 1.6, 9.0][rng.below(6)],
        tight: rng.below(2) == 0,
    }
}

fn run(seed: u64, steps: usize) {
    let mut rng = Lcg(seed);
    let mut m = Mixer::new(MixerConfig {
        jitter_buffer: Duration::from_millis([0, 40, 150][rng.below(3)]),
        adaptive_preroll: rng.below(2) == 0,
        trim_silence: rng.below(4) != 0,
        ..MixerConfig::default()
    });
    let rates = [SampleRate::HZ_16000, SampleRate::HZ_24000, SampleRate::HZ_48000];
    let mut phase = 0;
    let mut last = m.stats().translated;
    for step in 0..steps {
        match rng.below(12) {
            0..=3 => {
                let rate = rates[rng.below(3)];
                let f = frame(&mut rng, rate, phase);
                phase += f.len();
                m.push_translated(&f).expect("push");
            }
            4 | 5 => m.end_translated().expect("end"),
            6 => m.flush_translated(),
            7 => m.set_translated_pace(pace(&mut rng)),
            8 => m.set_mode([MixMode::Translation, MixMode::Both, MixMode::Original][rng.below(3)]),
            _ => {
                let mut out = vec![0.0; 1 + rng.below(4_000)];
                m.render(&mut out);
                assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0), "seed {seed} step {step}");
            }
        }
        let s = m.stats().translated;
        assert!(s.played_total <= s.enqueued_total + 1, "seed {seed} step {step}: played {} of {}", s.played_total, s.enqueued_total);
        assert!(s.played_total >= last.played_total, "seed {seed} step {step}: played went back");
        last = s;
    }
    // The end of a call: the stream is ended and everything is played out.
    m.end_translated().expect("end");
    let mut out = vec![0.0; 48_000];
    for _ in 0..40 {
        m.render(&mut out);
    }
}

#[test]
fn no_interleaving_of_push_end_flush_pace_and_render_panics() {
    for seed in 0..12 {
        run(seed, 80);
    }
}

#[test]
fn a_flush_or_an_end_with_the_speed_up_active_never_panics() {
    // The stop of the call while a backlog is caught up: speed above 1, clauses queued and a
    // flush in the middle of a stream, then the stream's end.
    for len_ms in [1usize, 7, 22, 60, 130, 700, 2_300] {
        for speed in [1.1f32, 1.25, 2.0] {
            let mut m = Mixer::new(MixerConfig::default());
            let rate = SampleRate::HZ_24000;
            m.set_translated_pace(Pace { speed, tight: true });
            let samples = (0..rate.samples_in(Duration::from_millis(len_ms as u64)))
                .map(|i| 0.4 * (i as f32 * 0.09).sin())
                .collect();
            let f = AudioFrame::new(samples, rate, MonoTime::ZERO, 0).expect("frame");
            for _ in 0..3 {
                m.push_translated(&f).expect("push");
                m.end_translated().expect("end");
            }
            m.push_translated(&f).expect("push");
            let mut out = vec![0.0; 4_800];
            m.render(&mut out);
            m.flush_translated();
            m.end_translated().expect("end");
            m.render(&mut out);
        }
    }
}
