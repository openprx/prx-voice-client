//! Mappings from the audio crate's typed errors and events to the coordinator's vocabulary.
//!
//! Pure functions: every one is covered by a unit test, so the typed code that
//! reaches the snapshot (and from there the message the user reads) does not
//! depend on hardware.

use prxvc_audio::call_activity::{CallJudgement, ProcessDevice};
use prxvc_audio::call_pump::PumpEvent;
use prxvc_audio::driver_status::{DriverIssue, DriverVariant};
use prxvc_audio::mic_consent::MicConsentError;
use prxvc_audio::routing::{CallModeError, RoutingEvent};
use prxvc_audio::tap_types::TapError;
use prxvc_audio::virtual_mic::VirtualMicError;
use prxvc_ports::BundleId;
use prxvc_session::Endpoint;
use prxvc_session::call::{
    ActivityJudgement, CallErrorCode as Code, HostFault, PathEvent, RoutingNotice,
};

/// The two paths of a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PathKind {
    /// Physical microphone to the virtual microphone.
    Outgoing,
    /// The call application's tap.
    Incoming,
}

impl PathKind {
    /// The endpoint whose loss ends this path's source.
    pub(super) fn source_endpoint(self) -> Endpoint {
        match self {
            Self::Outgoing => Endpoint::LocalInput,
            Self::Incoming => Endpoint::RemoteInput,
        }
    }
}

/// The driver variant a call application needs: WhatsApp's call device list
/// skips the Virtual transport, so it needs the USB variant (pipeline-design
/// PD-4.1 step 3). Other applications accept any variant until the per-app
/// profile file exists.
pub(super) fn required_variant(target: &BundleId) -> Option<DriverVariant> {
    (target.as_str() == "net.whatsapp.WhatsApp").then_some(DriverVariant::Usb)
}

pub(super) fn driver_fault(issue: &DriverIssue) -> HostFault {
    let code = match issue {
        DriverIssue::DriverMissing => Code::DriverMissing,
        DriverIssue::BundleUnreadable(_) => Code::DriverUnusable,
        DriverIssue::NotLoaded { .. } | DriverIssue::NotAlive { .. } => Code::DriverNotLoaded,
        DriverIssue::DriverStale { .. } => Code::DriverStale,
        DriverIssue::WrongVariant { .. } => Code::DriverWrongVariant,
    };
    HostFault::new(code, issue.to_string())
}

pub(super) fn mic_consent_fault(e: &MicConsentError) -> HostFault {
    let code = match e {
        MicConsentError::Denied => Code::MicrophoneConsentDenied,
        MicConsentError::NotInAppBundle | MicConsentError::NoAnswer { .. } => {
            Code::MicrophoneConsentUnavailable
        }
    };
    HostFault::new(code, e.to_string())
}

pub(super) fn capture_consent_fault(e: &TapError) -> HostFault {
    let code = match e {
        TapError::PermissionDenied => Code::CaptureConsentDenied,
        _ => Code::CaptureConsentUnavailable,
    };
    HostFault::new(code, e.to_string())
}

/// A failure to open the virtual microphone sink.
pub(super) fn virtual_mic_fault(e: &VirtualMicError) -> HostFault {
    match e {
        VirtualMicError::Driver(issue) => driver_fault(issue),
        VirtualMicError::DeviceMissing { .. } => HostFault::new(Code::DriverNotLoaded, e.to_string()),
        VirtualMicError::UnexpectedFormat { .. } | VirtualMicError::Audio(_) => {
            HostFault::new(Code::EndpointOpenFailed(Endpoint::RemoteOutput), e.to_string())
        }
    }
}

/// A failure of the routing controller (enter, leave, reassert).
pub(super) fn call_mode_fault(e: &CallModeError) -> HostFault {
    let code = match e {
        CallModeError::DriverNotReady(issue) => return driver_fault(issue),
        CallModeError::NoPhysicalMicrophone(_) => Code::NoMicrophone,
        CallModeError::JournalPresent { .. } | CallModeError::JournalCorrupt => {
            Code::RoutingJournalPresent
        }
        CallModeError::SwitchFailed(_) | CallModeError::SwitchNotApplied { .. } => {
            Code::RoutingSwitchFailed
        }
        CallModeError::VirtualMicrophoneLost => Code::VirtualMicrophoneLost,
        CallModeError::NoRestoreTarget
        | CallModeError::RestoreFailed(_)
        | CallModeError::RestoreNotApplied { .. } => Code::RestoreFailed,
        CallModeError::AlreadyInCallMode
        | CallModeError::NotInCallMode
        | CallModeError::JournalWrite(_)
        | CallModeError::Routing(_) => Code::RoutingUnavailable,
    };
    HostFault::new(code, e.to_string())
}

