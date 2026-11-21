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
use std::sync::Arc;

use prxvc_audio::call_pump::{
    EchoRole, EventQueue, FrameTap, PlaybackSignal, Pump, PumpConfig, PumpEvent, PumpLink,
    PumpRoute, PumpSpec, PumpStage, PumpStats, TranslatedFeed,
};
use prxvc_audio::cpal_output;
use prxvc_audio::echo_gate::EchoGateHandle;
use prxvc_audio::input_supervisor::PlatformInputBackend;
use prxvc_audio::mix::{MixMode, Mixer, MixerConfig};
use prxvc_audio::supervised_mic::SupervisedMic;
use prxvc_audio::tap_pump_source::TapPumpSource;
use prxvc_audio::tap_types::{TapConfig, TapMuteMode};
use prxvc_audio::virtual_mic::{VirtualMicOptions, VirtualMicSink};
use prxvc_ports::BundleId;
use prxvc_session::Endpoint;
use prxvc_session::OutputSource;
use prxvc_media::{ClauseId, PcmChunk};
use prxvc_session::call::{
    AudioPath, CallErrorCode, HostFault, LocalOutputMode, OutputRoute, PathEvent, PathLink,
    PathStats, PlaybackReport, Playout,
};

use super::map::{PathKind, path_event, virtual_mic_fault};

/// How long stopping a path may take before it is reported as a leak.
const STOP_TIMEOUT: Duration = Duration::from_secs(1);

/// A pump as an [`AudioPath`].
pub(super) struct PumpPath {
    pump: Pump,
    kind: PathKind,
    /// What the direction machine says the listener hears.
    hears: OutputSource,
    /// `call.local_output_mode`, which matters for the incoming path only.
    local_mode: LocalOutputMode,
    /// What the device the path plays to is.
    route: OutputRoute,
}

impl PumpPath {
    fn new(pump: Pump, kind: PathKind, route: OutputRoute) -> Self {
        Self {
            pump,
            kind,
            hears: OutputSource::Original,
            local_mode: LocalOutputMode::default(),
            route,
        }
    }

    fn apply(&self) {
        self.pump.set_mode(mix_mode(self.kind, self.hears, self.local_mode));
    }
}

/// The mixer's mode for what the machine says the listener hears.
///
/// "Nothing" is the translation mode without translated audio: silence (the pipeline flushes the
/// lane when it announces it). For the incoming path "translation" follows `call.local_output_mode`
/// (pipeline-design 4.5): the translation alone, the original lowered under it, or the original alone.
pub(super) fn mix_mode(kind: PathKind, hears: OutputSource, local: LocalOutputMode) -> MixMode {
    match (hears, kind) {
        (OutputSource::Original, _) => MixMode::Original,
        (OutputSource::Nothing, _) | (OutputSource::Translation, PathKind::Outgoing) => {
            MixMode::Translation
        }
        (OutputSource::Translation, PathKind::Incoming) => match local {
            LocalOutputMode::Translation => MixMode::Translation,
            LocalOutputMode::Both => MixMode::Both,
            LocalOutputMode::Original => MixMode::Original,
        },
    }
}

/// Translated speech into the translated lane of a pump's mixer.
struct FeedPlayout(TranslatedFeed);

impl Playout for FeedPlayout {
    fn push(&self, clause: ClauseId, chunk: &PcmChunk) {
        self.0.push(clause, chunk);
    }

    fn end_clause(&self, clause: ClauseId) {
        self.0.end_clause(clause);
    }

    fn flush(&self) {
        self.0.flush();
    }
}

impl AudioPath for PumpPath {
    fn connect(&mut self, link: PathLink) -> Result<Arc<dyn Playout>, HostFault> {
        let PathLink {
            tap_rate,
            on_frame,
            mut on_playback,
            clock,
        } = link;
        let feed = self
            .pump
            .connect(PumpLink {
                tap: FrameTap {
                    rate: tap_rate,
                    sink: on_frame,
                },
                on_playback: Box::new(move |signal| {
                    on_playback(match signal {
                        PlaybackSignal::Started { clause, at } => {
                            PlaybackReport::Started { clause, at }
                        }
                        PlaybackSignal::Finished { clause, at } => {
                            PlaybackReport::Finished { clause, at }
                        }
                    });
                }),
                clock,
            })
            .ok_or_else(|| {
                HostFault::new(
                    CallErrorCode::TranslationUnavailable,
                    format!("the {} path plays nothing, so it cannot carry translation", self.pump.name()),
                )
            })?;
        Ok(Arc::new(FeedPlayout(feed)))
    }

