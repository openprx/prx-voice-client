//! Scheduling, stability, fairness, cancellation and port behaviour of `QwenAsr` against a scripted
//! engine and an injected clock. No model, no GPU.

use std::sync::Arc;
use std::time::{Duration, Instant};

use prxvc_media::{Lang, ManualClock, MonoTime, SampleRate};
use prxvc_ports::{
    AsrContext, AsrHypothesis, AsrStream, CancellationToken, RefineAsr, StreamingAsr,
};

use super::adapter::{AsrParams, QwenAsr, QwenAsrConfig};
use super::test_engine::{TestHandle, engine};

const MS: usize = 16; // samples per millisecond at 16 kHz

pub(super) struct Fixture {
    pub(super) asr: QwenAsr,
    pub(super) engine: TestHandle,
    pub(super) clock: ManualClock,
    /// Decodes made by the warm-up.
    pub(super) base: usize,
}

pub(super) fn fixture(params: AsrParams) -> Fixture {
    let (e, handle) = engine();
    let clock = ManualClock::new();
    let asr = QwenAsr::start(
        Box::new(e),
        QwenAsrConfig {
            params,
            ..QwenAsrConfig::default()
        },
        Arc::new(clock.clone()),
    )
    .expect("start");
    let base = handle.started();
    Fixture {
        asr,
        engine: handle,
        clock,
        base,
    }
}

pub(super) fn wait_for(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A chunk of `ms` milliseconds of a tone.
pub(super) fn audio(ms: usize) -> Vec<f32> {
    (0..ms * MS)
        .map(|i| ((i % 40) as f32 / 40.0 - 0.5) * 0.4)
        .collect()
}

pub(super) fn open(f: &Fixture, lang: Lang) -> Box<dyn AsrStream> {
    f.asr.open(lang, AsrContext::default()).expect("open")
}

fn token() -> CancellationToken {
    CancellationToken::new()
}

#[test]
fn the_descriptor_says_segmented_and_is_live_but_not_native() {
    let f = fixture(AsrParams::default());
    let d = StreamingAsr::descriptor(&f.asr);
    assert_eq!(d.streaming, prxvc_ports::StreamingKind::Segmented);
    assert!(d.require_live().is_ok());
    assert!(d.require_native().is_err());
    assert!(d.supports_lang(Lang::Zh) && d.supports_lang(Lang::En));
    assert!(d.require_rate(SampleRate::HZ_16000).is_ok());
    assert!(d.require_rate(SampleRate::HZ_48000).is_err());
    assert_eq!(d, RefineAsr::descriptor(&f.asr));
}

#[test]
fn warm_up_decodes_both_languages_before_the_provider_is_ready() {
    let f = fixture(AsrParams::default());
    let calls = f.engine.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, Lang::Zh);
    assert_eq!(calls[1].0, Lang::En);
}

#[test]
fn a_failing_warm_up_is_a_load_error() {
    let (e, h) = engine();
    h.fail_decodes(true);
    let r = QwenAsr::start(
        Box::new(e),
        QwenAsrConfig::default(),
        Arc::new(ManualClock::new()),
    );
    assert!(matches!(r, Err(super::LoadError::Engine(_))));
}

#[test]
fn partials_follow_the_cadence_and_skip_when_not_due() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    // Too little audio for a first pass.
    assert_eq!(s.feed(&audio(300), MonoTime::ZERO), Ok(None));
    assert_eq!(f.engine.started(), f.base);
    // Enough: the first pass is requested at once.
    assert_eq!(s.feed(&audio(200), MonoTime::ZERO), Ok(None));
    wait_for("the first pass", || f.asr.stats().partial_passes == 1);
    // Collected by the next feed; not due again (clock has not moved).
    let h = s
        .feed(&audio(100), MonoTime::ZERO)
        .unwrap()
        .expect("first partial");
    assert_eq!((h.revision, h.is_final), (1, false));
    assert_eq!(h.text, format!("len{}", 500 * MS));
    assert_eq!(f.engine.started(), f.base + 1);
    // 399 ms later: still not due.
    f.clock.advance(Duration::from_millis(399));
    s.feed(&audio(100), MonoTime::ZERO).unwrap();
    assert_eq!(f.engine.started(), f.base + 1);
    // 400 ms: due, decodes everything fed so far.
    f.clock.advance(Duration::from_millis(1));
    s.feed(&audio(100), MonoTime::ZERO).unwrap();
    wait_for("the second pass", || f.asr.stats().partial_passes == 2);
    assert_eq!(f.engine.calls().last().copied(), Some((Lang::En, 800 * MS)));
    let h = s
        .feed(&audio(20), MonoTime::ZERO)
        .unwrap()
        .expect("second partial");
    assert_eq!(h.revision, 2);
}