pub(super) fn routing_notice(event: RoutingEvent) -> RoutingNotice {
    match event {
        RoutingEvent::DefaultInputDisplaced { to } => {
            RoutingNotice::DefaultInputDisplaced { to: to.to_string() }
        }
        RoutingEvent::OwnMicrophoneReselected => RoutingNotice::OwnMicrophoneReselected,
        RoutingEvent::RestoreTargetChanged { target } => RoutingNotice::RestoreTargetChanged {
            target: target.map(|d| d.name),
        },
        RoutingEvent::VirtualMicrophoneLost => RoutingNotice::VirtualMicrophoneLost,
        RoutingEvent::DefaultOutputChanged { to } => {
            RoutingNotice::DefaultOutputChanged { to: to.to_string() }
        }
        RoutingEvent::SystemSleeping => RoutingNotice::SystemSleeping,
        RoutingEvent::SystemWoke { default_input_intact } => {
            RoutingNotice::SystemWoke { default_input_intact }
        }
    }
}

pub(super) fn judgement(j: &CallJudgement) -> ActivityJudgement {
    match j {
        CallJudgement::NotRunning => ActivityJudgement::NotRunning,
        CallJudgement::Idle => ActivityJudgement::Idle,
        CallJudgement::ActiveOnOurMicrophone { output_running } => {
            ActivityJudgement::ActiveOnOurMicrophone { output_running: *output_running }
        }
        CallJudgement::ActiveOnOtherMicrophone { devices } => {
            ActivityJudgement::ActiveOnOtherMicrophone {
                devices: devices.iter().map(ProcessDevice::label).map(str::to_owned).collect(),
            }
        }
    }
}

