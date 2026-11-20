//! The two audio paths of a call as the coordinator sees them: a pump with its endpoints behind `AudioPath`.
//!
//! * Outgoing: the physical microphone (chosen and followed by the input
//!   policy) through the mixer's original lane into the virtual microphone sink.
//! * Incoming: the call application's tap, read and metered. **Nothing is played**:
//!   the tap is unmuted, so the call application plays the remote party itself,
//!   and playing the tap as well would double it. This is the design's "original
//!   only" behaviour for a build without translation.

use std::time::Duration;

use prxvc_audio::call_activity::native::read_app_activity;
use prxvc_audio::call_pump::{
    EventQueue, Pump, PumpConfig, PumpRoute, PumpSpec, PumpStage, PumpStats,
};
use prxvc_audio::input_supervisor::PlatformInputBackend;
use prxvc_audio::mix::{MixMode, Mixer, MixerConfig};
use prxvc_audio::supervised_mic::SupervisedMic;
use prxvc_audio::tap_pump_source::TapPumpSource;
use prxvc_audio::tap_types::TapConfig;
use prxvc_audio::virtual_mic::{VirtualMicOptions, VirtualMicSink};
use prxvc_ports::BundleId;
use prxvc_session::Endpoint;
use prxvc_session::OutputSource;
use prxvc_session::call::{AudioPath, CallErrorCode, HostFault, PathEvent, PathStats};

use super::map::{PathKind, path_event, virtual_mic_fault};

/// How long stopping a path may take before it is reported as a leak.
const STOP_TIMEOUT: Duration = Duration::from_secs(1);

/// A pump as an [`AudioPath`].
pub(super) struct PumpPath {
    pump: Pump,
    kind: PathKind,
}

impl AudioPath for PumpPath {
    fn set_listener_hears(&mut self, hears: OutputSource) {
        // No translated audio exists in this build, so "nothing" is the mixer without its
        // original lane: the translation mode with no translated input plays silence.
        let mode = match hears {
            OutputSource::Original => MixMode::Original,
            OutputSource::Translation | OutputSource::Nothing => MixMode::Translation,
        };
        self.pump.set_mode(mode);
    }

    fn stats(&self) -> PathStats {
        stats_of(&self.pump.stats())
    }

    fn take_events(&mut self) -> Vec<PathEvent> {
        self.pump
            .take_events()
            .into_iter()
            .filter_map(|e| path_event(self.kind, e))
            .collect()
    }

    fn stop(&mut self) -> Result<(), HostFault> {
        self.pump.stop(STOP_TIMEOUT).map_err(|e| {
            HostFault::new(
                CallErrorCode::StopLeak,
                format!("{} path: {e}", self.pump.name()),
            )
        })
    }
}

/// The pump's counters in the coordinator's terms.
fn stats_of(s: &PumpStats) -> PathStats {
    PathStats {
        running: s.running,
        failed: s.failed,
        source_device: s.source_device.clone(),
        sink_device: s.sink_device.clone(),
        frames: s.frames,
        peak: s.level.peak,
        rms: s.level.rms,
        last_frame_ts: s.last_frame_ts,
        overruns: s.source_dropped_samples,
        drops: s.sink_dropped_samples + s.lane_dropped_samples,
        underruns: s.sink_underruns + s.lane_underruns,
        queued: s.sink_queued,
    }
}

/// What the outgoing path is built from.
pub(super) struct OutgoingPlan<'a> {
    /// The microphone to follow: a UID or name, pinned when call mode was checked.
    pub microphone: &'a str,
    /// `audio.virtual_mic_device`; empty is our driver's sink.
    pub virtual_mic_device: &'a str,
}

/// Open the virtual microphone sink and the microphone and start the outgoing pump.
pub(super) fn start_outgoing(plan: &OutgoingPlan<'_>) -> Result<Box<dyn AudioPath>, HostFault> {
    let sink = VirtualMicSink::open(&VirtualMicOptions::from_config(
        plan.virtual_mic_device,
        None,
    ))
    .map_err(|e| virtual_mic_fault(&e))?;
    let events = EventQueue::new();
    let source = SupervisedMic::new(PlatformInputBackend, plan.microphone, events.clone());
    let mixer = Box::new(Mixer::new(MixerConfig {
        mode: MixMode::Original,
        ..MixerConfig::default()
    }));
    let pump = Pump::start(PumpSpec {
        config: PumpConfig::new("outgoing"),
        source: Box::new(source),
        route: PumpRoute::Mix {
            mixer,
            sink: Box::new(sink),
        },
        events,
    })
    .map_err(|e| {
        let endpoint = match e.stage {
            PumpStage::Sink => Endpoint::RemoteOutput,
            PumpStage::Source => Endpoint::LocalInput,
        };
        HostFault::new(CallErrorCode::EndpointOpenFailed(endpoint), e.to_string())
    })?;
    Ok(Box::new(PumpPath {
        pump,
        kind: PathKind::Outgoing,
    }))
}

/// Tap the call application (unmuted) and start the metering pump.
pub(super) fn start_incoming(target: &BundleId) -> Result<Box<dyn AudioPath>, HostFault> {
    // The audio system lists a process only while the application runs: a typed answer
    // to "is it running", which the tap's own error would only give as text.
    if read_app_activity(target.as_str()).is_ok_and(|a| !a.present) {
        return Err(HostFault::new(
            CallErrorCode::TargetNotRunning,
            format!("{target} is not running or has no audio process"),
        ));
    }
    let events = EventQueue::new();
    let source = TapPumpSource::new(TapConfig::for_bundle(target.as_str()), events.clone());
    let pump = Pump::start(PumpSpec {
        config: PumpConfig::new("incoming"),
        source: Box::new(source),
        route: PumpRoute::MeterOnly,
        events,
    })
    .map_err(|e| {
        HostFault::new(
            CallErrorCode::EndpointOpenFailed(Endpoint::RemoteInput),
            e.to_string(),
        )
    })?;
    Ok(Box::new(PumpPath {
        pump,
        kind: PathKind::Incoming,
    }))
}

#[cfg(test)]
mod tests {
    use prxvc_audio::call_pump::Level;
    use prxvc_media::MonoTime;

    use super::*;

    #[test]
    fn counters_are_summed_into_the_coordinators_terms() {
        let stats = PumpStats {
            running: true,
            failed: false,
            source_device: "mic".into(),
            sink_device: Some("sink".into()),
            frames: 9,
            samples: 90,
            level: Level {
                peak: 0.5,
                rms: 0.25,
            },
            last_frame_ts: Some(MonoTime::from_millis(5)),
            source_dropped_samples: 7,
            sink_dropped_samples: 3,
            sink_underruns: 2,
            lane_underruns: 1,
            lane_dropped_samples: 4,
            sink_queued: Duration::from_millis(40),
            mode: Some(MixMode::Original),
        };
        let s = stats_of(&stats);
        assert_eq!((s.frames, s.overruns, s.drops, s.underruns), (9, 7, 7, 3));
        assert_eq!((s.peak, s.rms), (0.5, 0.25));
        assert_eq!(s.sink_device.as_deref(), Some("sink"));
        assert_eq!(s.queued, Duration::from_millis(40));
    }
}
