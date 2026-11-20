//! Assistant playback supervision for the engine loop.
//!
//! [`PlaybackDriver`] owns the [`AssistantPlayer`] and connects it to the
//! engine and the echo gate:
//!
//! - audio chunks the transport delivers are played, and the microphone gate
//!   closes as soon as a reply starts (before any audio plays);
//! - the server's end-of-reply marker (`tts_end`) ends the output stream so
//!   the tail of the reply is flushed out of the resampler, and the session
//!   returns to listening only after the output has drained;
//! - a local interrupt flushes the output and lets the gate reopen after its
//!   hold-off.

use std::time::Instant;

use prxvc_audio::echo_gate::EchoGateHandle;
use prxvc_audio::playback::AssistantPlayer;
use prxvc_core::engine::Engine;
use prxvc_media::{PcmChunk, SampleRate};
use prxvc_protocol::transport::TtsAudioChunk;
use prxvc_session::SessionState;

/// Plays the assistant's reply and keeps the echo gate in step with it.
pub struct PlaybackDriver {
    player: AssistantPlayer,
    gate: EchoGateHandle,
    /// The output has been told the current reply has ended.
    end_signalled: bool,
    /// The session was speaking at the previous tick.
    was_speaking: bool,
}

impl PlaybackDriver {
    /// Drive `player`, gating the microphone with `gate`.
    pub fn new(player: AssistantPlayer, gate: EchoGateHandle) -> Self {
        Self {
            player,
            gate,
            end_signalled: false,
            was_speaking: false,
        }
    }

    /// The gate the audio loop listens to.
    pub fn gate(&self) -> EchoGateHandle {
        self.gate.clone()
    }

    /// Play the audio the transport has received since the last call.
    pub fn play_pending(&mut self, engine: &mut Engine, now: Instant) {
        let chunks = engine.poll_assistant_audio();
        self.play_chunks(&chunks, now);
    }

    fn play_chunks(&mut self, chunks: &[TtsAudioChunk], now: Instant) {
        for chunk in chunks {
            // Audio is arriving: close the microphone even before the device
            // has started playing it.
            self.gate.with(|g| g.reply_activity(now));
            self.end_signalled = false;
            let pcm = match pcm_chunk(chunk) {
                Ok(pcm) => pcm,
                Err(e) => {
                    tracing::warn!("assistant audio playback failed: {e}");
                    continue;
                }
            };
            if let Err(e) = self.player.play(&pcm) {
                tracing::warn!("assistant audio playback failed: {e}");
            }
        }
    }

    /// The server started a spoken reply.
    pub fn on_reply_started(&mut self, engine: &Engine, now: Instant) {
        if reply_is_audible(engine) {
            self.gate.with(|g| g.reply_activity(now));
        }
    }

    /// Housekeeping to run every loop iteration: finish a reply whose end the
    /// server announced, and keep the gate informed about the output.
    pub fn tick(&mut self, engine: &mut Engine, now: Instant) {
        let speaking = engine
            .active_session()
            .is_some_and(|s| s.state == SessionState::AssistantSpeaking);
        if speaking && !self.was_speaking && reply_is_audible(engine) {
            self.gate.with(|g| g.reply_activity(now));
        }
        self.was_speaking = speaking;

        if engine.reply_end_pending() {
            // Audio sent just before `tts_end` may not have been polled yet.
            self.play_pending(engine, now);
            if !self.end_signalled {
                self.player.end_stream();
                self.gate.with(|g| g.reply_ended(now));
                self.end_signalled = true;
            }
            if self.player.is_drained() {
                engine.playback_drained();
                self.end_signalled = false;
            }
        }

        let drained = self.player.is_drained();
        self.gate.with(|g| g.update(drained, now));
    }

    /// Silence the assistant right now (interrupt, stop): drop received but
    /// unplayed audio, flush the device, and let the gate reopen after its
    /// hold-off.
    pub fn flush(&mut self, engine: &mut Engine, now: Instant) {
        let started = Instant::now();
        let stale = engine.poll_assistant_audio().len();
        self.player.flush();
        self.end_signalled = false;
        self.gate.with(|g| g.flushed(now));
        tracing::info!(
            discarded_chunks = stale,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "assistant playback flushed"
        );
    }

    /// Whether the microphone is currently paused by the echo gate.
    pub fn mic_paused(&self, engine: &Engine, now: Instant) -> bool {
        engine.active_session().is_some() && self.gate.is_closed(now)
    }
}

