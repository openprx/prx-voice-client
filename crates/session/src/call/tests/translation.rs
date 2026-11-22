//! Translated calls: the start order, provider problems, the outgoing flow from microphone frames to the virtual microphone, what each listener hears, pause, failure and recovery, and releasing everything on stop.

use std::time::Duration;

use prxvc_media::{AudioFrame, ClauseId, Direction, MonoTime, SampleRate, SegmentId};
use prxvc_ports::fakes::{AsrScript, MtScript, TtsScript, VadScript};
use prxvc_ports::{MtError, TraceStage};

use super::kit::{Played, Rig, steps};
use crate::call::{
    CallCommand, CallErrorCode, HostFault, LocalOutputMode, PlaybackReport, ProviderIssue,
};
use crate::call_state::CallState;
use crate::dir_set::DirSet;
use crate::direction_state::DirectionState;
use crate::fault::ProviderRole;
use crate::output::{OutputCause, OutputSource};
use crate::segment::ClauseState;

pub(super) const SOURCE_TEXT: &str = "今天天气很好";
/// What the remote party says in the incoming direction (English, translated into Chinese).
const INCOMING_TEXT: &str = "The weather is very nice today";

pub(super) fn clause(direction: Direction, seg: u64, index: u16) -> ClauseId {
    ClauseId::new(SegmentId::new(direction, seg).expect("seq >= 1"), index)
}

/// One utterance: 10 frames of silence, 20 of speech, 25 of silence (a segment end).
pub(super) fn one_utterance() -> VadScript {
    VadScript::probabilities(
        std::iter::repeat_n(0.0, 10)
            .chain(std::iter::repeat_n(0.9, 20))
            .chain(std::iter::repeat_n(0.0, 25))
            .chain(std::iter::repeat_n(0.0, 4000)),
    )
}

/// Each direction says one clause in its own language (`SOURCE_TEXT` outgoing, `INCOMING_TEXT`
/// incoming) and gets a two-chunk translation back.
pub(super) fn script_one_utterance(rig: &Rig) {
    rig.world.with_translation(|t| {
        for d in [Direction::Outgoing, Direction::Incoming] {
            t.vad.insert(d, one_utterance());
            let said = match d {
                Direction::Outgoing => SOURCE_TEXT,
                Direction::Incoming => INCOMING_TEXT,
            };
            t.asr.insert(d, AsrScript::new([], said).then_round([], ""));
        }
        t.mt = std::mem::replace(
            &mut t.mt,
            prxvc_ports::fakes::FakeTranslator::new(rig.clock.clone()),
        )
        .with_script(SOURCE_TEXT, MtScript::tokens(["Nice ", "weather."]))
        .with_script(INCOMING_TEXT, MtScript::tokens(["今天", "天气很好。"]))
        .with_script("Hello.", MtScript::tokens(["Hello."]));
        t.tts = prxvc_ports::fakes::FakeSpeechSynth::new(rig.clock.clone())
            .with_script("Nice weather.", TtsScript::chunks(2, 2400))
            .with_script("今天天气很好。", TtsScript::chunks(2, 2400))
            .with_script("Hello.", TtsScript::chunks(1, 2400));
    });
}

pub(super) fn translating_both(rig: &mut Rig) {
    rig.until("both directions translate", |s| {
        s.directions
            .iter()
            .all(|d| d.hears == OutputSource::Translation)
    });
}

/// Push `n` frames of 32 ms at 16 kHz into the path called `name`, as its source would.
pub(super) fn speak(rig: &Rig, name: &'static str, n: u64) {
    for i in 0..n {
        let frame = AudioFrame::new(
            vec![0.1; 512],
            SampleRate::HZ_16000,
            MonoTime::from_millis(i * 32),
            i,
        )
        .expect("a valid frame");
        rig.world.push_frame(name, frame);
    }
}

