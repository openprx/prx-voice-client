//! Fairness between streams, preemption, cancellation, dropping, the refine port and failures.

use std::time::Duration;

use prxvc_media::{Lang, MonoTime, SampleRate};
use prxvc_ports::{AsrContext, AsrError, CancellationToken, RefineAsr, StreamingAsr};

use super::adapter::AsrParams;
use super::tests::{audio, fixture, open, wait_for};

const MS: usize = 16;

#[test]
fn streams_are_served_round_robin_and_neither_starves() {
    // Both directions always have a pass wanted while one engine serves them; the engine is held
    // so that both requests are waiting when it frees up, round after round.
    let f = fixture(AsrParams::default());
    let mut zh = open(&f, Lang::Zh);
    let mut en = open(&f, Lang::En);
    let mut served = Vec::new();
    for round in 0..6usize {
        f.clock.advance(Duration::from_millis(400));
        f.engine.hold();
        let base = f.engine.started();
        // The outgoing direction asks first every round, the incoming one right after.
        zh.feed(&audio(500), MonoTime::ZERO).unwrap();
        wait_for("the first pass to start", || f.engine.started() == base + 1);
        en.feed(&audio(500), MonoTime::ZERO).unwrap();
        f.engine.release();
        wait_for("both passes", || f.engine.finished() == base + 2);
        wait_for("both results", || {
            f.asr.stats().partial_passes == 2 * (round + 1) as u64
        });
        zh.feed(&audio(1), MonoTime::ZERO).unwrap();
        en.feed(&audio(1), MonoTime::ZERO).unwrap();
        let calls = f.engine.calls();
        served.extend(calls[base..].iter().map(|c| c.0));
    }
    let zh_count = served.iter().filter(|l| **l == Lang::Zh).count();
    assert_eq!((zh_count, served.len() - zh_count), (6, 6));
    // The stream that asked second is served second, then the other first again: strict alternation.
    for pair in served.chunks(2) {
        assert_eq!(pair, [Lang::Zh, Lang::En]);
    }
    assert_eq!(f.asr.stats().skipped_busy, 0);
}

#[test]
fn a_waiting_stream_goes_before_a_stream_that_was_served_last() {
    let f = fixture(AsrParams::default());
    let mut a = open(&f, Lang::Zh);
    let mut b = open(&f, Lang::En);
    f.engine.hold();
    let base = f.engine.started();
    a.feed(&audio(500), MonoTime::ZERO).unwrap();
    wait_for("a to start", || f.engine.started() == base + 1);
    b.feed(&audio(500), MonoTime::ZERO).unwrap();
    // a finishes and asks again before b is served: a's second request must wait behind b.
    f.engine.release();
    wait_for("a done", || f.asr.stats().partial_passes >= 1);
    f.engine.hold();
    f.clock.advance(Duration::from_millis(400));
    a.feed(&audio(500), MonoTime::ZERO).unwrap();
    f.engine.release();
    wait_for("three passes", || f.asr.stats().partial_passes == 3);
    let langs: Vec<Lang> = f.engine.calls()[base..].iter().map(|c| c.0).collect();
    assert_eq!(langs, [Lang::Zh, Lang::En, Lang::Zh]);
}

#[test]
fn a_final_preempts_another_streams_partial_which_is_decoded_again_afterwards() {
    let f = fixture(AsrParams::default());
    let mut a = open(&f, Lang::Zh);
    let mut b = open(&f, Lang::En);
    f.engine.hold();
    let base = f.engine.started();
    a.feed(&audio(500), MonoTime::ZERO).unwrap();
    wait_for("a's pass", || f.engine.started() == base + 1);
    b.feed(&audio(300), MonoTime::ZERO).unwrap();
    let engine = f.engine.clone();
    let releaser = std::thread::spawn(move || {
        wait_for("b's final", || engine.started() == base + 2);
        engine.release();
    });
    let h = b.finalize(&CancellationToken::new()).unwrap();
    releaser.join().unwrap();
    assert_eq!(h.text, format!("len{}", 300 * MS));
    assert_eq!(f.engine.aborted(), 1);
    // a's abandoned pass is wanted again and decodes the same (latest) audio.
    wait_for("a's second pass", || f.asr.stats().partial_passes == 1);
    let stats = f.asr.stats();
    assert_eq!(stats.aborted_passes, 1);
    assert_eq!(f.engine.calls()[base + 2], (Lang::Zh, 500 * MS));
}

