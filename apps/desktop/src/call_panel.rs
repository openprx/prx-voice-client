//! Maps the coordinator's snapshot into the debug control's view model: enums, stable i18n keys for issues, numbers for levels and counters.

use prxvc_desktop_ui::call_view::{
    CaptionFlag, CaptionView, ClauseStatus, CountersView, LatencyView, NoticeState, NoticeView,
    PartialView, SkipView,
};
use prxvc_desktop_ui::call_view::IssueAction;
use prxvc_desktop_ui::{
    ActivityView, CallPanel, CallPhase, Heard, IssueSeverity, IssueView, LaneView,
};
use prxvc_media::Direction;
use prxvc_ports::MtFlag;
use prxvc_session::ClauseState;
use prxvc_session::pipeline::SkipReason;
use prxvc_session::call::{
    ActivityJudgement, CallErrorCode, CallIssue, CallSnapshot, DirectionSnapshot,
    DisclosureSnapshot, DisclosureState, ProviderIssue, Severity,
};
use prxvc_session::{CallState, OutputSource};

use crate::call_kit::trace::{LastClause, TraceSlot};

/// The i18n key of the message for an error code.
pub(crate) fn issue_key(code: CallErrorCode) -> &'static str {
    match code {
        CallErrorCode::NotInBundle => "call.issue.not_in_bundle",
        CallErrorCode::NoCallApp => "call.issue.no_call_app",
        CallErrorCode::MicrophoneConsentDenied => "call.issue.mic_consent_denied",
        CallErrorCode::MicrophoneConsentUnavailable => "call.issue.mic_consent_unavailable",
        CallErrorCode::CaptureConsentDenied => "call.issue.capture_consent_denied",
        CallErrorCode::CaptureConsentUnavailable => "call.issue.capture_consent_unavailable",
        CallErrorCode::DriverMissing => "call.issue.driver_missing",
        CallErrorCode::DriverNotLoaded => "call.issue.driver_not_loaded",
        CallErrorCode::DriverStale => "call.issue.driver_stale",
        CallErrorCode::DriverWrongVariant => "call.issue.driver_wrong_variant",
        CallErrorCode::DriverUnusable => "call.issue.driver_unusable",
        CallErrorCode::NoMicrophone => "call.issue.no_microphone",
        CallErrorCode::EndpointOpenFailed(_) => "call.issue.endpoint_open_failed",
        CallErrorCode::TargetNotRunning => "call.issue.target_not_running",
        CallErrorCode::RoutingJournalPresent => "call.issue.routing_journal_present",
        CallErrorCode::RoutingSwitchFailed => "call.issue.routing_switch_failed",
        CallErrorCode::RoutingUnavailable => "call.issue.routing_unavailable",
        CallErrorCode::VirtualMicrophoneLost => "call.issue.virtual_mic_lost",
        CallErrorCode::DefaultInputDisplaced => "call.issue.default_input_displaced",
        CallErrorCode::CaptureNoSignal => "call.issue.capture_no_signal",
        CallErrorCode::CaptureStalled => "call.issue.capture_stalled",
        CallErrorCode::TargetGone => "call.issue.target_gone",
        CallErrorCode::EndpointLost(_) => "call.issue.endpoint_lost",
        CallErrorCode::MicrophoneUnavailable => "call.issue.microphone_unavailable",
        CallErrorCode::MicrophoneChanged => "call.issue.microphone_changed",
        CallErrorCode::RecoveryTimeout => "call.issue.recovery_timeout",
        CallErrorCode::CallOnOtherMicrophone => "call.issue.call_on_other_microphone",
        CallErrorCode::ActivityUnavailable => "call.issue.activity_unavailable",
        CallErrorCode::DirectionDegraded => "call.issue.direction_degraded",
        CallErrorCode::LocalOutputUnavailable => "call.issue.local_output_unavailable",
        CallErrorCode::RestoreFailed => "call.issue.restore_failed",
        CallErrorCode::StopLeak => "call.issue.stop_leak",
        CallErrorCode::CommandRejected => "call.issue.command_rejected",
        CallErrorCode::AssistantActive => "call.issue.assistant_active",
        CallErrorCode::TranslationUnavailable => "call.issue.translation_unavailable",
        CallErrorCode::Provider(issue) => provider_key(issue),
        CallErrorCode::SpeakerOutput => "call.issue.speaker_output",
        CallErrorCode::DisclosureUnavailable => "call.issue.disclosure_unavailable",
        _ => "call.issue.internal",
    }
}