#[test]
fn a_translated_start_runs_the_checks_then_the_paths_then_the_providers_then_connects_the_pipelines()
 {
    let mut rig = Rig::new();
    rig.start_translating();
    translating_both(&mut rig);
    let log = rig.log();
    let steps = steps(&log);
    let pos = |name: &str| {
        steps
            .iter()
            .position(|s| *s == name)
            .unwrap_or_else(|| panic!("{name} missing in {steps:?}"))
    };
    for pair in [
        ("bundle", "mic_consent"),
        ("mic_consent", "capture_consent"),
        ("capture_consent", "driver"),
        ("mic_check", "start_outgoing"),
        ("start_outgoing", "enter"),
        ("enter", "start_incoming"),
        ("start_incoming", "probe"),
        ("probe", "providers"),
    ] {
        assert!(
            pos(pair.0) < pos(pair.1),
            "{pair:?} out of order: {steps:?}"
        );
    }
    let at = |entry: &str| log.iter().position(|e| e == entry).expect(entry);
    assert!(at("providers") < at("outgoing.connect"));
    assert!(at("providers") < at("incoming.connect"));
    assert_eq!(rig.last().state, CallState::Translating);
    assert_eq!(
        rig.world.tap_rate("outgoing"),
        Some(SampleRate::HZ_16000),
        "the pipeline gets the VAD's rate"
    );
    assert!(rig.world.live_providers() > 0);
}

#[test]
fn provider_problems_become_typed_issues_and_the_call_stays_audio_only() {
    let mut rig = Rig::new();
    rig.world.fail_providers_with(
        ProviderRole::Asr,
        vec![
            HostFault::new(
                CallErrorCode::Provider(ProviderIssue::ModelNotInstalled),
                "qwen3-asr-0.6b-q8",
            ),
            HostFault::new(
                CallErrorCode::Provider(ProviderIssue::ModelNotInstalled),
                "qwen3-asr-1.7b-q8",
            ),
            HostFault::new(
                CallErrorCode::Provider(ProviderIssue::ServerUnreachable),
                "http://127.0.0.1:8080/v1",
            ),
        ],
    );
    rig.start_translating();
    let s = rig.last();
    assert!(
        matches!(s.state, CallState::Degraded { .. }),
        "{:?}",
        s.state
    );
    let provider_issues: Vec<_> = s
        .issues
        .iter()
        .filter(|i| matches!(i.code, CallErrorCode::Provider(_)))
        .map(|i| (i.code, i.detail.as_str()))
        .collect();
    assert_eq!(
        provider_issues,
        [
            (
                CallErrorCode::Provider(ProviderIssue::ModelNotInstalled),
                "qwen3-asr-0.6b-q8"
            ),
            (
                CallErrorCode::Provider(ProviderIssue::ModelNotInstalled),
                "qwen3-asr-1.7b-q8"
            ),
            (
                CallErrorCode::Provider(ProviderIssue::ServerUnreachable),
                "http://127.0.0.1:8080/v1"
            ),
        ]
    );
    // Both paths run and the listeners hear the original (the D-P3 default); nothing connected.
    assert!(s.direction(Direction::Outgoing).path.is_some());
    assert!(s.direction(Direction::Incoming).path.is_some());
    assert_eq!(rig.hears(Direction::Outgoing), OutputSource::Original);
    assert_eq!(rig.hears(Direction::Incoming), OutputSource::Original);
    assert!(!rig.world.is_connected("outgoing"));
    assert!(!rig.log().contains(&"outgoing.connect".to_owned()));
}

#[test]
fn an_audio_only_start_never_touches_providers_or_pipelines() {
    let mut rig = Rig::new();
    rig.start();
    let log = rig.log();
    assert!(!log.contains(&"providers".to_owned()));
    assert!(!log.iter().any(|e| e.ends_with(".connect")));
    assert_eq!(rig.last().state, CallState::AudioReady { translate: false });
    assert!(rig.last().directions.iter().all(|d| d.pipeline.is_none()));
    assert_eq!(rig.world.live_providers(), 0);
}

