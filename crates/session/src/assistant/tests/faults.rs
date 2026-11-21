//! Every typed fault reaches the host: a reply that fails leaves the session listening, a fault of the microphone or of listening ends it.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use prxvc_media::{Lang, SampleRate};
use prxvc_ports::fakes::{
    AsrScript, DialogueScript, FakeDialogue, FakeSpeechSynth, TtsScript, VadScript,
};
use prxvc_ports::{
    AsrContext, AsrError, AsrStream, DialogueError, EndpointError, ProviderDescriptor,
    StreamingAsr, StreamingKind, TtsError, VadError,
};

use super::super::{AssistantEvent, AssistantFault, Phase, StartError};
use super::rig::{ONE_UTTERANCE, Rig, RigConfig, vad_runs};
use crate::fault::{FaultKind, Stage};

fn failing_turn(cfg_dialogue: Option<FakeDialogue>, cfg_tts: Option<FakeSpeechSynth>) -> Rig {
    let (mut cfg, clock) = RigConfig::new("what time is it", ["x"]);
    if cfg_dialogue.is_some() {
        cfg.dialogue = cfg_dialogue;
    }
    if cfg_tts.is_some() {
        cfg.tts = cfg_tts;
    }
    let mut rig = Rig::build(cfg, clock);
    rig.start().expect("starts");
    rig.speak();
    rig
}

#[test]
fn a_dialogue_server_that_does_not_answer_is_a_typed_fault_and_the_session_listens_again() {
    let clock = prxvc_media::ManualClock::new();
    let down = DialogueError::Unavailable {
        reason: "cannot connect: connection refused or host unreachable".into(),
    };
    let dialogue = FakeDialogue::new(clock)
        .with_default_script(DialogueScript::tokens(["x"]).failing_open(down.clone()));
    let mut rig = failing_turn(Some(dialogue), None);
    rig.wait_for("the fault", |e| matches!(e, AssistantEvent::Fault(_)));
    rig.wait_until("listening again", |r| {
        r.phases().ends_with(&[Phase::Thinking, Phase::Listening])
    });
    assert_eq!(rig.faults(), [AssistantFault::Dialogue(down)]);
    assert!(
        !rig.handle().is_finished(),
        "a failed turn does not end the session"
    );
    assert!(rig.tts.requests().is_empty(), "no mock reply is spoken");
    rig.stop();
}

#[test]
fn a_dialogue_that_fails_in_the_middle_keeps_the_text_it_delivered_and_reports_the_fault() {
    let clock = prxvc_media::ManualClock::new();
    let err = DialogueError::Unavailable {
        reason: "the connection broke".into(),
    };
    let dialogue = FakeDialogue::new(clock).with_default_script(
        DialogueScript::tokens(["Well, ", "I think"]).failing_after(1, err.clone()),
    );
    let mut rig = failing_turn(Some(dialogue), None);
    rig.wait_for("the fault", |e| matches!(e, AssistantEvent::Fault(_)));
    assert!(rig.reply_text().starts_with("Well"));
    assert_eq!(rig.faults(), [AssistantFault::Dialogue(err)]);
    rig.stop();
}

#[test]
fn a_timeout_and_a_rejection_of_the_dialogue_are_told_apart() {
    for err in [
        DialogueError::Timeout,
        DialogueError::Rejected { status: 401 },
    ] {
        let clock = prxvc_media::ManualClock::new();
        let dialogue = FakeDialogue::new(clock)
            .with_default_script(DialogueScript::tokens(["x"]).failing_open(err.clone()));
        let mut rig = failing_turn(Some(dialogue), None);
        rig.wait_for("the fault", |e| matches!(e, AssistantEvent::Fault(_)));
        assert_eq!(rig.faults(), [AssistantFault::Dialogue(err)]);
        rig.stop();
    }
}

#[test]
fn a_synthesis_that_cannot_start_is_a_typed_fault() {
    let clock = prxvc_media::ManualClock::new();
    let err = TtsError::Unavailable {
        reason: "the voice model is not loaded".into(),
    };
    let tts = FakeSpeechSynth::new(clock)
        .with_default_script(TtsScript::chunks(1, 100).failing_open(err.clone()));
    let mut rig = failing_turn(None, Some(tts));
    rig.wait_for("the fault", |e| matches!(e, AssistantEvent::Fault(_)));
    assert_eq!(rig.faults(), [AssistantFault::Synthesis(err)]);
    assert_eq!(rig.out.chunks.load(Ordering::SeqCst), 0);
    rig.stop();
}

#[test]
fn a_synthesis_that_fails_after_the_first_audio_lets_what_was_made_play_out() {
    let clock = prxvc_media::ManualClock::new();
    let err = TtsError::Runaway {
        produced_secs: 30.0,
    };
    let tts = FakeSpeechSynth::new(clock)
        .with_default_script(TtsScript::chunks(3, 100).failing_after(1, err.clone()));
    let mut rig = failing_turn(None, Some(tts));
    rig.wait_for("the fault", |e| matches!(e, AssistantEvent::Fault(_)));
    assert_eq!(rig.faults(), [AssistantFault::Synthesis(err)]);
    assert_eq!(
        rig.out.flushes.load(Ordering::SeqCst),
        0,
        "no abrupt cut of what was already said"
    );
    rig.wait_until("listening again", |r| {
        r.phases().last() == Some(&Phase::Listening)
    });
    rig.stop();
}

