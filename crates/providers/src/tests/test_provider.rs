//! "Test provider" per role with the ports' fakes.

use std::time::Duration;

use prxvc_media::{Lang, ManualClock};
use prxvc_ports::fakes::{
    AsrScript, FakeRefineAsr, FakeSpeechSynth, FakeStreamingAsr, FakeTranslator, FakeVadProvider,
    MtScript, RefineScript, TtsScript, VadScript,
};
use prxvc_ports::{MtError, TtsError, VadError, VoiceSelection};

use crate::test_provider::*;

const T: Duration = Duration::from_secs(5);

fn clock() -> ManualClock {
    ManualClock::new()
}

#[test]
fn a_translation_test_reads_the_stream_and_reports_the_text() {
    let fake = FakeTranslator::new(clock()).with_default_script(MtScript::tokens(["你好", "，", "能听到吗？"]));
    let report = test_translator(&fake, Lang::En, Lang::Zh, T);
    assert!(report.ok, "{report:?}");
    assert_eq!(report.detail, "你好，能听到吗？");
    assert!(report.first_response.is_some());
    assert_eq!(fake.requests()[0].text, TEST_SENTENCE);
}

#[test]
fn translation_failures_are_classified() {
    for (error, class) in [
        (MtError::Rejected { status: 401 }, FailureClass::Unauthorized),
        (MtError::Rejected { status: 403 }, FailureClass::Forbidden),
        (MtError::Rejected { status: 404 }, FailureClass::NotFound),
        (MtError::Rejected { status: 429 }, FailureClass::RateLimited),
        (MtError::Rejected { status: 500 }, FailureClass::Rejected),
        (MtError::Timeout, FailureClass::Timeout),
        (MtError::Unavailable { reason: "cannot connect: refused".into() }, FailureClass::Unreachable),
        (MtError::Unavailable { reason: "connect timeout".into() }, FailureClass::Unreachable),
        (MtError::Unavailable { reason: "stream stalled".into() }, FailureClass::Unavailable),
    ] {
        let fake = FakeTranslator::new(clock())
            .with_default_script(MtScript::tokens(["x"]).failing_open(error.clone()));
        let report = test_translator(&fake, Lang::En, Lang::Zh, T);
        assert!(!report.ok);
        assert_eq!(report.error.unwrap().class, class, "{error:?}");
    }
}

#[test]
fn an_empty_translation_is_a_bad_response() {
    let fake = FakeTranslator::new(clock()).with_default_script(MtScript::tokens([" "]));
    let report = test_translator(&fake, Lang::En, Lang::Zh, T);
    assert_eq!(report.error.unwrap().class, FailureClass::BadResponse);
}

#[test]
fn a_synthesis_test_discards_the_audio_and_reports_the_duration() {
    let fake = FakeSpeechSynth::new(clock()).with_default_script(TtsScript::chunks(3, 2400));
    let report = test_synth(&fake, Lang::En, VoiceSelection::Preset { name: "preset-a".into() }, T);
    assert!(report.ok, "{report:?}");
    assert!(report.first_response.is_some());
    assert_eq!(fake.requests()[0].text, TEST_CLAUSE);
    let failing = FakeSpeechSynth::new(clock())
        .with_default_script(TtsScript::chunks(1, 10).failing_open(TtsError::UnknownVoice("preset-a".into())));
    let report = test_synth(&failing, Lang::En, VoiceSelection::Preset { name: "preset-a".into() }, T);
    assert_eq!(report.error.unwrap().class, FailureClass::UnknownVoice);
}

#[test]
fn a_recognition_test_expects_silence_to_decode_to_nothing() {
    let quiet = FakeStreamingAsr::new(clock(), AsrScript::new([], ""));
    let report = test_recogniser(&quiet, Lang::En, T);
    assert!(report.ok, "{report:?}");
    let noisy = FakeStreamingAsr::new(clock(), AsrScript::new([], "thank you"));
    let report = test_recogniser(&noisy, Lang::En, T);
    assert_eq!(report.error.unwrap().class, FailureClass::BadResponse);

    let refine = FakeRefineAsr::new(clock(), RefineScript::new("", Duration::ZERO));
    assert!(test_refiner(&refine, Lang::Zh, T).ok);
    let refine = FakeRefineAsr::new(clock(), RefineScript::new("x", Duration::ZERO));
    assert!(!test_refiner(&refine, Lang::Zh, T).ok);
}

#[test]
fn a_vad_test_loads_the_engine() {
    let vad = FakeVadProvider::new(VadScript::probabilities([0.0]));
    let report = test_vad(&vad);
    assert!(report.ok, "{report:?}");
    assert_eq!(vad.opens(), 1);
    let broken = FakeVadProvider::new(VadScript::probabilities([0.0]).failing_open(VadError::Unavailable { reason: "no model".into() }));
    assert_eq!(test_vad(&broken).error.unwrap().class, FailureClass::Unavailable);
}