#[test]
fn cancelling_a_final_returns_cancelled_and_leaves_the_engine_usable() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    s.feed(&audio(300), MonoTime::ZERO).unwrap();
    f.engine.hold();
    let token = CancellationToken::new();
    let t = token.clone();
    let engine = f.engine.clone();
    let base = f.base;
    let canceller = std::thread::spawn(move || {
        wait_for("the final to start", || engine.started() == base + 1);
        t.cancel();
    });
    let r = s.finalize(&token);
    canceller.join().unwrap();
    assert_eq!(r, Err(AsrError::Cancelled));
    wait_for("the abort", || f.engine.aborted() == 1);
    // The engine and the other streams are unaffected.
    f.engine.release();
    let mut other = open(&f, Lang::Zh);
    other.feed(&audio(200), MonoTime::ZERO).unwrap();
    let h = other.finalize(&CancellationToken::new()).unwrap();
    assert_eq!(h.text, format!("len{}", 200 * MS));
}

#[test]
fn a_token_cancelled_before_the_call_returns_cancelled_at_once() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    s.feed(&audio(300), MonoTime::ZERO).unwrap();
    f.engine.hold();
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(s.finalize(&token), Err(AsrError::Cancelled));
    f.engine.release();
}

#[test]
fn dropping_a_stream_abandons_its_running_pass() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    f.engine.hold();
    s.feed(&audio(500), MonoTime::ZERO).unwrap();
    wait_for("the pass to start", || f.engine.started() == f.base + 1);
    drop(s);
    wait_for("the abort", || f.engine.aborted() == 1);
    f.engine.release();
    // Nothing is re-requested for the dropped stream.
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(f.engine.started(), f.base + 1);
    assert_eq!(f.asr.stats().aborted_passes, 0);
}

#[test]
fn streams_are_independent() {
    let f = fixture(AsrParams::default());
    let mut a = open(&f, Lang::Zh);
    let mut b = open(&f, Lang::En);
    a.feed(&audio(200), MonoTime::ZERO).unwrap();
    b.feed(&audio(300), MonoTime::ZERO).unwrap();
    let ha = a.finalize(&CancellationToken::new()).unwrap();
    // Finalizing a leaves b open and its audio intact.
    b.feed(&audio(100), MonoTime::ZERO).unwrap();
    let hb = b.finalize(&CancellationToken::new()).unwrap();
    assert_eq!(ha.text, format!("len{}", 200 * MS));
    assert_eq!(hb.text, format!("len{}", 400 * MS));
    assert_eq!(f.engine.calls()[f.base], (Lang::Zh, 200 * MS));
    assert_eq!(f.engine.calls()[f.base + 1], (Lang::En, 400 * MS));
}

#[test]
fn the_language_of_a_stream_is_the_language_of_its_decodes() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::Zh);
    s.feed(&audio(100), MonoTime::ZERO).unwrap();
    s.finalize(&CancellationToken::new()).unwrap();
    assert_eq!(f.engine.calls()[f.base].0, Lang::Zh);
}