/// The i18n key of what the user must do about a provider problem.
fn provider_key(issue: ProviderIssue) -> &'static str {
    match issue {
        ProviderIssue::ModelNotInstalled => "call.issue.provider.model_not_installed",
        ProviderIssue::ModelInstalling => "call.issue.provider.model_installing",
        ProviderIssue::ModelDamaged => "call.issue.provider.model_damaged",
        ProviderIssue::RuntimeNotBuiltIn => "call.issue.provider.runtime_not_built_in",
        ProviderIssue::VoiceProfileMissing => "call.issue.provider.voice_profile_missing",
        ProviderIssue::VoiceNotPrepared => "call.issue.provider.voice_not_prepared",
        ProviderIssue::VoicePresetMissing => "call.issue.provider.voice_preset_missing",
        ProviderIssue::ServerUnreachable => "call.issue.provider.server_unreachable",
        ProviderIssue::Unsupported => "call.issue.provider.unsupported",
        ProviderIssue::Offline => "call.issue.provider.offline",
        ProviderIssue::Setting => "call.issue.provider.setting",
        ProviderIssue::LanguagePackMissing => "call.issue.provider.language_pack_missing",
        ProviderIssue::OsTooOld => "call.issue.provider.os_too_old",
        ProviderIssue::ServerFailed => "call.issue.provider.server_failed",
        _ => "call.issue.provider.start_failed",
    }
}

fn phase(state: &CallState) -> CallPhase {
    match state {
        CallState::Idle => CallPhase::Idle,
        CallState::Initializing { .. } => CallPhase::Initializing,
        CallState::AudioReady { .. } => CallPhase::AudioReady,
        CallState::Translating => CallPhase::Translating,
        CallState::Paused { .. } => CallPhase::Paused,
        CallState::Degraded { .. } => CallPhase::Degraded,
        CallState::Recovering { .. } => CallPhase::Recovering,
        CallState::Stopping => CallPhase::Stopping,
    }
}

fn heard(source: OutputSource) -> Heard {
    match source {
        OutputSource::Translation => Heard::Translation,
        OutputSource::Original => Heard::Original,
        OutputSource::Nothing => Heard::Nothing,
    }
}

fn clause_status(state: &ClauseState) -> ClauseStatus {
    match state {
        ClauseState::Committed => ClauseStatus::Committed,
        ClauseState::Recognized => ClauseStatus::Recognized,
        ClauseState::Translating => ClauseStatus::Translating,
        ClauseState::Translated => ClauseStatus::Translated,
        ClauseState::Synthesizing => ClauseStatus::Synthesizing,
        ClauseState::Playing => ClauseStatus::Playing,
        ClauseState::Played => ClauseStatus::Played,
        ClauseState::Cancelled => ClauseStatus::Cancelled,
        ClauseState::Skipped => ClauseStatus::Skipped,
        ClauseState::Failed(_) => ClauseStatus::Failed,
    }
}

fn caption_flag(flag: MtFlag) -> CaptionFlag {
    match flag {
        MtFlag::WrongLanguage => CaptionFlag::WrongLanguage,
        MtFlag::TooLong => CaptionFlag::TooLong,
        MtFlag::Truncated => CaptionFlag::Truncated,
        MtFlag::Refusal => CaptionFlag::Refusal,
        MtFlag::AnswerLike => CaptionFlag::AnswerLike,
        MtFlag::ThinkResidue => CaptionFlag::ThinkResidue,
    }
}