#[test]
fn a_pass_that_is_due_while_the_previous_runs_is_skipped_not_queued() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    f.engine.hold();
    s.feed(&audio(500), MonoTime::ZERO).unwrap();
    wait_for("the pass to start", || f.engine.started() == f.base + 1);
    for _ in 0..3 {
        f.clock.advance(Duration::from_millis(400));
        s.feed(&audio(100), MonoTime::ZERO).unwrap();
    }
    assert_eq!(
        f.engine.started(),
        f.base + 1,
        "nothing queued behind the running pass"
    );
    assert_eq!(f.asr.stats().skipped_busy, 3);
    f.engine.release();
    wait_for("the pass to finish", || f.asr.stats().partial_passes == 1);
    // The next pass decodes all the audio there is, not the audio of the skipped requests' time.
    f.clock.advance(Duration::from_millis(400));
    s.feed(&audio(50), MonoTime::ZERO).unwrap();
    wait_for("the next pass", || f.asr.stats().partial_passes == 2);
    assert_eq!(f.engine.calls().last().copied(), Some((Lang::En, 850 * MS)));
}

#[test]
fn a_waiting_pass_decodes_the_latest_audio_when_it_finally_starts() {
    let f = fixture(AsrParams::default());
    let mut a = open(&f, Lang::Zh);
    let mut b = open(&f, Lang::En);
    f.engine.hold();
    b.feed(&audio(500), MonoTime::ZERO).unwrap();
    wait_for("b's pass to start", || f.engine.started() == f.base + 1);
    // a's pass is wanted but cannot start: the engine is busy. Its audio keeps growing.
    a.feed(&audio(500), MonoTime::ZERO).unwrap();
    a.feed(&audio(300), MonoTime::ZERO).unwrap();
    f.engine.release();
    wait_for("both passes", || f.asr.stats().partial_passes == 2);
    assert_eq!(f.engine.calls()[f.base + 1], (Lang::Zh, 800 * MS));
}

#[test]
fn finalize_decodes_the_whole_segment_and_preempts_a_running_pass() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::Zh);
    f.engine.hold();
    s.feed(&audio(600), MonoTime::from_millis(1000)).unwrap();
    wait_for("the partial to start", || f.engine.started() == f.base + 1);
    s.feed(&audio(200), MonoTime::from_millis(1600)).unwrap();
    // The held partial is abandoned for the final, which also waits for the hold: release shortly.
    let engine = f.engine.clone();
    let base = f.base;
    let releaser = std::thread::spawn(move || {
        wait_for("the final to start", || engine.started() == base + 2);
        engine.release();
    });
    let h = s.finalize(&token()).unwrap();
    releaser.join().unwrap();
    assert!(h.is_final);
    assert_eq!(h.text, format!("len{}", 800 * MS));
    assert_eq!(h.stable_prefix_len, h.text.len());
    assert_eq!(h.revision, 1);
    assert_eq!(h.audio_end, MonoTime::from_millis(1800));
    assert_eq!(
        f.engine.aborted(),
        1,
        "the partial was aborted, not waited for"
    );
    assert_eq!(f.asr.stats().finals, 1);
}