#[test]
fn an_utterance_flows_from_microphone_frames_to_the_translated_lane_and_back_as_playback_reports() {
    let mut rig = Rig::new();
    script_one_utterance(&rig);
    rig.world.translate_only(DirSet::one(Direction::Outgoing));
    rig.start_translating();
    rig.until("outgoing translates", |s| {
        s.direction(Direction::Outgoing).hears == OutputSource::Translation
    });
    assert_eq!(
        rig.hears(Direction::Incoming),
        OutputSource::Original,
        "a direction without a pipeline keeps the original"
    );
    assert!(!rig.world.is_connected("incoming"));

    speak(&rig, "outgoing", 55);
    let c0 = clause(Direction::Outgoing, 1, 0);
    rig.until_world("the clause reached the mixer lane", |w| {
        w.played("outgoing").contains(&Played::End(c0))
    });
    assert_eq!(
        rig.world
            .played("outgoing")
            .iter()
            .filter(|p| matches!(p, Played::Chunk(c, 2400) if *c == c0))
            .count(),
        2
    );

    // The mixer reports the clause's playback; the pipeline moves it to Played.
    rig.world.report_playback(
        "outgoing",
        PlaybackReport::Started {
            clause: c0,
            at: MonoTime::from_millis(2_000),
        },
    );
    rig.world.report_playback(
        "outgoing",
        PlaybackReport::Finished {
            clause: c0,
            at: MonoTime::from_millis(2_200),
        },
    );
    let s = rig.until("the clause is played", |s| {
        s.direction(Direction::Outgoing)
            .pipeline
            .as_ref()
            .is_some_and(|p| p.clauses.iter().any(|c| c.state == ClauseState::Played))
    });
    let pipeline = s
        .direction(Direction::Outgoing)
        .pipeline
        .clone()
        .expect("pipeline snapshot");
    assert_eq!(pipeline.clauses[0].source_text, SOURCE_TEXT);
    assert_eq!(pipeline.clauses[0].translation, "Nice weather.");
    let stages: Vec<TraceStage> = rig
        .world
        .trace()
        .events()
        .into_iter()
        .filter(|e| e.clause == c0)
        .map(|e| e.stage)
        .collect();
    for needed in [
        TraceStage::SpeechStart,
        TraceStage::MtFirstToken,
        TraceStage::TtsFirstChunk,
        TraceStage::PlayoutStart,
        TraceStage::PlayoutEnd,
    ] {
        assert!(stages.contains(&needed), "{needed:?} missing in {stages:?}");
    }
    // The first change announced the translation; nothing else changed what the remote party hears.
    let hears: Vec<_> = rig
        .log()
        .into_iter()
        .filter(|e| e.starts_with("outgoing.hears:"))
        .collect();
    assert_eq!(
        hears.last().map(String::as_str),
        Some("outgoing.hears:Translation")
    );
}

#[test]
fn a_cancelled_segment_flushes_the_translated_lane_through_the_path() {
    let mut rig = Rig::new();
    script_one_utterance(&rig);
    rig.world.translate_only(DirSet::one(Direction::Outgoing));
    rig.start_translating();
    rig.until("outgoing translates", |s| {
        s.direction(Direction::Outgoing).hears == OutputSource::Translation
    });
    speak(&rig, "outgoing", 55);
    let c0 = clause(Direction::Outgoing, 1, 0);
    rig.until_world("audio arrived", |w| {
        w.played("outgoing").contains(&Played::End(c0))
    });
    rig.coord.handle(CallCommand::Skip(Direction::Outgoing));
    rig.until_world("the lane was flushed", |w| {
        w.played("outgoing").contains(&Played::Flush)
    });
}

#[test]
fn skip_without_a_pipeline_is_rejected() {
    let mut rig = Rig::new();
    rig.start();
    rig.coord.handle(CallCommand::Skip(Direction::Incoming));
    assert!(rig.has_issue(CallErrorCode::CommandRejected));
}