fn skip_view(skipped: Option<SkipReason>) -> SkipView {
    match skipped {
        None => SkipView::None,
        Some(SkipReason::SourceInTarget) => SkipView::AlreadyInTarget,
        Some(SkipReason::SelfEcho) => SkipView::OwnEcho,
        Some(_) => SkipView::NotSpoken,
    }
}

fn lane(d: &DirectionSnapshot, latency: Option<LastClause>) -> LaneView {
    let mut view = LaneView {
        hears: heard(d.hears),
        ..LaneView::default()
    };
    if let Some(path) = &d.path {
        view.running = path.running;
        view.device = path.source_device.clone();
        view.frames = path.frames;
        view.rms = path.rms;
        view.peak = path.peak;
        view.overruns = path.overruns;
        view.drops = path.drops;
        view.gaps = path.translated_underruns;
        view.gap_ms = path.translated_gap_ms;
        view.translated_peak = path.translated_peak;
        view.translated_rms = path.translated_rms;
        view.input_gated = path.input_gated;
    }
    if let Some(p) = &d.pipeline {
        view.translating = true;
        view.languages = d.languages.map(|(a, b)| (a.code().to_owned(), b.code().to_owned()));
        view.partial = p.partial.as_ref().map(|x| PartialView {
            utterance: x.segment.seq(),
            text: x.text.clone(),
            stable_len: x.stable_prefix_len,
        });
        view.clauses = p
            .clauses
            .iter()
            .map(|c| CaptionView {
                utterance: c.clause.segment().seq(),
                index: c.clause.index(),
                source: c.source_text.clone(),
                translation: c.translation.clone(),
                status: if c.skipped.is_some() {
                    ClauseStatus::Skipped
                } else {
                    clause_status(&c.state)
                },
                skip: skip_view(c.skipped),
                flags: c.flags.iter().copied().map(caption_flag).collect(),
                retried: c.retried,
            })
            .collect();
        let c = &p.counters;
        // Gaps in the translated speech only: a dry sink or capture lane is not heard as one.
        let underruns = d.path.as_ref().map_or(0, |x| x.translated_underruns);
        view.counters = Some(CountersView {
            late_dropped: c.late_dropped,
            backlog_skips: c.backlog_skips,
            stale_skips: c.stale_skips,
            forced_cuts: c.forced_cuts,
            final_rewrites: c.final_rewrites,
            mt_suspect: c.mt_suspect,
            runaway_synth: c.runaway_synth,
            capture_overruns: c.capture_overruns,
            underruns,
            refine_fallbacks: c.refine_fallbacks,
            filler_dropped: c.filler_dropped,
            unspeakable_dropped: c.unspeakable_dropped,
            in_target_passed: c.in_target_passed,
            mt_retries: c.mt_retries,
            flags: MtFlag::ALL.map(|f| c.flag(f)),
        });
        view.latency = latency.map(|l| LatencyView {
            speech_start_to_first_audio_ms: l.speech_start_to_first_audio_ms,
            pause_to_first_audio_ms: l.clause_pause_to_first_audio_ms,
            stages: l.stages,
        });
    }
    view
}

/// What the call window offers next to an issue.
fn action_of(code: CallErrorCode) -> IssueAction {
    match code {
        CallErrorCode::AssistantActive => IssueAction::EndAssistantAndStart,
        CallErrorCode::MicrophoneConsentDenied
        | CallErrorCode::MicrophoneConsentUnavailable
        | CallErrorCode::CaptureConsentDenied
        | CallErrorCode::CaptureConsentUnavailable
        | CallErrorCode::DriverMissing
        | CallErrorCode::DriverNotLoaded
        | CallErrorCode::DriverStale
        | CallErrorCode::DriverWrongVariant
        | CallErrorCode::DriverUnusable => IssueAction::OpenPermissions,
        CallErrorCode::SpeakerOutput
        | CallErrorCode::NoMicrophone
        | CallErrorCode::MicrophoneUnavailable
        | CallErrorCode::MicrophoneChanged
        | CallErrorCode::LocalOutputUnavailable => IssueAction::OpenAudioSettings,
        CallErrorCode::TranslationUnavailable | CallErrorCode::Provider(_) => {
            IssueAction::OpenSpeechSettings
        }
        _ => IssueAction::None,
    }
}

