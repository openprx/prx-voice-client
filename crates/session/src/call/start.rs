//! The start-up sequence of call mode (pipeline-design PD-4.1): preflight, endpoints, routing, incoming path, activity probe.
//!
//! The order matters and is asserted by the tests through the host's log:
//!
//! 1. running from the application bundle
//! 2. a call application is selected
//! 3. microphone consent, then audio-capture consent (asked when undetermined)
//! 4. the driver (installed, loaded, current, the variant the application needs)
//! 5. a physical microphone can be used
//! 6. the outgoing path starts (physical microphone, virtual microphone sink, pump), so
//!    that the virtual microphone carries a signal before anything switches to it
//! 7. the default input switches to the virtual microphone (journal first)
//! 8. the incoming path starts (tap of the call application, pump)
//! 9. the call-activity probe starts (a failure is shown, not blocking)
//!
//! Steps 1 to 5 change nothing, so a failure there is `PreflightFailed` and the
//! machine goes straight to `Idle`. A failure in 6 to 8 is `EndpointFailed` or
//! `RoutingFailed`: the machine enters `Stopping` and its actions undo what was
//! done (routing left, paths closed) through the same code as a normal stop.

use prxvc_ports::BundleId;

use super::coordinator::{Coordinator, slot};
use super::error::{CallErrorCode, HostFault};
use crate::call_state::CallEvent;
use crate::fault::Endpoint;
use crate::output::OutputSource;
use prxvc_media::Direction;

impl Coordinator {
    /// Run steps 1 to 9 and report the outcome to the call machine.
    pub(super) fn start_sequence(&mut self, target: Option<BundleId>) {
        if self.routing.in_call_mode {
            // An earlier stop could not restore the default input: try again first.
            self.restore_routing();
        }
        let target = match self.preflight(target) {
            Ok(target) => target,
            Err(fault) => {
                self.raise_fault(&fault);
                self.feed(CallEvent::PreflightFailed(fault.code.blocker()));
                return;
            }
        };
        match self.start_paths(&target) {
            Ok(()) => {
                self.start_activity_probe(&target);
                self.feed(CallEvent::AudioRunning);
            }
            Err(Failed::Endpoint(endpoint, fault)) => {
                self.raise_fault(&fault);
                self.feed(CallEvent::EndpointFailed {
                    endpoint,
                    fault: fault.code.fault_kind(),
                });
            }
            Err(Failed::Routing(fault)) => {
                self.raise_fault(&fault);
                self.feed(CallEvent::RoutingFailed(fault.code.fault_kind()));
            }
        }
    }

    /// Steps 1 to 5. Returns the call application.
    fn preflight(&mut self, target: Option<BundleId>) -> Result<BundleId, HostFault> {
        if !self.host.in_app_bundle() {
            return Err(HostFault::new(
                CallErrorCode::NotInBundle,
                "call mode needs the application bundle: the tap would deliver silence outside it",
            ));
        }
        let target = target.filter(|t| !t.as_str().trim().is_empty()).ok_or_else(|| {
            HostFault::new(CallErrorCode::NoCallApp, "no call application is selected")
        })?;
        self.host.ensure_microphone_consent()?;
        self.host.ensure_capture_consent()?;
        self.host.check_driver(&target)?;
        let microphone = self.host.check_microphone()?;
        tracing::info!(%microphone, app = %target, "call preflight passed");
        Ok(target)
    }

    /// Steps 6 to 8.
    fn start_paths(&mut self, target: &BundleId) -> Result<(), Failed> {
        let outgoing = self
            .host
            .start_outgoing()
            .map_err(|f| Failed::Endpoint(endpoint_of(f.code, Endpoint::RemoteOutput), f))?;
        self.install_path(Direction::Outgoing, outgoing);

        let entered = self.host.enter_call_mode(target).map_err(Failed::Routing)?;
        self.routing.in_call_mode = true;
        self.routing.default_input_is_ours = true;
        self.routing.displaced_to = None;
        self.routing.saved_input = entered.saved_input;
        self.devices.microphone = Some(entered.physical_input);

        let incoming = self
            .host
            .start_incoming(target)
            .map_err(|f| Failed::Endpoint(endpoint_of(f.code, Endpoint::RemoteInput), f))?;
        self.install_path(Direction::Incoming, incoming);
        self.devices.call_app = Some(target.as_str().to_owned());
        Ok(())
    }

    /// Keep the path, note its devices and tell it what its listener hears (the original).
    fn install_path(&mut self, d: Direction, mut path: Box<dyn super::host::AudioPath>) {
        let stats = path.stats();
        if d == Direction::Outgoing {
            self.devices.virtual_sink.clone_from(&stats.sink_device);
            if self.devices.microphone.is_none() {
                self.devices.microphone = Some(stats.source_device.clone());
            }
        }
        path.set_listener_hears(OutputSource::Original);
        let lane = &mut self.lanes[slot(d)];
        lane.hears = OutputSource::Original;
        lane.stats = Some(stats);
        lane.path = Some(path);
    }

    /// Step 9.
    fn start_activity_probe(&mut self, target: &BundleId) {
        match self.host.start_activity_probe(target) {
            Ok(()) => self.activity_probe_on = true,
            Err(fault) => self.raise_fault(&fault),
        }
    }
}

/// A failed step of 6 to 8.
enum Failed {
    Endpoint(Endpoint, HostFault),
    Routing(HostFault),
}

/// The endpoint a failure belongs to: the one in the code, else `fallback`.
pub(super) fn endpoint_of(code: CallErrorCode, fallback: Endpoint) -> Endpoint {
    match code {
        CallErrorCode::EndpointOpenFailed(ep) | CallErrorCode::EndpointLost(ep) => ep,
        _ => fallback,
    }
}