#[test]
fn refine_transcribes_one_segment_and_checks_rate_and_cancellation() {
    let f = fixture(AsrParams::default());
    let samples = audio(700);
    let h = f
        .asr
        .transcribe(
            &samples,
            SampleRate::HZ_16000,
            Lang::En,
            &AsrContext::default(),
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(h.text, format!("len{}", 700 * MS));
    assert_eq!(
        (h.revision, h.is_final, h.stable_prefix_len),
        (1, true, h.text.len())
    );
    assert_eq!(h.audio_end, MonoTime::from_millis(700));
    assert_eq!(f.asr.stats().refines, 1);
    let r = f.asr.transcribe(
        &samples,
        SampleRate::HZ_48000,
        Lang::En,
        &AsrContext::default(),
        &CancellationToken::new(),
    );
    assert_eq!(r, Err(AsrError::UnsupportedRate(SampleRate::HZ_48000)));
    let token = CancellationToken::new();
    token.cancel();
    let r = f.asr.transcribe(
        &samples,
        SampleRate::HZ_16000,
        Lang::En,
        &AsrContext::default(),
        &token,
    );
    assert_eq!(r, Err(AsrError::Cancelled));
    let empty = f
        .asr
        .transcribe(
            &[],
            SampleRate::HZ_16000,
            Lang::Zh,
            &AsrContext::default(),
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(empty.text, "");
    assert_eq!(
        f.engine.started(),
        f.base + 1,
        "empty and cancelled calls decode nothing"
    );
}

#[test]
fn a_refine_call_waits_behind_nothing_but_other_finals() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    f.engine.hold();
    s.feed(&audio(500), MonoTime::ZERO).unwrap();
    wait_for("the pass to start", || f.engine.started() == f.base + 1);
    let engine = f.engine.clone();
    let base = f.base;
    let releaser = std::thread::spawn(move || {
        wait_for("the refine decode", || engine.started() == base + 2);
        engine.release();
    });
    let h = f
        .asr
        .transcribe(
            &audio(200),
            SampleRate::HZ_16000,
            Lang::Zh,
            &AsrContext::default(),
            &CancellationToken::new(),
        )
        .unwrap();
    releaser.join().unwrap();
    assert_eq!(h.text, format!("len{}", 200 * MS));
}

#[test]
fn a_call_that_exceeds_the_timeout_fails_with_timeout() {
    let f = fixture(AsrParams {
        final_timeout: Duration::from_secs(3),
        ..AsrParams::default()
    });
    f.engine.hold();
    let clock = f.clock.clone();
    let engine = f.engine.clone();
    let base = f.base;
    let mover = std::thread::spawn(move || {
        wait_for("the decode", || engine.started() == base + 1);
        clock.advance(Duration::from_secs(4));
    });
    let r = f.asr.transcribe(
        &audio(200),
        SampleRate::HZ_16000,
        Lang::En,
        &AsrContext::default(),
        &CancellationToken::new(),
    );
    mover.join().unwrap();
    assert_eq!(r, Err(AsrError::Timeout));
    f.engine.release();
}

#[test]
fn an_engine_failure_in_a_pass_is_reported_by_the_next_feed() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    f.engine.fail_decodes(true);
    s.feed(&audio(500), MonoTime::ZERO).unwrap();
    wait_for("the failure", || f.engine.finished() == f.base + 1);
    let mut err = None;
    for _ in 0..100 {
        match s.feed(&audio(1), MonoTime::ZERO) {
            Err(e) => {
                err = Some(e);
                break;
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    assert!(matches!(err, Some(AsrError::Internal(m)) if m.contains("scripted failure")));
    // The engine stays usable.
    f.engine.fail_decodes(false);
    let mut other = open(&f, Lang::En);
    other.feed(&audio(100), MonoTime::ZERO).unwrap();
    assert!(other.finalize(&CancellationToken::new()).is_ok());
}

#[test]
fn an_engine_panic_stops_the_provider_with_a_visible_error() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    s.feed(&audio(300), MonoTime::ZERO).unwrap();
    f.engine.panic_on_next_decode();
    let r = s.finalize(&CancellationToken::new());
    assert!(matches!(r, Err(AsrError::Internal(m)) if m.contains("panicked")));
    wait_for("the worker to stop", || {
        f.asr.open(Lang::En, AsrContext::default()).is_err()
    });
    assert!(matches!(
        f.asr.open(Lang::En, AsrContext::default()).err(),
        Some(AsrError::Unavailable { .. })
    ));
}

#[test]
fn shutting_down_fails_open_streams_and_new_opens() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    f.asr.shut_down();
    assert!(matches!(
        s.feed(&audio(10), MonoTime::ZERO),
        Err(AsrError::Unavailable { .. })
    ));
    assert!(matches!(
        s.finalize(&CancellationToken::new()),
        Err(AsrError::Unavailable { .. })
    ));
    assert!(matches!(
        f.asr.open(Lang::Zh, AsrContext::default()).err(),
        Some(AsrError::Unavailable { .. })
    ));
}

#[test]
fn audio_with_a_non_finite_sample_is_refused() {
    let f = fixture(AsrParams::default());
    let mut s = open(&f, Lang::En);
    let mut bad = audio(10);
    bad[3] = f32::NAN;
    assert!(matches!(
        s.feed(&bad, MonoTime::ZERO),
        Err(AsrError::Internal(_))
    ));
}

#[test]
fn silence_has_no_text_and_costs_no_decode() {
    let f = fixture(AsrParams::default());
    let mut stream = open(&f, Lang::Zh);
    let zeros = vec![0.0f32; 100 * MS];
    for i in 0..10 {
        f.clock.advance(Duration::from_millis(100));
        let partial = stream.feed(&zeros, MonoTime::from_millis(i * 100)).unwrap();
        assert!(partial.is_none_or(|h| h.text.is_empty()));
    }
    let fin = stream.finalize(&CancellationToken::new()).unwrap();
    assert_eq!(fin.text, "");
    let refined = f
        .asr
        .transcribe(
            &zeros,
            SampleRate::HZ_16000,
            Lang::En,
            &AsrContext::default(),
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(refined.text, "");
    assert_eq!(f.engine.started(), f.base, "the engine was never asked");
}

#[test]
fn quiet_speech_is_still_decoded() {
    let f = fixture(AsrParams::default());
    let mut stream = open(&f, Lang::Zh);
    let quiet: Vec<f32> = audio(200).into_iter().map(|s| s * 0.01).collect();
    stream.feed(&quiet, MonoTime::ZERO).unwrap();
    let fin = stream.finalize(&CancellationToken::new()).unwrap();
    assert!(!fin.text.is_empty());
    assert_eq!(f.engine.started(), f.base + 1);
}