    fn disconnect(&mut self) {
        self.pump.disconnect();
    }

    fn set_local_output_mode(&mut self, mode: LocalOutputMode) {
        self.local_mode = mode;
        self.apply();
    }

    fn set_listener_hears(&mut self, hears: OutputSource) {
        self.hears = hears;
        self.apply();
    }

    fn stats(&self) -> PathStats {
        PathStats {
            sink_route: self.route,
            ..stats_of(&self.pump.stats())
        }
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
        sink_underruns: s.sink_underruns,
        lane_underruns: s.lane_underruns,
        queued: s.sink_queued,
        translated_underruns: s.translated_underruns,
        translated_gap_ms: s.translated_gap_ms,
        translated_worst_gap_ms: s.translated_worst_gap_ms,
        translated_dropped: s.translated_dropped_samples,
        translated_peak: s.translated_level.peak,
        translated_rms: s.translated_level.rms,
        sink_route: OutputRoute::Unknown,
        input_gated: s.gated,
        gated_frames: s.gated_frames,
    }
}

/// What the outgoing path is built from.
pub(super) struct OutgoingPlan<'a> {
    /// The microphone to follow: a UID or name, pinned when call mode was checked.
    pub microphone: &'a str,
    /// `audio.virtual_mic_device`; empty is our driver's sink.
    pub virtual_mic_device: &'a str,
    /// The half-duplex gate, in speaker mode: while it is closed the microphone is not listened to.
    pub echo: Option<EchoGateHandle>,
    /// The mixer's settings: the translated lane's jitter buffer and adaptive pre-roll.
    pub mixer: MixerConfig,
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
        ..plan.mixer
    }));
    let pump = Pump::start(PumpSpec {
        config: PumpConfig::new("outgoing"),
        source: Box::new(source),
        route: PumpRoute::Mix {
            mixer,
            sink: Box::new(sink),
        },
        events,
        echo: plan.echo.clone().map(EchoRole::Input),
    })
    .map_err(|e| {
        let endpoint = match e.stage {
            PumpStage::Sink => Endpoint::RemoteOutput,
            PumpStage::Source => Endpoint::LocalInput,
        };
        HostFault::new(CallErrorCode::EndpointOpenFailed(endpoint), e.to_string())
    })?;
    Ok(Box::new(PumpPath::new(
        pump,
        PathKind::Outgoing,
        OutputRoute::Unknown,
    )))
}

/// What the incoming path is built from.
pub(super) struct IncomingPlan<'a> {
    /// The call application.
    pub target: &'a BundleId,
    /// Translated call: mute the tap while we tap it and play through our mixer.
    pub translate: bool,
    /// `audio.output_device`; empty follows the system default output.
    pub output_device: &'a str,
    /// The incoming mixer's settings: `call.output_gain_db`, `call.original_gain_db`, the jitter buffer.
    pub mixer: MixerConfig,
    /// What the local output is.
    pub route: OutputRoute,
    /// The half-duplex gate, in speaker mode: told whether our output is audible.
    pub echo: Option<EchoGateHandle>,
}

/// Tap the call application and start the incoming pump.
///
/// Audio only: the tap is unmuted, nothing is played, the level is metered. Translated: the local
/// output opens first and the tap is created muted-when-tapped only after that, so the original
/// is never muted without our playback being ready; our mixer starts on the original and the
/// coordinator moves it to the translation. If the output cannot be opened the path falls back to
/// the unmuted tap (design 4.5.1) and reports `LocalOutputFailed`.
pub(super) fn start_incoming(plan: &IncomingPlan<'_>) -> Result<Box<dyn AudioPath>, HostFault> {
    // The audio system lists a process only while the application runs: a typed answer
    // to "is it running", which the tap's own error would only give as text.
    if read_app_activity(plan.target.as_str()).is_ok_and(|a| !a.present) {
        return Err(HostFault::new(
            CallErrorCode::TargetNotRunning,
            format!("{} is not running or has no audio process", plan.target),
        ));
    }
    if !plan.translate {
        return meter_only(plan.target, EventQueue::new(), plan.route);
    }
    let events = EventQueue::new();
    let sink = match open_output(plan.output_device) {
        Ok(sink) => sink,
        Err(detail) => {
            events.push(PumpEvent::LocalOutputFailed {
                detail: format!("{detail}; the call application plays the original itself"),
            });
            return meter_only(plan.target, events, plan.route);
        }
    };
    let mut tap = TapConfig::for_bundle(plan.target.as_str());
    tap.mute = TapMuteMode::MutedWhenTapped;
    let source = TapPumpSource::new(tap, events.clone());
    let mixer = Box::new(Mixer::new(MixerConfig {
        mode: MixMode::Original,
        ..plan.mixer
    }));
    let mut config = PumpConfig::new("incoming");
    // Stopping the tap is what gives the original back, so it must not wait for the sink.
    config.stop_source_first = true;
    let spec = PumpSpec {
        config,
        source: Box::new(source),
        route: PumpRoute::Mix {
            mixer,
            sink: Box::new(sink),
        },
        events: events.clone(),
        echo: plan.echo.clone().map(EchoRole::Playback),
    };
    match Pump::start(spec) {
        Ok(pump) => Ok(Box::new(PumpPath::new(pump, PathKind::Incoming, plan.route))),
        Err(e) if e.stage == PumpStage::Sink => {
            events.push(PumpEvent::LocalOutputFailed {
                detail: format!("{e}; the call application plays the original itself"),
            });
            meter_only(plan.target, events, plan.route)
        }
        Err(e) => Err(HostFault::new(
            CallErrorCode::EndpointOpenFailed(Endpoint::RemoteInput),
            e.to_string(),
        )),
    }
}

