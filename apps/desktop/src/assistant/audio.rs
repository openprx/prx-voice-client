//! The assistant's devices as the loop's seams: the microphone at the detector's rate, the output that plays the reply, and the half-duplex gate over the echo gate.

use std::time::{Duration, Instant};

use prxvc_audio::call_pump::EventQueue;
use prxvc_audio::echo_gate::{EchoGate, EchoGateHandle, GateState};
use prxvc_audio::input_supervisor::PlatformInputBackend;
use prxvc_audio::playback::AssistantPlayer;
use prxvc_audio::resampled_source::ResampledSource;
use prxvc_audio::stream_sink::StreamSink;
use prxvc_audio::supervised_mic::SupervisedMic;
use prxvc_config::AppConfig;
use prxvc_media::{Clock, MonoTime, PcmChunk, SampleRate};
use prxvc_ports::AudioSource;
use prxvc_session::assistant::{MicGate, MicState, ReplyOutput};
use std::sync::Arc;

/// Plays the reply through the app's output device, opened on the first chunk.
pub(crate) struct PlayerOutput {
    player: AssistantPlayer,
}

impl PlayerOutput {
    /// The reply plays on `audio.output_device` (empty is the system default).
    pub(crate) fn new(config: &AppConfig) -> Self {
        let device = config.audio.output_device.trim().to_owned();
        Self {
            player: AssistantPlayer::new(Box::new(move || {
                let sink = if device.is_empty() {
                    prxvc_audio::cpal_output::open_default()?
                } else {
                    prxvc_audio::cpal_output::open_named(&device)?
                };
                Ok(Box::new(sink) as Box<dyn StreamSink>)
            })),
        }
    }
}

impl ReplyOutput for PlayerOutput {
    fn play(&mut self, chunk: &PcmChunk) -> Result<(), String> {
        self.player.play(chunk).map_err(|e| e.to_string())
    }

    fn end_stream(&mut self) {
        self.player.end_stream();
    }

    fn flush(&mut self) {
        self.player.flush();
    }

    fn is_drained(&self) -> bool {
        self.player.is_drained()
    }
}

/// The half-duplex gate of the loop over the audio crate's echo gate, which counts in `Instant`s:
/// the loop's clock time maps onto an `Instant` fixed when the gate is made.
pub(crate) struct EchoMicGate {
    handle: EchoGateHandle,
    origin: Instant,
    base: MonoTime,
}

impl EchoMicGate {
    /// A gate over `handle`; `now` is the loop's clock reading at this moment.
    pub(crate) fn new(handle: EchoGateHandle, now: MonoTime) -> Self {
        Self {
            handle,
            origin: Instant::now(),
            base: now,
        }
    }

    fn at(&self, now: MonoTime) -> Instant {
        self.origin + now.saturating_since(self.base)
    }
}

impl MicGate for EchoMicGate {
    fn reply_activity(&mut self, now: MonoTime) {
        let at = self.at(now);
        self.handle.with(|g| g.reply_activity(at));
    }

    fn reply_ended(&mut self, now: MonoTime) {
        let at = self.at(now);
        self.handle.with(|g| g.reply_ended(at));
    }

    fn flushed(&mut self, now: MonoTime) {
        let at = self.at(now);
        self.handle.with(|g| g.flushed(at));
    }

    fn update(&mut self, output_drained: bool, now: MonoTime) {
        let at = self.at(now);
        self.handle.with(|g| g.update(output_drained, at));
    }

    fn poll(&mut self, now: MonoTime) -> MicState {
        match self.handle.poll(self.at(now)) {
            GateState::Closed => MicState::Closed,
            GateState::Open { just_reopened } => MicState::Open { just_reopened },
        }
    }
}

/// Whether the echo gate is on: `audio.echo_mode` and, in `Auto`, the route of the output device.
pub(crate) fn echo_enabled(config: &AppConfig) -> bool {
    #[cfg(target_os = "macos")]
    {
        let route = crate::call_kit::route::current_route(&config.audio.output_device);
        crate::call_kit::route::gate_enabled(config.audio.echo_mode, route)
    }
    #[cfg(not(target_os = "macos"))]
    {
        config.audio.echo_mode != prxvc_config::EchoMode::Off
    }
}

/// The echo gate for a session started now.
pub(crate) fn echo_gate(config: &AppConfig, clock: &dyn Clock) -> EchoMicGate {
    let hold_off = Duration::from_millis(u64::from(config.audio.half_duplex_tail_ms));
    let handle = EchoGateHandle::new(EchoGate::new(echo_enabled(config), hold_off));
    EchoMicGate::new(handle, clock.now())
}

/// The microphone the input policy chooses, delivered at 16 kHz; the queue reports the device
/// the policy moved capture to.
pub(crate) fn microphone(
    config: &AppConfig,
    clock: Arc<dyn Clock>,
) -> (Box<dyn AudioSource>, EventQueue) {
    let events = EventQueue::new();
    let mic = SupervisedMic::new(
        PlatformInputBackend,
        &config.audio.input_device,
        events.clone(),
    );
    (
        Box::new(ResampledSource::new(mic, SampleRate::HZ_16000, clock)),
        events,
    )
}

#[cfg(test)]
mod tests {
    use prxvc_audio::echo_gate::DEFAULT_HOLD_OFF;

    use super::*;

    fn gate(enabled: bool) -> (EchoMicGate, EchoGateHandle) {
        let handle = EchoGateHandle::new(EchoGate::new(enabled, DEFAULT_HOLD_OFF));
        (EchoMicGate::new(handle.clone(), MonoTime::ZERO), handle)
    }

    fn at(ms: u64) -> MonoTime {
        MonoTime::from_millis(ms)
    }

    #[test]
    fn the_microphone_closes_with_the_reply_and_reopens_after_the_hold_off_once() {
        let (mut g, _) = gate(true);
        assert_eq!(
            g.poll(at(0)),
            MicState::Open {
                just_reopened: false
            }
        );
        g.reply_activity(at(10));
        assert_eq!(g.poll(at(20)), MicState::Closed);
        g.update(false, at(50));
        g.reply_ended(at(200));
        g.update(true, at(300));
        assert_eq!(
            g.poll(at(400)),
            MicState::Closed,
            "the hold-off still drops audio"
        );
        let hold = u64::try_from(DEFAULT_HOLD_OFF.as_millis()).unwrap_or(300);
        assert_eq!(
            g.poll(at(300 + hold + 50)),
            MicState::Open {
                just_reopened: true
            }
        );
        assert_eq!(
            g.poll(at(300 + hold + 80)),
            MicState::Open {
                just_reopened: false
            }
        );
    }

    #[test]
    fn a_flush_leaves_only_the_hold_off() {
        let (mut g, _) = gate(true);
        g.reply_activity(at(0));
        g.flushed(at(100));
        assert_eq!(g.poll(at(150)), MicState::Closed);
        assert_eq!(
            g.poll(at(700)),
            MicState::Open {
                just_reopened: true
            }
        );
    }

    #[test]
    fn a_disabled_gate_never_closes() {
        let (mut g, _) = gate(false);
        g.reply_activity(at(0));
        g.update(false, at(10));
        assert_eq!(
            g.poll(at(20)),
            MicState::Open {
                just_reopened: false
            }
        );
    }
}