/// Decode a transport chunk (mono PCM16 little-endian) into the playback
/// chunk type. A dangling odd byte is ignored.
fn pcm_chunk(chunk: &TtsAudioChunk) -> Result<PcmChunk, String> {
    let rate = SampleRate::new(chunk.sample_rate)
        .map_err(|_| "format mismatch: expected non-zero sample rate, got 0 Hz".to_owned())?;
    let samples = chunk
        .pcm16
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / i16::MAX as f32)
        .collect();
    Ok(PcmChunk { samples, rate })
}

/// Whether the active session plays its reply aloud.
fn reply_is_audible(engine: &Engine) -> bool {
    engine
        .active_session()
        .is_some_and(|s| s.config.auto_play_reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    use prxvc_audio::echo_gate::{DEFAULT_HOLD_OFF, EchoGate, GateState};
    use prxvc_audio::output::OutputStats;
    use prxvc_audio::stream_sink::StreamSink;
    use prxvc_media::AudioFrame;
    use prxvc_ports::{AudioSink, EndpointError, EndpointHealth, SinkInfo, WriteOutcome};
    use prxvc_core::bus::event_channel;
    use prxvc_protocol::Command;

    #[derive(Default)]
    struct Probe {
        drained: bool,
        ends: u32,
        flushes: u32,
    }

    struct FakeOutput(Arc<parking_lot::Mutex<Probe>>);

    impl AudioSink for FakeOutput {
        fn start(&mut self) -> Result<SinkInfo, EndpointError> {
            Ok(SinkInfo {
                rate: SampleRate::HZ_48000,
                device: "fake".to_owned(),
            })
        }
        fn write(&mut self, frame: &AudioFrame) -> WriteOutcome {
            self.0.lock().drained = false;
            WriteOutcome {
                accepted: frame.len(),
                dropped: 0,
            }
        }
        fn flush(&mut self) {
            let mut p = self.0.lock();
            p.flushes += 1;
            p.drained = true;
        }
        fn queued(&self) -> Duration {
            Duration::ZERO
        }
        fn health(&self) -> EndpointHealth {
            EndpointHealth::default()
        }
        fn stop(&mut self) {}
    }

    impl StreamSink for FakeOutput {
        fn end_stream(&mut self) {
            self.0.lock().ends += 1;
        }
        fn is_drained(&self) -> bool {
            self.0.lock().drained
        }
        fn wait_drained(&self, _timeout: Duration) -> bool {
            self.is_drained()
        }
        fn stats(&self) -> OutputStats {
            OutputStats::default()
        }
    }

    struct Rig {
        engine: Engine,
        driver: PlaybackDriver,
        probe: Arc<parking_lot::Mutex<Probe>>,
        session: uuid::Uuid,
    }

    fn rig(echo_control: bool) -> Rig {
        let probe = Arc::new(parking_lot::Mutex::new(Probe {
            drained: true,
            ..Default::default()
        }));
        let p = Arc::clone(&probe);
        let player = AssistantPlayer::new(Box::new(move || {
            Ok(Box::new(FakeOutput(Arc::clone(&p))) as Box<dyn StreamSink>)
        }));
        let gate = EchoGateHandle::new(EchoGate::new(echo_control, DEFAULT_HOLD_OFF));
        let (event_tx, _rx) = event_channel(256);
        let mut engine = Engine::new(Box::new(prxvc_protocol::mock::MockGateway::new()), event_tx);
        engine.set_external_playback(true);
        engine.initialize().expect("init");
        engine.process_command(Command::CreateSession).expect("create");
        let session = engine.active_session().expect("active").id.0;
        engine
            .process_command(Command::StartSession { session_id: session })
            .expect("start");
        Rig {
            engine,
            driver: PlaybackDriver::new(player, gate),
            probe,
            session,
        }
    }

    /// A fixed origin so `ms(n)` can be used while the rig is borrowed.
    fn origin() -> Instant {
        static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *ORIGIN.get_or_init(Instant::now)
    }

    fn ms(n: u64) -> Instant {
        origin() + Duration::from_millis(n)
    }

    fn chunk(session: uuid::Uuid) -> TtsAudioChunk {
        TtsAudioChunk {
            session_id: session,
            sample_rate: 16_000,
            pcm16: vec![0; 640],
        }
    }

    fn state(r: &Rig) -> SessionState {
        r.engine.active_session().expect("active").state
    }

    /// The mock server answers `SendText` with a full reply, ending in
    /// `tts_end`; the engine ends up speaking with the end pending.
    fn speak(r: &mut Rig) {
        r.engine
            .process_command(Command::SendText {
                session_id: r.session,
                text: "hello".into(),
            })
            .expect("send text");
        assert_eq!(state(r), SessionState::AssistantSpeaking);
        assert!(r.engine.reply_end_pending());
    }

    #[test]
    fn tts_end_ends_the_stream_and_listening_waits_for_the_drain() {
        let mut r = rig(true);
        speak(&mut r);
        let (session, t) = (r.session, ms(0));
        r.driver.play_chunks(&[chunk(session)], t);
        assert!(!r.probe.lock().drained, "audio is queued in the output");

        r.driver.tick(&mut r.engine, ms(50));
        assert_eq!(r.probe.lock().ends, 1, "end_stream reached the output");
        assert_eq!(state(&r), SessionState::AssistantSpeaking, "still playing");
        assert!(r.driver.gate.is_closed(ms(60)));

        // More ticks while queued do not end the stream again.
        r.driver.tick(&mut r.engine, ms(100));
        assert_eq!(r.probe.lock().ends, 1);
        assert_eq!(state(&r), SessionState::AssistantSpeaking);

        // The output drains: the turn is over; the microphone follows after
        // the hold-off.
        r.probe.lock().drained = true;
        r.driver.tick(&mut r.engine, ms(2_000));
        assert_eq!(state(&r), SessionState::Listening);
        assert!(r.driver.mic_paused(&r.engine, ms(2_100)));
        assert!(!r.driver.mic_paused(&r.engine, ms(2_300)));
        let reopened = r.driver.gate.poll(ms(2_400));
        assert_eq!(reopened, GateState::Open { just_reopened: true });
    }

    #[test]
    fn the_microphone_closes_when_the_reply_starts_before_any_audio_plays() {
        let mut r = rig(true);
        r.engine
            .process_command(Command::SendText {
                session_id: r.session,
                text: "hello".into(),
            })
            .expect("send text");
        // Speaking, no chunk received yet, nothing queued.
        r.driver.tick(&mut r.engine, ms(10));
        assert!(r.driver.mic_paused(&r.engine, ms(20)));
    }

    #[test]
    fn interrupt_flushes_and_the_microphone_reopens_after_the_hold_off() {
        let mut r = rig(true);
        speak(&mut r);
        let (session, t) = (r.session, ms(0));
        r.driver.play_chunks(&[chunk(session)], t);
        r.driver.tick(&mut r.engine, ms(10));
        assert!(r.driver.mic_paused(&r.engine, ms(20)));

        r.engine
            .process_command(Command::InterruptPlayback { session_id: session })
            .expect("interrupt");
        r.driver.flush(&mut r.engine, ms(500));
        assert_eq!(state(&r), SessionState::Listening);
        assert_eq!(r.probe.lock().flushes, 1);

        r.driver.tick(&mut r.engine, ms(600));
        assert!(r.driver.mic_paused(&r.engine, ms(700)));
        assert!(!r.driver.mic_paused(&r.engine, ms(800)));
        assert!(!r.engine.reply_end_pending());
    }

    #[test]
    fn without_echo_control_the_microphone_stays_open() {
        let mut r = rig(false);
        speak(&mut r);
        let (session, t) = (r.session, ms(0));
        r.driver.play_chunks(&[chunk(session)], t);
        r.driver.tick(&mut r.engine, ms(10));
        assert!(!r.driver.mic_paused(&r.engine, ms(20)));
        assert_eq!(
            r.driver.gate.poll(ms(30)),
            GateState::Open { just_reopened: false }
        );
    }

    #[test]
    fn transport_chunks_decode_to_f32_and_reject_a_zero_rate() {
        let pcm = pcm_chunk(&TtsAudioChunk {
            session_id: uuid::Uuid::nil(),
            sample_rate: 24_000,
            pcm16: vec![0xFF, 0x7F, 0x00, 0x00, 0x01],
        })
        .expect("valid chunk");
        assert_eq!(pcm.rate, SampleRate::HZ_24000);
        assert_eq!(pcm.samples, vec![1.0, 0.0], "odd trailing byte dropped");
        let zero = TtsAudioChunk {
            session_id: uuid::Uuid::nil(),
            sample_rate: 0,
            pcm16: vec![0; 4],
        };
        assert!(pcm_chunk(&zero).is_err());
    }
}
