//! Trimming silence and catching up with speed in the translated lane.

use std::time::Duration;

use prxvc_media::{AudioFrame, MonoTime, SampleRate};

use super::*;

const RATE: SampleRate = SampleRate::HZ_48000;

fn ms(n: u64) -> usize {
    RATE.samples_in(Duration::from_millis(n))
}

fn frame(samples: Vec<f32>) -> AudioFrame {
    AudioFrame::new(samples, RATE, MonoTime::ZERO, 0).unwrap()
}

fn tone(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin())
        .collect()
}

/// A clause as a synthesiser delivers it: silence, speech, silence, in blocks of 80 ms.
fn clause(lead_ms: u64, speech_ms: u64, trail_ms: u64) -> Vec<Vec<f32>> {
    let mut all = vec![0.0; ms(lead_ms)];
    all.extend(tone(ms(speech_ms)));
    all.extend(vec![0.0; ms(trail_ms)]);
    all.chunks(ms(80)).map(<[f32]>::to_vec).collect()
}

fn mixer() -> Mixer {
    Mixer::new(MixerConfig {
        mode: MixMode::Translation,
        adaptive_preroll: false,
        ..MixerConfig::default()
    })
}

fn queue(m: &mut Mixer, blocks: &[Vec<f32>]) {
    for b in blocks {
        m.push_translated(&frame(b.clone())).unwrap();
    }
    m.end_translated().unwrap();
}

fn queued_ms(m: &Mixer) -> f64 {
    m.stats().translated.queued_samples as f64 / 48.0
}

#[test]
fn the_silence_at_both_ends_of_a_clause_is_cut_to_what_is_kept() {
    let mut m = mixer();
    queue(&mut m, &clause(400, 300, 500));
    // 60 ms kept at the start, 120 ms at the end; the 5 ms of the block grid are within the margin.
    let q = queued_ms(&m);
    assert!((q - (60.0 + 300.0 + 120.0)).abs() < 12.0, "{q} ms queued");
    let s = m.stats().translated;
    assert_eq!(s.enqueued_total, s.queued_samples as u64, "the counters follow the trim");
    assert!(s.trimmed_silence_samples as f64 / 48.0 > 700.0);
}

#[test]
fn a_backlog_cuts_the_pauses_to_a_third() {
    let mut m = mixer();
    m.set_translated_pace(Pace { speed: 1.0, tight: true });
    queue(&mut m, &clause(400, 300, 500));
    let q = queued_ms(&m);
    assert!((q - (20.0 + 300.0 + 40.0)).abs() < 12.0, "{q} ms queued");
}

#[test]
fn a_clause_that_is_only_silence_queues_at_most_what_is_kept_and_is_not_a_gap() {
    let mut m = mixer();
    queue(&mut m, &clause(500, 0, 0));
    assert!(queued_ms(&m) <= 61.0, "{} ms", queued_ms(&m));
    let mut out = vec![0.0; ms(300)];
    m.render(&mut out);
    assert_eq!(m.stats().translated.underruns, 0);
}

#[test]
fn a_speed_above_one_shortens_the_clause_and_keeps_every_sample_accounted_for() {
    let blocks = clause(0, 2_000, 0);
    let mut normal = mixer();
    queue(&mut normal, &blocks);
    let mut fast = mixer();
    fast.set_translated_pace(Pace { speed: 1.25, tight: false });
    queue(&mut fast, &blocks);
    let (n, f) = (queued_ms(&normal), queued_ms(&fast));
    assert!((f - n / 1.25).abs() < 40.0, "normal {n} ms, fast {f} ms");
    let s = fast.stats().translated;
    assert_eq!(s.enqueued_total, s.queued_samples as u64);
    assert!((s.saved_samples as f64 / 48.0 - (n - f)).abs() < 40.0);
    // It plays to the end without a gap.
    let mut out = vec![0.0; ms(2_500)];
    fast.render(&mut out);
    assert_eq!(fast.stats().translated.underruns, 0);
}

#[test]
fn the_speed_never_exceeds_what_the_mixer_allows() {
    let blocks = clause(0, 2_000, 0);
    let mut m = Mixer::new(MixerConfig {
        catch_up_max_speed: 1.1,
        adaptive_preroll: false,
        ..MixerConfig::default()
    });
    m.set_translated_pace(Pace { speed: 1.9, tight: false });
    queue(&mut m, &blocks);
    assert!(queued_ms(&m) > 2_000.0 / 1.1 - 40.0);
}

#[test]
fn the_next_clause_starts_clean_after_a_sped_up_one() {
    let mut m = mixer();
    m.set_translated_pace(Pace { speed: 1.25, tight: false });
    queue(&mut m, &clause(300, 500, 300));
    let mut out = vec![0.0; ms(800)];
    m.render(&mut out);
    m.set_translated_pace(Pace::default());
    queue(&mut m, &clause(300, 500, 300));
    let q = queued_ms(&m);
    assert!((q - (60.0 + 500.0 + 120.0)).abs() < 15.0, "{q} ms queued");
}

#[test]
fn without_trimming_a_clause_is_queued_whole() {
    let mut m = Mixer::new(MixerConfig {
        trim_silence: false,
        adaptive_preroll: false,
        ..MixerConfig::default()
    });
    queue(&mut m, &clause(400, 300, 500));
    assert!((queued_ms(&m) - 1_200.0).abs() < 3.0, "{} ms", queued_ms(&m));
}

/// A clause delivered live by a producer `rate` times as fast as real time, in 10 ms ticks.
fn live(m: &mut Mixer, clause_ms: u64, rate: f32) -> u64 {
    let before = m.stats().translated.underruns;
    let all = tone(ms(clause_ms));
    let per_tick = (480.0 * rate) as usize;
    for block in all.chunks(per_tick) {
        m.push_translated(&frame(block.to_vec())).unwrap();
        let mut out = vec![0.0; 480];
        m.render(&mut out);
    }
    m.end_translated().unwrap();
    let mut out = vec![0.0; ms(clause_ms) + ms(1_000)];
    m.render(&mut out);
    m.stats().translated.underruns - before
}

#[test]
fn a_clause_that_streams_in_live_is_not_starved_by_the_speed_up_or_the_trim() {
    for rate in [1.5f32, 3.0] {
        for speed in [1.0f32, 1.25] {
            let mut m = mixer();
            m.set_translated_pace(Pace { speed, tight: true });
            assert_eq!(live(&mut m, 3_000, rate), 0, "producer {rate}x, speed {speed}x");
        }
    }
}
