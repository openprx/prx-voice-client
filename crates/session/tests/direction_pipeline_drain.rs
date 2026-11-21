//! A graceful stop of one direction: no new audio is accepted, the open segment is ended so that what was said last is committed, and everything committed is translated, synthesised and played to the end.

mod pipeline_rig;

use prxvc_media::ManualClock;
use prxvc_ports::fakes::{AsrScript, FakeSpeechSynth, FakeTranslator, MtScript, TtsScript};
use prxvc_session::pipeline::PushOutcome;

use pipeline_rig::{Rig, vad_runs};

#[test]
fn draining_commits_the_open_segment_and_plays_it_to_the_end() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script("今天天气很好", MtScript::tokens(["The ", "weather ", "is ", "nice."]));
    let tts = FakeSpeechSynth::new(clock.clone())
        .with_script("The weather is nice.", TtsScript::chunks(3, 2400));
    // Speech that has not paused: the segment is still open when the stop comes.
    let vad = vad_runs(&[(0.0, 10), (0.9, 400)]);
    let asr = AsrScript::new(vec!["今天天气很好"; 60], "今天天气很好");
    let rig = Rig::start(&clock, vad, asr, mt, tts);
    rig.wait_running();
    rig.feed(24);
    let before = rig.latest();
    assert_eq!(before.counters.clauses_played, 0);
    assert!(!rig.pipeline.drain_flushed());

    rig.pipeline.drain();
    rig.wait_snapshot("the last sentence is playing", |s| {
        s.clauses.first().is_some_and(|c| c.state == prxvc_session::ClauseState::Playing)
    });
    rig.advance(pipeline_rig::ms(2_000));
    let snap = rig.wait_snapshot("the last sentence was played", |s| s.counters.clauses_played >= 1);
    assert_eq!(snap.counters.clauses_failed, 0);
    assert!(rig.pipeline.drain_flushed());
    assert!(rig.fakes.mt.requests().iter().any(|r| r.text == "今天天气很好"));
    assert!(rig.stop().clean);
}

#[test]
fn nothing_new_is_accepted_after_a_drain() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone());
    let tts = FakeSpeechSynth::new(clock.clone());
    let rig = Rig::start(&clock, vad_runs(&[(0.0, 400)]), AsrScript::new([], ""), mt, tts);
    rig.wait_running();
    rig.feed(5);
    let consumed = rig.pipeline.stats().frames_consumed;
    rig.pipeline.drain();
    let frame = prxvc_media::AudioFrame::new(
        vec![0.1; 512],
        prxvc_media::SampleRate::HZ_16000,
        prxvc_media::MonoTime::from_millis(10_000),
        99,
    )
    .unwrap();
    assert_eq!(rig.pipeline.audio().push(frame), PushOutcome::Draining);
    assert_eq!(rig.pipeline.stats().frames_consumed, consumed);
    assert!(rig.stop().clean);
}

#[test]
fn a_drain_with_nothing_open_is_flushed_at_once() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone());
    let tts = FakeSpeechSynth::new(clock.clone());
    let rig = Rig::start(&clock, vad_runs(&[(0.0, 400)]), AsrScript::new([], ""), mt, tts);
    rig.wait_running();
    rig.feed(5);
    rig.pipeline.drain();
    let began = std::time::Instant::now();
    while !rig.pipeline.drain_flushed() {
        assert!(began.elapsed() < std::time::Duration::from_secs(5), "never flushed");
        std::thread::yield_now();
    }
    assert!(rig.stop().clean);
}