#[test]
fn the_final_revision_follows_the_last_partial_and_reset_restarts_the_segment() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    s.feed(&audio(500), MonoTime::ZERO).unwrap();
    wait_for("a partial", || f.asr.stats().partial_passes == 1);
    s.feed(&audio(20), MonoTime::ZERO).unwrap();
    let h = s.finalize(&token()).unwrap();
    assert_eq!(h.revision, 2);
    assert!(s.finalize(&token()).is_err(), "finalize twice is an error");
    s.reset();
    s.feed(&audio(500), MonoTime::ZERO).unwrap();
    wait_for("a partial of the new segment", || {
        f.asr.stats().partial_passes == 2
    });
    let h = s
        .feed(&audio(20), MonoTime::ZERO)
        .unwrap()
        .expect("partial");
    assert_eq!(h.revision, 1);
}

#[test]
fn a_feed_after_finalize_starts_a_new_segment() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    s.feed(&audio(500), MonoTime::ZERO).unwrap();
    s.finalize(&token()).unwrap();
    s.feed(&audio(100), MonoTime::from_millis(5000)).unwrap();
    let h = s.finalize(&token()).unwrap();
    assert_eq!(h.text, format!("len{}", 100 * MS));
    assert_eq!(h.revision, 1);
    assert_eq!(h.audio_end, MonoTime::from_millis(5100));
}

#[test]
fn an_empty_segment_finalizes_to_empty_text_without_decoding() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    let h = s.finalize(&token()).unwrap();
    assert_eq!(
        (h.text.as_str(), h.stable_prefix_len, h.is_final),
        ("", 0, true)
    );
    assert_eq!(f.engine.started(), f.base);
}

#[test]
fn reset_discards_a_pass_of_the_old_segment() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    f.engine.hold();
    s.feed(&audio(500), MonoTime::ZERO).unwrap();
    wait_for("the pass to start", || f.engine.started() == f.base + 1);
    s.reset();
    // The held pass is aborted by the reset (its generation is gone); nothing from it is emitted.
    wait_for("the abort", || f.engine.aborted() == 1);
    f.engine.release();
    assert_eq!(s.feed(&audio(10), MonoTime::ZERO), Ok(None));
    assert_eq!(f.asr.stats().partial_passes, 0);
}

#[test]
fn hypotheses_carry_the_stable_prefix_and_never_retract_it() {
    let f = fixture(AsrParams {
        stable_agreement: 3,
        stable_tail_guard: 0,
        ..AsrParams::default()
    });
    let mut s = open(&f, Lang::Zh);
    let mut out: Vec<AsrHypothesis> = Vec::new();
    f.engine.script(&[
        "我想去",
        "我想去商",
        "我想去商店",
        "我要去超市买",
        "我想去商店买东西",
    ]);
    for _ in 0..5 {
        f.clock.advance(Duration::from_millis(400));
        let before = f.asr.stats().partial_passes;
        s.feed(&audio(500), MonoTime::ZERO).unwrap();
        wait_for("a pass", || f.asr.stats().partial_passes > before);
        out.extend(s.feed(&audio(1), MonoTime::ZERO).unwrap());
    }
    let declared: Vec<&str> = out.iter().map(|h| &h.text[..h.stable_prefix_len]).collect();
    // Each stable prefix extends the previous one (never retracts).
    for w in declared.windows(2) {
        assert!(w[1].starts_with(w[0]), "{declared:?}");
    }
    assert!(!declared[4].is_empty());
    assert!(
        out[3].text.starts_with(declared[2]),
        "the rewrite was repaired: {:?}",
        out[3].text
    );
    assert_eq!(f.asr.stats().stable_repairs, 1);
    // Revisions increase by one per hypothesis.
    let revs: Vec<u32> = out.iter().map(|h| h.revision).collect();
    assert_eq!(revs, [1, 2, 3, 4, 5]);
}

