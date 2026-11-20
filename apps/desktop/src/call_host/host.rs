//! The real `CallHost`: consent, driver and microphone checks, the two paths, the routing controller and the call-activity probe, on this machine.
//!
//! Built on the coordinator's thread (the routing controller owns native
//! listeners). Nothing here decides anything about the call: the coordinator
//! orders these steps and this file performs them and names their failures
//! with typed codes (`map`).

use std::path::PathBuf;
use std::time::Duration;

use prxvc_audio::call_activity::native::NativeActivitySource;
use prxvc_audio::call_activity::{CallActivityEvent, CallActivityWatcher, WatcherConfig};
use prxvc_audio::driver_status::DriverStatus;
use prxvc_audio::input_devices::list_input_devices;
use prxvc_audio::input_policy::{OWN_MIC_UID, choose_input};
use prxvc_audio::mic_consent::ensure_microphone_consent;
use prxvc_audio::routing::{
    CoreAudioRouting, EnterRequest, FileJournalStore, LeaveOutcome, OsSystem, RoutingController,
};
use prxvc_audio::tap_consent::{ConsentOutcome, request_capture_consent};
use prxvc_ports::BundleId;
use prxvc_session::ProviderRole;
use prxvc_session::call::{
    ActivityJudgement, AudioPath, CallErrorCode, CallHost, EnteredRouting, HostFault, LeftRouting,
    RoutingNotice,
};

use super::map::{
    call_mode_fault, capture_consent_fault, driver_fault, judgement, mic_consent_fault,
    required_variant, routing_notice,
};
use super::paths::{OutgoingPlan, start_incoming, start_outgoing};

/// How long a system consent prompt may wait for the user.
const CONSENT_TIMEOUT: Duration = Duration::from_secs(30);

/// What the host reads from the configuration and the application paths.
#[derive(Clone, Debug, Default)]
pub(crate) struct HostConfig {
    /// `audio.input_device`.
    pub input_device: String,
    /// `audio.virtual_mic_device`.
    pub virtual_mic_device: String,
    /// Where the routing journal lives.
    pub data_dir: PathBuf,
}

type Controller = RoutingController<CoreAudioRouting, OsSystem, FileJournalStore>;

/// [`CallHost`] on this machine.
pub(crate) struct RealCallHost {
    config: HostConfig,
    controller: Result<Controller, String>,
    /// The microphone the preflight chose, followed from then on (UID, else name).
    pinned_microphone: Option<String>,
    activity: Option<CallActivityWatcher>,
}

impl RealCallHost {
    /// A host; the routing controller starts listening to the system now.
    pub(crate) fn new(config: HostConfig) -> Self {
        let controller = CoreAudioRouting::new()
            .map(|routing| {
                RoutingController::new(routing, OsSystem, FileJournalStore::in_dir(&config.data_dir))
            })
            .map_err(|e| e.to_string());
        if let Err(reason) = &controller {
            tracing::error!(%reason, "call mode cannot read or change the system audio settings");
        }
        Self {
            config,
            controller,
            pinned_microphone: None,
            activity: None,
        }
    }

    fn controller(&mut self) -> Result<&mut Controller, HostFault> {
        self.controller
            .as_mut()
            .map_err(|reason| HostFault::new(CallErrorCode::RoutingUnavailable, reason.clone()))
    }
}

impl CallHost for RealCallHost {
    fn in_app_bundle(&self) -> bool {
        !crate::bundle::current().is_outside_bundle()
    }

    fn ensure_microphone_consent(&mut self) -> Result<(), HostFault> {
        ensure_microphone_consent(CONSENT_TIMEOUT).map_err(|e| mic_consent_fault(&e))
    }

    fn ensure_capture_consent(&mut self) -> Result<(), HostFault> {
        match request_capture_consent(CONSENT_TIMEOUT) {
            Ok(ConsentOutcome::Granted) => Ok(()),
            Ok(ConsentOutcome::Refused) => Err(capture_consent_fault(
                &prxvc_audio::tap_types::TapError::PermissionDenied,
            )),
            Err(e) => Err(capture_consent_fault(&e)),
        }
    }

    fn check_driver(&mut self, target: &BundleId) -> Result<(), HostFault> {
        DriverStatus::detect()
            .check(required_variant(target))
            .map_err(|issue| driver_fault(&issue))
    }