#[test]
fn a_reply_with_nothing_to_say_aloud_is_a_fault_not_a_silent_turn() {
    let clock = prxvc_media::ManualClock::new();
    let dialogue =
        FakeDialogue::new(clock).with_default_script(DialogueScript::tokens(["...", " ..."]));
    let mut rig = failing_turn(Some(dialogue), None);
    rig.wait_for("the fault", |e| matches!(e, AssistantEvent::Fault(_)));
    assert_eq!(rig.faults(), [AssistantFault::NoAudio]);
    assert!(
        rig.tts.requests().is_empty(),
        "nothing speakable was sent to synthesis"
    );
    rig.stop();
}

#[test]
fn losing_the_microphone_ends_the_session_with_a_typed_fault() {
    let mut rig = Rig::started("hi", ["Hello!"]);
    rig.wait_phase(Phase::Listening);
    let lost = EndpointError::DeviceLost {
        device: "usb mic".into(),
    };
    rig.feed.fail_with(lost.clone());
    rig.wait_for("the end", |e| *e == AssistantEvent::Ended);
    assert_eq!(rig.faults(), [AssistantFault::Microphone(lost)]);
    assert_eq!(rig.phases().last(), Some(&Phase::Idle));
    assert!(
        rig.feed.stopped.load(Ordering::SeqCst),
        "the microphone is released"
    );
}

#[test]
fn a_failing_speech_detector_ends_the_session_and_says_where() {
    let (mut cfg, clock) = RigConfig::new("hi", ["Hello!"]);
    cfg.vad = vad_runs(&ONE_UTTERANCE).failing_at(3, VadError::Internal("boom".into()));
    let mut rig = Rig::build(cfg, clock);
    rig.start().expect("starts");
    rig.speak();
    rig.wait_for("the end", |e| *e == AssistantEvent::Ended);
    match rig.faults().as_slice() {
        [
            AssistantFault::Listening {
                stage: Stage::Vad,
                kind: FaultKind::Crashed,
                ..
            },
        ] => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_failing_recogniser_ends_the_session_and_says_where() {
    let (mut cfg, clock) = RigConfig::new("hi", ["Hello!"]);
    cfg.asr = AsrScript::new(["h"], "hi").failing_finalize(AsrError::Unavailable {
        reason: "stopped".into(),
    });
    let mut rig = Rig::build(cfg, clock);
    rig.start().expect("starts");
    rig.speak();
    rig.wait_for("the end", |e| *e == AssistantEvent::Ended);
    match rig.faults().as_slice() {
        [
            AssistantFault::Listening {
                stage: Stage::Asr, ..
            },
        ] => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_output_that_refuses_the_audio_ends_the_session() {
    let mut rig = Rig::started("hi", ["Hello!"]);
    rig.out.refuse.store(true, Ordering::SeqCst);
    rig.speak();
    rig.wait_for("the end", |e| *e == AssistantEvent::Ended);
    assert!(matches!(
        rig.faults().as_slice(),
        [AssistantFault::Output(_)]
    ));
}

struct NoRecogniser;

impl StreamingAsr for NoRecogniser {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new("none", "none", StreamingKind::Redecode)
    }

    fn open(&self, _lang: Lang, _ctx: AsrContext) -> Result<Box<dyn AsrStream>, AsrError> {
        Err(AsrError::Unavailable {
            reason: "the model is not installed".into(),
        })
    }
}

#[test]
fn a_session_that_cannot_start_reports_what_could_not_be_opened_and_leaves_nothing_open() {
    // The speech detector.
    let (mut cfg, clock) = RigConfig::new("hi", ["Hello!"]);
    cfg.vad = VadScript::default().failing_open(VadError::Unavailable {
        reason: "no model".into(),
    });
    let mut rig = Rig::build(cfg, clock);
    assert!(matches!(rig.start(), Err(StartError::Vad(_))));

    // The recogniser.
    let (cfg, clock) = RigConfig::new("hi", ["Hello!"]);
    let mut rig = Rig::build(cfg, clock);
    let mut deps = rig.take_deps();
    deps.asr = Arc::new(NoRecogniser);
    assert!(matches!(
        super::super::start(deps, rig.params()),
        Err(StartError::Recognition(_))
    ));

    // The microphone.
    let (cfg, clock) = RigConfig::new("hi", ["Hello!"]);
    let (mut source, feed) = super::rig::mic();
    source.fail_start(EndpointError::Unavailable {
        reason: "permission denied".into(),
    });
    let mut rig = Rig::build_with(cfg, clock, source, feed);
    assert!(matches!(rig.start(), Err(StartError::Microphone(_))));

    // A microphone at the wrong rate.
    let (cfg, clock) = RigConfig::new("hi", ["Hello!"]);
    let (mut source, feed) = super::rig::mic();
    source.set_rate(SampleRate::HZ_48000);
    let stopped = Arc::clone(&feed.stopped);
    let mut rig = Rig::build_with(cfg, clock, source, feed);
    assert!(matches!(rig.start(), Err(StartError::Rate { .. })));
    assert!(
        stopped.load(Ordering::SeqCst),
        "the microphone is closed again"
    );
}