fn issue(i: &CallIssue) -> IssueView {
    IssueView {
        action: action_of(i.code),
        key: issue_key(i.code).to_owned(),
        severity: match i.severity {
            Severity::Blocking => IssueSeverity::Blocking,
            Severity::Degraded => IssueSeverity::Degraded,
            Severity::Info => IssueSeverity::Info,
        },
        detail: i.detail.clone(),
    }
}

fn notice(d: &DisclosureSnapshot) -> NoticeView {
    NoticeView {
        enabled: d.enabled,
        state: match d.state {
            DisclosureState::Off => NoticeState::Off,
            DisclosureState::Preparing => NoticeState::Preparing,
            DisclosureState::Ready => NoticeState::Ready,
            DisclosureState::Playing => NoticeState::Playing,
            DisclosureState::Played => NoticeState::Played,
            DisclosureState::Failed => NoticeState::Failed,
        },
        plays: d.plays,
        last_played_s: d.last_played_s,
    }
}

/// The control's view of a snapshot.
pub(crate) fn panel_of(s: &CallSnapshot, trace: &TraceSlot) -> CallPanel {
    let (activity, activity_devices) = match &s.routing.call_activity {
        ActivityJudgement::Unknown => (ActivityView::Unknown, String::new()),
        ActivityJudgement::Unavailable => (ActivityView::Unavailable, String::new()),
        ActivityJudgement::NotRunning => (ActivityView::NotRunning, String::new()),
        ActivityJudgement::Idle => (ActivityView::Idle, String::new()),
        ActivityJudgement::ActiveOnOurMicrophone { .. } => {
            (ActivityView::OnOurMicrophone, String::new())
        }
        ActivityJudgement::ActiveOnOtherMicrophone { devices } => {
            (ActivityView::OnOtherMicrophone, devices.join(", "))
        }
    };
    CallPanel {
        translation: prxvc_desktop_ui::call_view::TranslationView::default(),
        translate_enabled: s.translate_enabled,
        phase: phase(&s.state),
        in_call_mode: s.routing.in_call_mode,
        outgoing: lane(&s.directions[0], trace.last(Direction::Outgoing)),
        incoming: lane(&s.directions[1], trace.last(Direction::Incoming)),
        microphone: s.devices.microphone.clone().unwrap_or_default(),
        virtual_sink: s.devices.virtual_sink.clone().unwrap_or_default(),
        call_app: s.devices.call_app.clone().unwrap_or_default(),
        activity,
        activity_devices,
        default_input_ours: s.routing.default_input_is_ours,
        displaced_to: s.routing.displaced_to.clone().unwrap_or_default(),
        issues: s.issues.iter().map(issue).collect(),
        last_error: s.last_error.as_ref().map(issue),
        notice: notice(&s.disclosure),
        finishing_s: s.finishing_s,
    }
}

#[cfg(test)]
mod tests {
    use prxvc_media::{Direction, MonoTime};
    use prxvc_session::call::{
        CallSnapshot, DeviceNames, LocalOutputMode, PathStats, RoutingSnapshot,
    };
    use prxvc_session::{DirectionState, Endpoint};

    use super::*;