    fn check_microphone(&mut self) -> Result<String, HostFault> {
        let devices = list_input_devices()
            .map_err(|e| HostFault::new(CallErrorCode::NoMicrophone, e.to_string()))?;
        let chosen = choose_input(&devices, &self.config.input_device)
            .map_err(|e| HostFault::new(CallErrorCode::NoMicrophone, e.to_string()))?
            .device;
        // Follow this device from now on: once the default input is ours the policy
        // would otherwise prefer another one.
        self.pinned_microphone = Some(if chosen.uid.is_empty() {
            chosen.name.clone()
        } else {
            chosen.uid.clone()
        });
        Ok(chosen.name)
    }

    fn start_outgoing(&mut self) -> Result<Box<dyn AudioPath>, HostFault> {
        let microphone = self
            .pinned_microphone
            .clone()
            .unwrap_or_else(|| self.config.input_device.clone());
        start_outgoing(&OutgoingPlan {
            microphone: &microphone,
            virtual_mic_device: &self.config.virtual_mic_device,
        })
    }

    fn enter_call_mode(&mut self, target: &BundleId) -> Result<EnteredRouting, HostFault> {
        let request = EnterRequest {
            required_variant: required_variant(target),
            configured_input: self.config.input_device.clone(),
        };
        let outcome = self
            .controller()?
            .enter_call_mode(&request)
            .map_err(|e| call_mode_fault(&e))?;
        Ok(EnteredRouting {
            saved_input: outcome.saved.map(|d| d.name),
            physical_input: outcome.physical_input.name,
        })
    }

    fn start_incoming(&mut self, target: &BundleId) -> Result<Box<dyn AudioPath>, HostFault> {
        start_incoming(target)
    }

    fn start_activity_probe(&mut self, target: &BundleId) -> Result<(), HostFault> {
        let source = Box::new(NativeActivitySource::new(target.as_str()));
        let watcher = CallActivityWatcher::spawn(source, WatcherConfig::new(OWN_MIC_UID))
            .map_err(|e| HostFault::new(CallErrorCode::ActivityUnavailable, e.to_string()))?;
        self.activity = Some(watcher);
        Ok(())
    }

    fn call_activity(&mut self) -> ActivityJudgement {
        match self.activity.as_ref().and_then(CallActivityWatcher::latest) {
            Some(CallActivityEvent::Changed { judgement: j, .. }) => judgement(&j),
            Some(CallActivityEvent::Unavailable { .. }) => ActivityJudgement::Unavailable,
            None => ActivityJudgement::Unknown,
        }
    }

    fn stop_activity_probe(&mut self) {
        self.activity = None; // dropping the watcher stops and joins its thread
    }

    fn poll_routing(&mut self) -> Vec<RoutingNotice> {
        match self.controller.as_mut() {
            Ok(controller) => controller.poll().into_iter().map(routing_notice).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn reassert_routing(&mut self) -> Result<(), HostFault> {
        self.controller()?
            .reassert()
            .map_err(|e| call_mode_fault(&e))
    }

    fn leave_call_mode(&mut self) -> Result<LeftRouting, HostFault> {
        let outcome = self
            .controller()?
            .leave_call_mode()
            .map_err(|e| call_mode_fault(&e))?;
        Ok(match outcome {
            LeaveOutcome::NotInCallMode => LeftRouting {
                restored_to: None,
                left_alone: false,
                previous_gone: false,
                was_not_in_call_mode: true,
            },
            LeaveOutcome::Restored { restored, .. } => LeftRouting {
                restored_to: Some(restored.to.name),
                left_alone: false,
                previous_gone: matches!(
                    restored.reason,
                    prxvc_audio::routing::RestoreReason::Policy
                ),
                was_not_in_call_mode: false,
            },
            LeaveOutcome::LeftAlone { .. } => LeftRouting {
                restored_to: None,
                left_alone: true,
                previous_gone: false,
                was_not_in_call_mode: false,
            },
        })
    }

    fn prepare_providers(&mut self) -> Result<(), (ProviderRole, HostFault)> {
        Err((
            ProviderRole::Asr,
            HostFault::new(
                CallErrorCode::TranslationUnavailable,
                "translation is not part of this build: call mode runs audio only",
            ),
        ))
    }
}