/// The local output as a sink; not started.
fn open_output(device: &str) -> Result<prxvc_audio::playback_sink::PlaybackSink, String> {
    let device = device.trim();
    let sink = if device.is_empty() {
        cpal_output::open_default()
    } else {
        cpal_output::open_named(device)
    };
    sink.map_err(|e| format!("the local output cannot be opened: {e}"))
}

/// An unmuted tap whose audio is only metered: the call application plays the remote party.
fn meter_only(
    target: &BundleId,
    events: EventQueue,
    route: OutputRoute,
) -> Result<Box<dyn AudioPath>, HostFault> {
    let source = TapPumpSource::new(TapConfig::for_bundle(target.as_str()), events.clone());
    let pump = Pump::start(PumpSpec {
        config: PumpConfig::new("incoming"),
        source: Box::new(source),
        route: PumpRoute::MeterOnly,
        events,
        echo: None,
    })
    .map_err(|e| {
        HostFault::new(
            CallErrorCode::EndpointOpenFailed(Endpoint::RemoteInput),
            e.to_string(),
        )
    })?;
    Ok(Box::new(PumpPath::new(pump, PathKind::Incoming, route)))
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
            translated_underruns: 5,
            translated_gap_ms: 800,
            translated_worst_gap_ms: 300,
            translated_dropped_samples: 6,
            translated_level: Level {
                peak: 0.75,
                rms: 0.5,
            },
            gated_frames: 12,
            gated: true,
        };
        let s = stats_of(&stats);
        assert_eq!((s.frames, s.overruns, s.drops, s.underruns), (9, 7, 7, 3));
        assert_eq!((s.peak, s.rms), (0.5, 0.25));
        assert_eq!(s.sink_device.as_deref(), Some("sink"));
        assert_eq!(s.queued, Duration::from_millis(40));
        assert_eq!((s.translated_underruns, s.translated_dropped), (5, 6));
        assert_eq!((s.translated_gap_ms, s.translated_worst_gap_ms), (800, 300));
        assert_eq!((s.translated_peak, s.translated_rms), (0.75, 0.5));
        assert!(s.input_gated);
        assert_eq!(s.gated_frames, 12);
    }

    #[test]
    fn the_mixer_mode_follows_what_the_machine_says_and_the_local_output_mode() {
        use LocalOutputMode as L;
        use OutputSource as S;
        for local in [L::Translation, L::Both, L::Original] {
            // The remote party never hears the original lowered under the translation.
            assert_eq!(mix_mode(PathKind::Outgoing, S::Translation, local), MixMode::Translation);
            assert_eq!(mix_mode(PathKind::Outgoing, S::Original, local), MixMode::Original);
            assert_eq!(mix_mode(PathKind::Incoming, S::Original, local), MixMode::Original);
            assert_eq!(mix_mode(PathKind::Incoming, S::Nothing, local), MixMode::Translation);
        }
        assert_eq!(mix_mode(PathKind::Incoming, S::Translation, L::Translation), MixMode::Translation);
        assert_eq!(mix_mode(PathKind::Incoming, S::Translation, L::Both), MixMode::Both);
        assert_eq!(mix_mode(PathKind::Incoming, S::Translation, L::Original), MixMode::Original);
    }
}