#[test]
fn an_unchanged_text_is_not_emitted_again() {
    let f = fixture(AsrParams {
        stable_tail_guard: 0,
        ..AsrParams::default()
    });
    f.engine.text_with(|_, _| "same words here".to_owned());
    let mut s = open(&f, Lang::En);
    let mut emitted = Vec::new();
    for _ in 0..6 {
        f.clock.advance(Duration::from_millis(400));
        let before = f.asr.stats().partial_passes;
        s.feed(&audio(500), MonoTime::ZERO).unwrap();
        wait_for("a pass", || f.asr.stats().partial_passes > before);
        emitted.extend(s.feed(&audio(1), MonoTime::ZERO).unwrap());
    }
    // First sight, then the stable prefix grows to the whole text on the third agreement; the
    // remaining identical hypotheses add nothing.
    let stables: Vec<usize> = emitted.iter().map(|h| h.stable_prefix_len).collect();
    assert_eq!(stables.first(), Some(&0));
    assert_eq!(stables.last(), Some(&"same words here".len()));
    assert_eq!(emitted.len(), 2);
}

#[test]
fn the_maximum_segment_length_forces_an_endpoint_at_the_quietest_point() {
    let params = AsrParams {
        max_segment: Duration::from_secs(2),
        cut_search: Duration::from_millis(500),
        ..AsrParams::default()
    };
    let f = fixture(params);
    let mut s = open(&f, Lang::En);
    // 1.7 s of tone, a 40 ms gap of silence, then tone: the limit is reached at 2 s.
    let mut stream = audio(1700);
    stream.extend(vec![0.0; 40 * MS]);
    stream.extend(audio(400));
    let mut finals = Vec::new();
    let mut fed = 0;
    let mut ts = MonoTime::ZERO;
    for chunk in stream.chunks(100 * MS) {
        finals.extend(s.feed(chunk, ts).unwrap().filter(|h| h.is_final));
        fed += chunk.len();
        ts = ts.saturating_add(Duration::from_millis(100));
        f.clock.advance(Duration::from_millis(100));
    }
    assert_eq!(f.asr.stats().forced_cuts, 1);
    // The cut final arrives on a later feed, once its decode is done.
    for _ in 0..500 {
        if !finals.is_empty() {
            break;
        }
        finals.extend(s.feed(&audio(10), ts).unwrap().filter(|h| h.is_final));
        fed += 10 * MS;
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(finals.len(), 1);
    let h = &finals[0];
    // The head ends inside the gap (1700 to 1740 ms), not at the 2 s limit.
    let head = f
        .engine
        .calls()
        .iter()
        .map(|c| c.1)
        .find(|&n| n > 1700 * MS && n < 1800 * MS)
        .expect("a decode of the head");
    assert!(head <= 1740 * MS, "{head}");
    assert_eq!(h.text, format!("len{head}"));
    assert_eq!(h.stable_prefix_len, h.text.len());
    assert_eq!(
        h.audio_end,
        MonoTime::ZERO.saturating_add(Duration::from_millis((head / MS) as u64))
    );
    // The stream continues with the tail as the next segment; its revisions keep counting.
    let last = s.finalize(&token()).unwrap();
    assert_eq!(last.text, format!("len{}", fed - head));
    assert_eq!(last.revision, h.revision + 1);
}

#[test]
fn the_default_guard_keeps_the_end_of_a_growing_text_unstable() {
    let f = fixture(AsrParams::default());
    f.engine.text_with(|_, n| {
        let words = [
            "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        ];
        words[..(n / (400 * MS)).clamp(1, 10)].join(" ")
    });
    let mut s = open(&f, Lang::En);
    let mut last = None;
    for _ in 0..8 {
        f.clock.advance(Duration::from_millis(400));
        let before = f.asr.stats().partial_passes;
        s.feed(&audio(500), MonoTime::ZERO).unwrap();
        wait_for("a pass", || f.asr.stats().partial_passes > before);
        last = s.feed(&audio(1), MonoTime::ZERO).unwrap().or(last);
    }
    let h = last.expect("a partial");
    let stable = &h.text[..h.stable_prefix_len];
    let units = stable.split_whitespace().count();
    let total = h.text.split_whitespace().count();
    assert!(
        units > 0 && units + 4 <= total,
        "{stable:?} of {:?}",
        h.text
    );
}