    fn snapshot() -> CallSnapshot {
        let dir = |d, path| DirectionSnapshot {
            direction: d,
            state: DirectionState::Off,
            hears: OutputSource::Original,
            path,
            pipeline: None,
            languages: None,
        };
        CallSnapshot {
            revision: 3,
            state: CallState::AudioReady { translate: false },
            since: MonoTime::ZERO,
            translate_enabled: false,
            local_output_mode: LocalOutputMode::Translation,
            directions: [
                dir(
                    Direction::Outgoing,
                    Some(PathStats {
                        running: true,
                        source_device: "Built-in Mic".into(),
                        frames: 5,
                        peak: 0.5,
                        rms: 0.1,
                        overruns: 1,
                        drops: 2,
                        translated_underruns: 3,
                        translated_gap_ms: 900,
                        ..PathStats::default()
                    }),
                ),
                dir(Direction::Incoming, None),
            ],
            routing: RoutingSnapshot {
                in_call_mode: true,
                default_input_is_ours: true,
                call_activity: ActivityJudgement::ActiveOnOtherMicrophone {
                    devices: vec!["A".into(), "B".into()],
                },
                ..RoutingSnapshot::default()
            },
            devices: DeviceNames {
                microphone: Some("Built-in Mic".into()),
                ..DeviceNames::default()
            },
            issues: vec![CallIssue {
                code: CallErrorCode::CaptureNoSignal,
                severity: Severity::Degraded,
                detail: "d".into(),
                since: MonoTime::ZERO,
            }],
            last_error: None,
            last_output_change: None,
            disclosure: DisclosureSnapshot::default(),
            finishing_s: None,
        }
    }

    #[test]
    fn a_graceful_stop_reaches_the_view_with_the_seconds_left() {
        let mut s = snapshot();
        assert_eq!(panel_of(&s, &TraceSlot::default()).finishing_s, None);
        s.finishing_s = Some(4);
        assert_eq!(panel_of(&s, &TraceSlot::default()).finishing_s, Some(4));
    }

    #[test]
    fn the_snapshot_becomes_the_controls_view() {
        let p = panel_of(&snapshot(), &TraceSlot::default());
        assert_eq!(p.phase, CallPhase::AudioReady);
        assert!(p.in_call_mode && p.default_input_ours);
        assert!(p.outgoing.running && !p.incoming.running);
        assert_eq!(
            (
                p.outgoing.frames,
                p.outgoing.overruns,
                p.outgoing.drops,
                p.outgoing.gaps
            ),
            (5, 1, 2, 3)
        );
        assert_eq!(p.outgoing.device, "Built-in Mic");
        assert_eq!(p.activity, ActivityView::OnOtherMicrophone);
        assert_eq!(p.activity_devices, "A, B");
        assert_eq!(p.issues.len(), 1);
        assert_eq!(p.issues[0].key, "call.issue.capture_no_signal");
        assert_eq!(p.issues[0].severity, IssueSeverity::Degraded);
        assert!(p.can_stop() && !p.can_start());
    }