#[test]
fn a_translation_failure_plays_the_original_announced_and_the_translation_returns_after_recovery() {
    let mut rig = Rig::new();
    script_one_utterance(&rig);
    rig.world.with_translation(|t| {
        t.mt = std::mem::replace(
            &mut t.mt,
            prxvc_ports::fakes::FakeTranslator::new(rig.clock.clone()),
        )
        .with_script(
            SOURCE_TEXT,
            MtScript::tokens([]).failing_open(MtError::Unavailable {
                reason: "cannot connect (scripted)".to_owned(),
            }),
        );
    });
    rig.world.translate_only(DirSet::one(Direction::Outgoing));
    rig.start_translating();
    rig.until("outgoing translates", |s| {
        s.direction(Direction::Outgoing).hears == OutputSource::Translation
    });
    speak(&rig, "outgoing", 55);
    let s = rig.until("the remote party hears the original again", |s| {
        s.direction(Direction::Outgoing).hears == OutputSource::Original
    });
    assert!(
        matches!(s.state, CallState::Degraded { .. }),
        "{:?}",
        s.state
    );
    let change = s
        .last_output_change
        .clone()
        .expect("the change is announced");
    assert_eq!(
        (change.direction, change.to, change.cause),
        (
            Direction::Outgoing,
            OutputSource::Original,
            OutputCause::Failed
        )
    );
    assert!(rig.log().contains(&"outgoing.hears:Original".to_owned()));
    assert!(matches!(
        s.direction(Direction::Outgoing).state,
        DirectionState::Failed { .. }
    ));

    // The probe succeeds after the first wait: the pipeline recovers by itself.
    rig.clock.advance(Duration::from_secs(3));
    let s = rig.until("the translation is back", |s| {
        s.direction(Direction::Outgoing).hears == OutputSource::Translation
            && s.state == CallState::Translating
    });
    assert_eq!(
        s.last_output_change.map(|c| c.cause),
        Some(OutputCause::TranslationStarted),
        "a failed direction starts again"
    );
}

#[test]
fn pause_and_resume_drive_the_pipeline_and_the_passthrough_policy_is_announced() {
    let mut rig = Rig::new();
    rig.start_translating();
    translating_both(&mut rig);
    rig.coord
        .handle(CallCommand::Pause(DirSet::one(Direction::Outgoing)));
    let s = rig.until("the paused direction plays the original", |s| {
        s.direction(Direction::Outgoing).hears == OutputSource::Original
    });
    assert!(matches!(
        s.direction(Direction::Outgoing)
            .pipeline
            .as_ref()
            .map(|p| p.state.clone()),
        Some(DirectionState::Paused { .. })
    ));
    assert_eq!(
        s.direction(Direction::Incoming).hears,
        OutputSource::Translation,
        "the other direction is untouched"
    );
    rig.coord.handle(CallCommand::Resume);
    rig.until("the translation resumes", |s| {
        s.direction(Direction::Outgoing).hears == OutputSource::Translation
    });
}

#[test]
fn both_directions_translate_at_once_without_touching_each_other() {
    let mut rig = Rig::new();
    script_one_utterance(&rig);
    rig.start_translating();
    translating_both(&mut rig);
    speak(&rig, "outgoing", 55);
    speak(&rig, "incoming", 55);
    let out = clause(Direction::Outgoing, 1, 0);
    let inc = clause(Direction::Incoming, 1, 0);
    rig.until_world("both clauses reached their lanes", |w| {
        w.played("outgoing").contains(&Played::End(out))
            && w.played("incoming").contains(&Played::End(inc))
    });
    assert!(
        rig.world
            .played("outgoing")
            .iter()
            .all(|p| !matches!(p, Played::Chunk(c, _) | Played::End(c) if c.segment().direction() != Direction::Outgoing)),
        "the outgoing lane carries only outgoing clauses"
    );
}

#[test]
fn speech_already_in_the_listeners_language_is_heard_as_the_original_and_the_translation_returns() {
    let mut rig = Rig::new();
    script_one_utterance(&rig);
    // The remote party speaks Chinese to a Chinese listener: nothing to translate.
    rig.world.with_translation(|t| {
        t.asr.insert(
            Direction::Incoming,
            AsrScript::new([], SOURCE_TEXT).then_round([], ""),
        );
    });
    rig.start_translating();
    translating_both(&mut rig);
    speak(&rig, "incoming", 55);
    rig.until_world("the original is heard and the translation returns", |w| {
        w.log()
            .iter()
            .filter(|l| *l == "incoming.hears:Translation")
            .count()
            >= 2
    });
    let heard: Vec<String> = rig
        .log()
        .into_iter()
        .filter(|l| l.starts_with("incoming.hears:"))
        .collect();
    assert_eq!(
        heard[heard.len() - 3..],
        [
            "incoming.hears:Translation",
            "incoming.hears:Original",
            "incoming.hears:Translation"
        ]
    );
    assert!(
        rig.world.played("incoming").is_empty(),
        "nothing was synthesised"
    );
    let s = rig.until("incoming hears the translation again", |s| {
        s.direction(Direction::Incoming).hears == OutputSource::Translation
    });
    let pipeline = s.direction(Direction::Incoming).pipeline.as_ref().unwrap();
    assert_eq!(pipeline.counters.in_target_passed, 1);
}

