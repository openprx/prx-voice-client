//! Maps the coordinator's snapshot into the debug control's view model: enums, stable i18n keys for issues, numbers for levels and counters.

use prxvc_desktop_ui::{
    ActivityView, CallPanel, CallPhase, Heard, IssueSeverity, IssueView, LaneView,
};
use prxvc_session::call::{
    ActivityJudgement, CallErrorCode, CallIssue, CallSnapshot, DirectionSnapshot, Severity,
};
use prxvc_session::{CallState, OutputSource};

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
        CallErrorCode::RestoreFailed => "call.issue.restore_failed",
        CallErrorCode::StopLeak => "call.issue.stop_leak",
        CallErrorCode::CommandRejected => "call.issue.command_rejected",
        CallErrorCode::AssistantActive => "call.issue.assistant_active",
        CallErrorCode::TranslationUnavailable => "call.issue.translation_unavailable",
        _ => "call.issue.internal",
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

fn lane(d: &DirectionSnapshot) -> LaneView {
    let Some(path) = &d.path else {
        return LaneView {
            hears: heard(d.hears),
            ..LaneView::default()
        };
    };
    LaneView {
        running: path.running,
        hears: heard(d.hears),
        device: path.source_device.clone(),
        frames: path.frames,
        rms: path.rms,
        peak: path.peak,
        overruns: path.overruns,
        drops: path.drops,
        underruns: path.underruns,
    }
}

fn issue(i: &CallIssue) -> IssueView {
    IssueView {
        key: issue_key(i.code).to_owned(),
        severity: match i.severity {
            Severity::Blocking => IssueSeverity::Blocking,
            Severity::Degraded => IssueSeverity::Degraded,
            Severity::Info => IssueSeverity::Info,
        },
        detail: i.detail.clone(),
    }
}

/// The control's view of a snapshot.
pub(crate) fn panel_of(s: &CallSnapshot) -> CallPanel {
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
        phase: phase(&s.state),
        in_call_mode: s.routing.in_call_mode,
        outgoing: lane(&s.directions[0]),
        incoming: lane(&s.directions[1]),
        microphone: s.devices.microphone.clone().unwrap_or_default(),
        virtual_sink: s.devices.virtual_sink.clone().unwrap_or_default(),
        call_app: s.devices.call_app.clone().unwrap_or_default(),
        activity,
        activity_devices,
        default_input_ours: s.routing.default_input_is_ours,
        displaced_to: s.routing.displaced_to.clone().unwrap_or_default(),
        issues: s.issues.iter().map(issue).collect(),
        last_error: s.last_error.as_ref().map(issue),
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
                        underruns: 3,
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
        }
    }

    #[test]
    fn the_snapshot_becomes_the_controls_view() {
        let p = panel_of(&snapshot());
        assert_eq!(p.phase, CallPhase::AudioReady);
        assert!(p.in_call_mode && p.default_input_ours);
        assert!(p.outgoing.running && !p.incoming.running);
        assert_eq!(
            (
                p.outgoing.frames,
                p.outgoing.overruns,
                p.outgoing.drops,
                p.outgoing.underruns
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
            CallErrorCode::RestoreFailed,
            CallErrorCode::StopLeak,
            CallErrorCode::CommandRejected,
            CallErrorCode::AssistantActive,
            CallErrorCode::TranslationUnavailable,
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
}