    #[test]
    fn every_error_code_has_a_message_in_both_languages() {
        let codes = [
            CallErrorCode::NotInBundle,
            CallErrorCode::NoCallApp,
            CallErrorCode::MicrophoneConsentDenied,
            CallErrorCode::MicrophoneConsentUnavailable,
            CallErrorCode::CaptureConsentDenied,
            CallErrorCode::CaptureConsentUnavailable,
            CallErrorCode::DriverMissing,
            CallErrorCode::DriverNotLoaded,
            CallErrorCode::DriverStale,
            CallErrorCode::DriverWrongVariant,
            CallErrorCode::DriverUnusable,
            CallErrorCode::NoMicrophone,
            CallErrorCode::EndpointOpenFailed(Endpoint::RemoteOutput),
            CallErrorCode::TargetNotRunning,
            CallErrorCode::RoutingJournalPresent,
            CallErrorCode::RoutingSwitchFailed,
            CallErrorCode::RoutingUnavailable,
            CallErrorCode::VirtualMicrophoneLost,
            CallErrorCode::DefaultInputDisplaced,
            CallErrorCode::CaptureNoSignal,
            CallErrorCode::CaptureStalled,
            CallErrorCode::TargetGone,
            CallErrorCode::EndpointLost(Endpoint::LocalInput),
            CallErrorCode::MicrophoneUnavailable,
            CallErrorCode::MicrophoneChanged,
            CallErrorCode::RecoveryTimeout,
            CallErrorCode::CallOnOtherMicrophone,
            CallErrorCode::ActivityUnavailable,
            CallErrorCode::DirectionDegraded,
            CallErrorCode::LocalOutputUnavailable,
            CallErrorCode::RestoreFailed,
            CallErrorCode::StopLeak,
            CallErrorCode::CommandRejected,
            CallErrorCode::AssistantActive,
            CallErrorCode::TranslationUnavailable,
            CallErrorCode::SpeakerOutput,
            CallErrorCode::DisclosureUnavailable,
            CallErrorCode::Provider(ProviderIssue::ModelNotInstalled),
            CallErrorCode::Provider(ProviderIssue::ModelInstalling),
            CallErrorCode::Provider(ProviderIssue::ModelDamaged),
            CallErrorCode::Provider(ProviderIssue::RuntimeNotBuiltIn),
            CallErrorCode::Provider(ProviderIssue::VoiceProfileMissing),
            CallErrorCode::Provider(ProviderIssue::VoiceNotPrepared),
            CallErrorCode::Provider(ProviderIssue::VoicePresetMissing),
            CallErrorCode::Provider(ProviderIssue::ServerUnreachable),
            CallErrorCode::Provider(ProviderIssue::Unsupported),
            CallErrorCode::Provider(ProviderIssue::Offline),
            CallErrorCode::Provider(ProviderIssue::Setting),
            CallErrorCode::Provider(ProviderIssue::StartFailed),
            CallErrorCode::Internal,
        ];
        for locale in [prxvc_i18n::Locale::En, prxvc_i18n::Locale::ZhCn] {
            prxvc_i18n::set_locale(locale);
            for code in codes {
                let key = issue_key(code);
                assert_ne!(prxvc_i18n::t(key), key, "{code:?} has no message");
            }
        }
        prxvc_i18n::set_locale(prxvc_i18n::Locale::En);
    }

    #[test]
    fn a_translating_direction_shows_captions_counters_languages_and_the_last_latency() {
        use prxvc_session::pipeline::{ClauseSnapshot, Counters, DirectionSnapshot as Pipe, PartialSnapshot};
        use prxvc_media::{ClauseId, Lang, SegmentId};
        let seg = SegmentId::first(Direction::Outgoing);
        let pipe = Pipe {
            version: 1,
            direction: Direction::Outgoing,
            state: DirectionState::Running,
            reasons: Default::default(),
            output: OutputSource::Translation,
            activity: prxvc_session::Activity::Recognizing,
            partial: Some(PartialSnapshot { segment: seg, revision: 2, text: "你好吗".into(), stable_prefix_len: 3 }),
            clauses: vec![ClauseSnapshot {
                clause: ClauseId::new(seg, 0),
                state: ClauseState::Translated,
                source_text: "你好".into(),
                translation: "Hello.".into(),
                skipped: None,
                flags: Vec::new(),
                retried: false,
            }],
            counters: Counters { late_dropped: 4, forced_cuts: 1, ..Counters::default() },
            backlog_ms: 0,
            recent_skips: Vec::new(),
            last_error: None,
            ledger: Default::default(),
        };
        let mut s = snapshot();
        s.translate_enabled = true;
        s.directions[0].pipeline = Some(std::sync::Arc::new(pipe));
        s.directions[0].languages = Some((Lang::Zh, Lang::En));
        let lane = panel_of(&s, &TraceSlot::default()).outgoing;
        assert!(lane.translating);
        assert_eq!(lane.languages, Some(("zh".into(), "en".into())));
        assert_eq!(lane.partial.as_ref().map(|p| p.stable_len), Some(3));
        assert_eq!(lane.clauses[0].translation, "Hello.");
        assert_eq!(lane.clauses[0].status, ClauseStatus::Translated);
        assert_eq!((lane.clauses[0].utterance, lane.clauses[0].index), (1, 0));
        let c = lane.counters.expect("counters");
        assert_eq!((c.late_dropped, c.forced_cuts), (4, 1));
        assert!(lane.latency.is_none(), "no clause finished yet");
        assert!(!panel_of(&snapshot(), &TraceSlot::default()).incoming.translating);
    }
}