#[test]
fn the_local_output_mode_reaches_the_incoming_path_only() {
    let mut rig = Rig::new();
    rig.start_translating();
    translating_both(&mut rig);
    rig.coord
        .handle(CallCommand::SetLocalOutputMode(LocalOutputMode::Both));
    rig.coord
        .handle(CallCommand::SetLocalOutputMode(LocalOutputMode::Original));
    assert_eq!(
        rig.world.modes("incoming"),
        [LocalOutputMode::Both, LocalOutputMode::Original]
    );
    assert!(rig.world.modes("outgoing").is_empty());
    assert_eq!(rig.last().local_output_mode, LocalOutputMode::Original);
}

#[test]
fn a_refused_connection_applies_the_failure_policy_for_that_direction() {
    let mut rig = Rig::new();
    rig.world.connect_fails("outgoing");
    rig.start_translating();
    rig.until("incoming translates", |s| {
        s.direction(Direction::Incoming).hears == OutputSource::Translation
    });
    let s = rig.last();
    assert_eq!(
        s.direction(Direction::Outgoing).hears,
        OutputSource::Original
    );
    assert!(rig.has_issue(CallErrorCode::TranslationUnavailable));
    assert!(s.direction(Direction::Outgoing).pipeline.is_none());
}

#[test]
fn stop_from_translating_flushes_stops_the_pipelines_restores_routing_and_releases_the_providers() {
    let mut rig = Rig::new();
    script_one_utterance(&rig);
    rig.start_translating();
    translating_both(&mut rig);
    speak(&rig, "outgoing", 55);
    let c0 = clause(Direction::Outgoing, 1, 0);
    rig.until_world("audio arrived", |w| {
        w.played("outgoing").contains(&Played::End(c0))
    });
    rig.world.clear_log();
    rig.stop();
    let s = rig.last();
    assert_eq!(s.state, CallState::Idle);
    let log = rig.log();
    let at = |entry: &str| {
        log.iter()
            .position(|e| e == entry)
            .unwrap_or_else(|| panic!("{entry} missing in {log:?}"))
    };
    assert!(at("outgoing.disconnect") < at("outgoing.stop"), "{log:?}");
    assert!(
        at("leave") < at("incoming.stop"),
        "routing is restored before the paths close"
    );
    assert!(
        at("incoming.stop") < at("outgoing.stop"),
        "the feeder closes last: {log:?}"
    );
    assert!(rig.world.played("outgoing").contains(&Played::Flush));
    assert!(!rig.world.in_call_mode());
    assert_eq!(
        rig.world.live_providers(),
        0,
        "no provider is kept after the stop"
    );
    assert!(s.directions.iter().all(|d| d.pipeline.is_none()));
    assert!(!rig.world.is_connected("outgoing") && !rig.world.is_connected("incoming"));
}