/// A pump event as the coordinator's path event; `None` for events with no meaning there.
pub(super) fn path_event(kind: PathKind, event: PumpEvent) -> Option<PathEvent> {
    Some(match event {
        PumpEvent::SourceLost { reason } => PathEvent::Lost(HostFault::new(
            Code::EndpointLost(kind.source_endpoint()),
            reason,
        )),
        PumpEvent::MixFailed { reason } => PathEvent::Fault(HostFault::new(Code::Internal, reason)),
        PumpEvent::DeviceChanged { device } => PathEvent::DeviceChanged(device),
        PumpEvent::DeviceUnavailable { message } => PathEvent::DeviceUnavailable(message),
        PumpEvent::NoSignal { detail } => PathEvent::NoSignal(detail),
        PumpEvent::SignalRestored => PathEvent::SignalRestored,
        PumpEvent::TargetGone => PathEvent::TargetGone,
        PumpEvent::TargetBack => PathEvent::TargetBack,
        PumpEvent::Stalled { idle_ms } => PathEvent::Stalled { idle_ms },
        PumpEvent::CaptureDenied { detail } => {
            PathEvent::Fault(HostFault::new(Code::CaptureConsentDenied, detail))
        }
        PumpEvent::CaptureFailed { detail } => {
            PathEvent::Fault(HostFault::new(Code::CaptureStalled, detail))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use prxvc_audio::input_policy::InputPolicyError;
    use prxvc_audio::routing::DeviceRef;
    use prxvc_ports::routing::{DeviceUid, RoutingError};

    use super::*;

    fn code(f: HostFault) -> Code {
        f.code
    }

    #[test]
    fn whatsapp_needs_the_usb_variant_and_others_accept_any() {
        assert_eq!(
            required_variant(&BundleId::new("net.whatsapp.WhatsApp")),
            Some(DriverVariant::Usb)
        );
        assert_eq!(required_variant(&BundleId::new("ru.keepcoder.Telegram")), None);
    }

    #[test]
    fn driver_issues_map_to_driver_codes() {
        let cases = [
            (DriverIssue::DriverMissing, Code::DriverMissing),
            (DriverIssue::BundleUnreadable("x".into()), Code::DriverUnusable),
            (DriverIssue::NotLoaded { uid: "u".into() }, Code::DriverNotLoaded),
            (DriverIssue::NotAlive { uid: "u".into() }, Code::DriverNotLoaded),
            (
                DriverIssue::DriverStale { installed: "0.0.1".into(), bundled: "0.1.0".into() },
                Code::DriverStale,
            ),
            (
                DriverIssue::WrongVariant {
                    installed: DriverVariant::Virtual,
                    required: DriverVariant::Usb,
                },
                Code::DriverWrongVariant,
            ),
        ];
        for (issue, expected) in cases {
            assert_eq!(code(driver_fault(&issue)), expected, "{issue}");
        }
    }

    #[test]
    fn consent_errors_map_to_consent_codes() {
        assert_eq!(code(mic_consent_fault(&MicConsentError::Denied)), Code::MicrophoneConsentDenied);
        assert_eq!(
            code(mic_consent_fault(&MicConsentError::NotInAppBundle)),
            Code::MicrophoneConsentUnavailable
        );
        assert_eq!(
            code(mic_consent_fault(&MicConsentError::NoAnswer { secs: 30 })),
            Code::MicrophoneConsentUnavailable
        );
        assert_eq!(code(capture_consent_fault(&TapError::PermissionDenied)), Code::CaptureConsentDenied);
        assert_eq!(
            code(capture_consent_fault(&TapError::ConsentNoAnswer { reason: "x".into() })),
            Code::CaptureConsentUnavailable
        );
    }

    #[test]
    fn routing_errors_map_to_routing_codes() {
        let uid = DeviceUid::new("ours");
        let unavailable = RoutingError::Unavailable { reason: "x".into() };
        let cases = [
            (CallModeError::DriverNotReady(DriverIssue::DriverMissing), Code::DriverMissing),
            (
                CallModeError::NoPhysicalMicrophone(InputPolicyError::NoUsableDevice),
                Code::NoMicrophone,
            ),
            (CallModeError::JournalPresent { owner_pid: 1, owner_alive: false }, Code::RoutingJournalPresent),
            (CallModeError::JournalCorrupt, Code::RoutingJournalPresent),
            (CallModeError::SwitchFailed(unavailable.clone()), Code::RoutingSwitchFailed),
            (
                CallModeError::SwitchNotApplied { expected: uid.clone(), actual: None },
                Code::RoutingSwitchFailed,
            ),
            (CallModeError::VirtualMicrophoneLost, Code::VirtualMicrophoneLost),
            (CallModeError::NoRestoreTarget, Code::RestoreFailed),
            (CallModeError::RestoreFailed(unavailable.clone()), Code::RestoreFailed),
            (CallModeError::AlreadyInCallMode, Code::RoutingUnavailable),
            (CallModeError::JournalWrite("disk".into()), Code::RoutingUnavailable),
            (CallModeError::Routing(unavailable), Code::RoutingUnavailable),
        ];
        for (error, expected) in cases {
            assert_eq!(code(call_mode_fault(&error)), expected, "{error}");
        }
    }

    #[test]
    fn routing_events_keep_their_meaning() {
        assert_eq!(
            routing_notice(RoutingEvent::DefaultInputDisplaced { to: DeviceUid::new("airpods") }),
            RoutingNotice::DefaultInputDisplaced { to: "airpods".into() }
        );
        assert_eq!(
            routing_notice(RoutingEvent::RestoreTargetChanged {
                target: Some(DeviceRef { uid: DeviceUid::new("u"), name: "Studio Mic".into() })
            }),
            RoutingNotice::RestoreTargetChanged { target: Some("Studio Mic".into()) }
        );
        assert_eq!(
            routing_notice(RoutingEvent::SystemWoke { default_input_intact: false }),
            RoutingNotice::SystemWoke { default_input_intact: false }
        );
        assert_eq!(routing_notice(RoutingEvent::VirtualMicrophoneLost), RoutingNotice::VirtualMicrophoneLost);
        assert_eq!(routing_notice(RoutingEvent::SystemSleeping), RoutingNotice::SystemSleeping);
    }

    #[test]
    fn the_activity_judgement_names_the_other_microphones() {
        let airpods = ProcessDevice {
            object_id: 1,
            uid: "u".into(),
            name: "AirPods".into(),
            input_channels: 1,
            output_channels: 0,
        };
        assert_eq!(
            judgement(&CallJudgement::ActiveOnOtherMicrophone { devices: vec![airpods] }),
            ActivityJudgement::ActiveOnOtherMicrophone { devices: vec!["AirPods".into()] }
        );
        assert_eq!(judgement(&CallJudgement::NotRunning), ActivityJudgement::NotRunning);
        assert_eq!(
            judgement(&CallJudgement::ActiveOnOurMicrophone { output_running: true }),
            ActivityJudgement::ActiveOnOurMicrophone { output_running: true }
        );
    }

    #[test]
    fn a_lost_source_names_the_endpoint_of_its_path() {
        let lost = |kind| path_event(kind, PumpEvent::SourceLost { reason: "gone".into() });
        assert_eq!(
            lost(PathKind::Outgoing),
            Some(PathEvent::Lost(HostFault::new(Code::EndpointLost(Endpoint::LocalInput), "gone")))
        );
        assert_eq!(
            lost(PathKind::Incoming),
            Some(PathEvent::Lost(HostFault::new(Code::EndpointLost(Endpoint::RemoteInput), "gone")))
        );
        assert_eq!(path_event(PathKind::Incoming, PumpEvent::TargetGone), Some(PathEvent::TargetGone));
        assert!(matches!(
            path_event(PathKind::Incoming, PumpEvent::CaptureDenied { detail: "d".into() }),
            Some(PathEvent::Fault(f)) if f.code == Code::CaptureConsentDenied
        ));
    }
}
