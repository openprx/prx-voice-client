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
use prxvc_audio::echo_gate::{EchoGate, EchoGateHandle};
use prxvc_audio::input_devices::list_input_devices;
use prxvc_audio::input_policy::{OWN_MIC_UID, choose_input};
use prxvc_audio::mic_consent::ensure_microphone_consent;
use prxvc_audio::mix::MixerConfig;
use prxvc_audio::routing::{
    CoreAudioRouting, EnterRequest, FileJournalStore, LeaveOutcome, OsSystem, RoutingController,
};
use prxvc_audio::tap_consent::{ConsentOutcome, request_capture_consent};
use std::sync::Arc;

use prxvc_media::Direction;
use prxvc_ports::BundleId;
use prxvc_session::call::{
    ActivityJudgement, AudioPath, CallErrorCode, CallHost, EnteredRouting, HostFault, LeftRouting,
    PreparedProviders, ProvidersFailure, RoutingNotice,
};

use crate::call_kit::config::ConfigSource;
use crate::call_kit::providers::ProviderHost;
use crate::call_kit::trace::{CallTrace, TraceOptions, TraceSlot};

use super::map::{
    call_mode_fault, capture_consent_fault, driver_fault, judgement, mic_consent_fault,
    required_variant, routing_notice,
};
use super::paths::{IncomingPlan, OutgoingPlan, start_incoming, start_outgoing};
use crate::call_kit::route::{current_route, gate_enabled};

/// How long a system consent prompt may wait for the user.
const CONSENT_TIMEOUT: Duration = Duration::from_secs(30);

/// What the host reads from the configuration and the application paths.
#[derive(Clone)]
pub(crate) struct HostConfig {
    /// `audio.input_device`.
    pub input_device: String,
    /// `audio.virtual_mic_device`.
    pub virtual_mic_device: String,
    /// `audio.output_device`: where the translation is played; empty follows the system default.
    pub output_device: String,
    /// `audio.echo_mode`.
    pub echo_mode: prxvc_config::EchoMode,
    /// `audio.half_duplex_tail_ms`.
    pub echo_tail: Duration,
    /// The incoming mixer: `call.output_gain_db`, `call.original_gain_db` and the jitter buffer.
    pub incoming_mixer: MixerConfig,
    /// The outgoing mixer: the translated lane's jitter buffer and adaptive pre-roll.
    pub outgoing_mixer: MixerConfig,
    /// Where the routing journal lives.
    pub data_dir: PathBuf,
    /// The configuration the providers are resolved from, read again at every translated start.
    pub config_source: ConfigSource,
    /// The directions that translate.
    pub translate: Vec<Direction>,
    /// The current call's trace recorder, made at every translated start.
    pub trace: TraceSlot,
}

type Controller = RoutingController<CoreAudioRouting, OsSystem, FileJournalStore>;

/// [`CallHost`] on this machine.
pub(crate) struct RealCallHost {
    config: HostConfig,
    controller: Result<Controller, String>,
    /// The microphone the preflight chose, followed from then on (UID, else name).
    pinned_microphone: Option<String>,
    activity: Option<CallActivityWatcher>,
    providers: ProviderHost,
    /// The half-duplex gate of this call; closed only in speaker mode while our output plays.
    echo: EchoGateHandle,
    /// What the local output was when the call was checked.
    route: prxvc_session::call::OutputRoute,
}

impl RealCallHost {
    /// A host; the routing controller starts listening to the system now.
    pub(crate) fn new(config: HostConfig) -> Self {
        let controller = CoreAudioRouting::new()
            .map(|routing| {
                RoutingController::new(
                    routing,
                    OsSystem,
                    FileJournalStore::in_dir(&config.data_dir),
                )
            })
            .map_err(|e| e.to_string());
        if let Err(reason) = &controller {
            tracing::error!(%reason, "call mode cannot read or change the system audio settings");
        }
        let providers =
            ProviderHost::new(Arc::clone(&config.config_source), config.translate.clone());
        Self {
            config,
            controller,
            pinned_microphone: None,
            activity: None,
            providers,
            echo: EchoGateHandle::disabled(),
            route: prxvc_session::call::OutputRoute::Unknown,
        }
    }

    /// Read what the local output is and set the echo gate for this call (design 6.3).
    fn check_output(&mut self) {
        self.route = current_route(&self.config.output_device);
        let enabled = gate_enabled(self.config.echo_mode, self.route);
        tracing::info!(route = ?self.route, gate = enabled, "local output checked");
        self.echo = EchoGateHandle::new(EchoGate::new(enabled, self.config.echo_tail));
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
        self.check_output();
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
            echo: Some(self.echo.clone()),
            mixer: self.config.outgoing_mixer,
        })
    }

    fn enter_call_mode(&mut self, target: &BundleId) -> Result<EnteredRouting, HostFault> {
        let request = EnterRequest {
            required_variant: required_variant(target),
            configured_input: self.config.input_device.clone(),
        };
        // The idle watch of the default input stands aside from here until call mode is left.
        crate::routing_recovery::set_call_mode(true);
        // The assistant's engines kept warm after its last session make room for the call's own.
        crate::assistant::launch::release_warm();
        let outcome = match self
            .controller()
            .and_then(|c| c.enter_call_mode(&request).map_err(|e| call_mode_fault(&e)))
        {
            Ok(o) => o,
            Err(e) => {
                crate::routing_recovery::set_call_mode(false);
                return Err(e);
            }
        };
        Ok(EnteredRouting {
            saved_input: outcome.saved.map(|d| d.name),
            physical_input: outcome.physical_input.name,
        })
    }

    fn start_incoming(
        &mut self,
        target: &BundleId,
        translate: bool,
    ) -> Result<Box<dyn AudioPath>, HostFault> {
        start_incoming(&IncomingPlan {
            target,
            translate,
            output_device: &self.config.output_device,
            mixer: self.config.incoming_mixer,
            route: self.route,
            echo: Some(self.echo.clone()),
        })
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
        crate::routing_recovery::set_call_mode(false);
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

    fn prepare_providers(&mut self) -> Result<PreparedProviders, ProvidersFailure> {
        // A new recorder for each call: its own file when tracing is on, and the figures of the
        // debug view in every case.
        let options =
            TraceOptions::from_config(&(self.config.config_source)(), &self.config.data_dir);
        let trace = CallTrace::open(&options);
        self.config.trace.set(Arc::clone(&trace));
        self.providers.prepare(trace)
    }

    fn release_providers(&mut self) {
        self.providers.release();
    }
}