#[test]
fn stop_while_providers_failed_and_while_paused_and_while_recovering_leaves_nothing_behind() {
    // Degraded after a provider failure.
    let mut rig = Rig::new();
    rig.world.fail_providers(ProviderRole::Tts);
    rig.start_translating();
    rig.stop();
    assert_eq!(rig.last().state, CallState::Idle);
    assert_eq!(rig.world.live_providers(), 0);

    // Paused.
    let mut rig = Rig::new();
    rig.start_translating();
    translating_both(&mut rig);
    rig.coord.handle(CallCommand::Pause(DirSet::BOTH));
    rig.until("paused", |s| matches!(s.state, CallState::Paused { .. }));
    rig.stop();
    assert_eq!(rig.last().state, CallState::Idle);
    assert_eq!(rig.world.live_providers(), 0);
    assert!(!rig.world.in_call_mode());

    // Recovering after a lost endpoint, then stop.
    let mut rig = Rig::new();
    rig.start_translating();
    translating_both(&mut rig);
    rig.world
        .fail_always("start_incoming", CallErrorCode::TargetNotRunning);
    rig.world.push_path_event(
        "incoming",
        crate::call::PathEvent::Lost(HostFault::new(
            CallErrorCode::EndpointLost(crate::fault::Endpoint::RemoteInput),
            "tap lost (scripted)",
        )),
    );
    rig.until("recovering", |s| {
        matches!(s.state, CallState::Recovering { .. })
    });
    rig.stop();
    assert_eq!(rig.last().state, CallState::Idle);
    assert_eq!(rig.world.live_providers(), 0);
}

#[test]
fn a_replaced_path_is_connected_to_its_running_pipeline_again() {
    let mut rig = Rig::new();
    rig.start_translating();
    translating_both(&mut rig);
    rig.world.push_path_event(
        "incoming",
        crate::call::PathEvent::Lost(HostFault::new(
            CallErrorCode::EndpointLost(crate::fault::Endpoint::RemoteInput),
            "tap lost (scripted)",
        )),
    );
    rig.coord.tick();
    rig.tick_after(2);
    rig.until_world("the path was reopened and connected again", |w| {
        w.log().iter().filter(|e| *e == "incoming.connect").count() >= 2
    });
    assert!(rig.world.is_connected("incoming"));
}

#[test]
fn the_incoming_path_is_told_whether_the_call_translates_at_start_and_after_a_reopen() {
    let mut rig = Rig::new();
    rig.start();
    assert!(rig.log().contains(&"incoming.mode:audio_only".to_owned()));
    rig.stop();

    let mut rig = Rig::new();
    rig.start_translating();
    translating_both(&mut rig);
    assert!(rig.log().contains(&"incoming.mode:translated".to_owned()));
    rig.world.clear_log();
    rig.world.push_path_event(
        "incoming",
        crate::call::PathEvent::Lost(HostFault::new(
            CallErrorCode::EndpointLost(crate::fault::Endpoint::RemoteInput),
            "tap lost (scripted)",
        )),
    );
    rig.until_world("the path was reopened", |w| {
        w.log().contains(&"incoming.mode:translated".to_owned())
    });
}

#[test]
fn translation_on_the_built_in_speakers_shows_the_headphone_warning_until_the_route_changes() {
    use crate::call::{OutputRoute, PathStats};
    let stats = |route| PathStats {
        sink_route: route,
        sink_device: Some("MacBook Pro Speakers".to_owned()),
        ..PathStats::default()
    };

    let mut rig = Rig::new();
    rig.world
        .set_path_stats("incoming", stats(OutputRoute::Speakers));
    rig.start_translating();
    translating_both(&mut rig);
    rig.coord.tick();
    assert!(rig.has_issue(CallErrorCode::SpeakerOutput));
    let detail = rig
        .last()
        .issues
        .iter()
        .find(|i| i.code == CallErrorCode::SpeakerOutput)
        .map(|i| i.detail.clone())
        .expect("the warning");
    assert!(detail.contains("MacBook Pro Speakers"), "{detail}");

    rig.world
        .set_path_stats("incoming", stats(OutputRoute::Headphones));
    rig.coord.tick();
    assert!(!rig.has_issue(CallErrorCode::SpeakerOutput));
}

#[test]
fn an_audio_only_call_on_the_speakers_shows_no_warning_because_nothing_is_played_by_us() {
    use crate::call::{OutputRoute, PathStats};
    let mut rig = Rig::new();
    rig.world.set_path_stats(
        "incoming",
        PathStats {
            sink_route: OutputRoute::Speakers,
            ..PathStats::default()
        },
    );
    rig.start();
    rig.coord.tick();
    assert!(!rig.has_issue(CallErrorCode::SpeakerOutput));
}
