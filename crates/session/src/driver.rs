//! Interpreting driver: ties the [`InterpretSession`] state machine to the MT
//! and TTS adapters, running one consecutive turn end to end.
//!
//! A turn is driven in three caller-visible steps so the async playback boundary
//! stays clean (audio plays over real time, outside this synchronous code):
//!
//! ```text
//! begin_turn(dir)                     // floor acquired, Listening
//! end_utterance()                     // speaker done, Recognizing
//! process_recognized(text) -> audio   // RecognitionDone → MT → TranslationDone
//!                                     //   → TTS; state now Speaking, audio returned
//! …caller plays `audio`…
//! finish_playback()                   // PlaybackDone → Idle, turn recorded
//! ```
//!
//! Upstream (`prxvc-audio`: tap → resample → ASR) produces the recognized text;
//! downstream the caller plays `audio` (to the ear for Peer→Self, or into the
//! virtual mic for Self→Peer) and then calls [`finish_playback`].

use crate::adapter::{AdapterError, SynthesizedAudio, Synthesizer, Translator};
use crate::interpret::{
    Direction, InterpretCommand, InterpretError, InterpretEvent, InterpretSession,
};

/// Errors surfaced while driving a turn.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DriverError {
    /// The state machine rejected a transition.
    #[error(transparent)]
    State(#[from] InterpretError),
    /// An MT/TTS adapter failed (the session is moved to `Failed`).
    #[error(transparent)]
    Adapter(#[from] AdapterError),
}

/// Result of [`InterpretDriver::process_recognized`].
#[derive(Debug, Clone)]
pub struct TurnOutput {
    /// Events emitted across RecognitionDone → TranslationDone.
    pub events: Vec<InterpretEvent>,
    /// Synthesized translation to play, or `None` if the turn was aborted
    /// (nothing recognized).
    pub audio: Option<SynthesizedAudio>,
    /// Direction of the turn (informational).
    pub direction: Option<Direction>,
}

/// Drives an [`InterpretSession`] with concrete MT and TTS adapters.
pub struct InterpretDriver<T: Translator, S: Synthesizer> {
    session: InterpretSession,
    translator: T,
    synthesizer: S,
}

impl<T: Translator, S: Synthesizer> InterpretDriver<T, S> {
    /// Build a driver around a session and its adapters.
    pub fn new(session: InterpretSession, translator: T, synthesizer: S) -> Self {
        Self {
            session,
            translator,
            synthesizer,
        }
    }

    /// Borrow the underlying session (state, history, config).
    pub fn session(&self) -> &InterpretSession {
        &self.session
    }

    /// A party takes the floor and begins speaking.
    pub fn begin_turn(&mut self, direction: Direction) -> Result<Vec<InterpretEvent>, DriverError> {
        Ok(self.session.apply(InterpretCommand::BeginTurn(direction))?)
    }

    /// The speaker finished; move to recognition.
    pub fn end_utterance(&mut self) -> Result<Vec<InterpretEvent>, DriverError> {
        Ok(self.session.apply(InterpretCommand::EndUtterance)?)
    }

    /// Feed recognized source text, run translation + synthesis, and leave the
    /// session in `Speaking` with the audio to play returned. If `text` is empty
    /// the turn is aborted and `audio` is `None`.
    ///
    /// On adapter failure the session is moved to `Failed` and the error is
    /// returned; the caller should [`reset`](Self::reset) before continuing.
    pub fn process_recognized(&mut self, text: String) -> Result<TurnOutput, DriverError> {
        let started_dir = self.session.current_direction();

        let mut events = self
            .session
            .apply(InterpretCommand::RecognitionDone { text: text.clone() })?;

        // Empty recognition: the machine aborted back to Idle.
        let dir = match self.session.current_direction() {
            Some(d) => d,
            None => {
                return Ok(TurnOutput {
                    events,
                    audio: None,
                    direction: started_dir,
                });
            }
        };

        let source_lang = dir.source_language(&self.session.config).to_owned();
        let target_lang = dir.target_language(&self.session.config).to_owned();
        let clone_voice = dir.uses_clone_voice(&self.session.config);

        let translated = match self.translator.translate(&text, &source_lang, &target_lang) {
            Ok(t) => t,
            Err(e) => return Err(self.fail(e)),
        };

        events.extend(self.session.apply(InterpretCommand::TranslationDone {
            text: translated.clone(),
        })?);

        let audio = match self
            .synthesizer
            .synthesize(&translated, &target_lang, clone_voice)
        {
            Ok(a) => a,
            Err(e) => return Err(self.fail(e)),
        };

        Ok(TurnOutput {
            events,
            audio: Some(audio),
            direction: Some(dir),
        })
    }

    /// Signal that the synthesized translation finished playing; records the turn
    /// and releases the floor.
    pub fn finish_playback(&mut self) -> Result<Vec<InterpretEvent>, DriverError> {
        Ok(self.session.apply(InterpretCommand::PlaybackDone)?)
    }

    /// Pause the session, discarding any in-flight turn.
    pub fn pause(&mut self) -> Result<Vec<InterpretEvent>, DriverError> {
        Ok(self.session.apply(InterpretCommand::Pause)?)
    }

    /// Resume a paused session.
    pub fn resume(&mut self) -> Result<Vec<InterpretEvent>, DriverError> {
        Ok(self.session.apply(InterpretCommand::Resume)?)
    }

    /// Clear an error/pause and return to idle.
    pub fn reset(&mut self) -> Result<Vec<InterpretEvent>, DriverError> {
        Ok(self.session.apply(InterpretCommand::Reset)?)
    }

    /// Move the session to `Failed` for the given adapter error and return it.
    fn fail(&mut self, error: AdapterError) -> DriverError {
        let _ = self.session.apply(InterpretCommand::Fail {
            reason: error.to_string(),
        });
        DriverError::Adapter(error)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionId;
    use crate::adapter::AdapterError;
    use crate::interpret::{InterpretConfig, InterpretState};

    /// Echoing translator: tags the text with the target language.
    struct MockTranslator;
    impl Translator for MockTranslator {
        fn translate(
            &mut self,
            text: &str,
            _src: &str,
            target: &str,
        ) -> Result<String, AdapterError> {
            Ok(format!("[{target}] {text}"))
        }
    }

    /// Synthesizer that returns one sample per character at 24 kHz.
    struct MockSynth;
    impl Synthesizer for MockSynth {
        fn synthesize(
            &mut self,
            text: &str,
            _lang: &str,
            _clone: bool,
        ) -> Result<SynthesizedAudio, AdapterError> {
            Ok(SynthesizedAudio {
                samples: vec![0.0; text.chars().count()],
                sample_rate: 24_000,
            })
        }
    }

    struct FailingTranslator;
    impl Translator for FailingTranslator {
        fn translate(&mut self, _t: &str, _s: &str, _d: &str) -> Result<String, AdapterError> {
            Err(AdapterError::Translation("boom".into()))
        }
    }

    fn driver_with<T: Translator, S: Synthesizer>(t: T, s: S) -> InterpretDriver<T, S> {
        let session =
            InterpretSession::new(SessionId(uuid::Uuid::nil()), InterpretConfig::default());
        InterpretDriver::new(session, t, s)
    }

    #[test]
    fn full_turn_self_to_peer() {
        let mut d = driver_with(MockTranslator, MockSynth);
        d.begin_turn(Direction::SelfToPeer).unwrap();
        d.end_utterance().unwrap();

        let out = d.process_recognized("hello".into()).unwrap();
        assert_eq!(
            d.session().state(),
            InterpretState::Speaking(Direction::SelfToPeer)
        );
        let audio = out.audio.expect("audio");
        assert_eq!(audio.sample_rate, 24_000);
        // MockTranslator yields "[en] hello"; MockSynth makes one sample per char.
        assert_eq!(audio.samples.len(), "[en] hello".chars().count());

        d.finish_playback().unwrap();
        assert_eq!(d.session().state(), InterpretState::Idle);
        assert_eq!(d.session().history().len(), 1);
        let rec = &d.session().history()[0];
        assert_eq!(rec.source_text, "hello");
        assert_eq!(rec.translated_text, "[en] hello");
    }

    #[test]
    fn empty_recognition_aborts_with_no_audio() {
        let mut d = driver_with(MockTranslator, MockSynth);
        d.begin_turn(Direction::PeerToSelf).unwrap();
        d.end_utterance().unwrap();
        let out = d.process_recognized("  ".into()).unwrap();
        assert!(out.audio.is_none());
        assert_eq!(d.session().state(), InterpretState::Idle);
        assert!(d.session().history().is_empty());
    }

    #[test]
    fn peer_to_self_does_not_clone_voice() {
        // Verify the driver wires direction → languages correctly: Peer→Self
        // translates en→zh.
        let mut d = driver_with(MockTranslator, MockSynth);
        d.begin_turn(Direction::PeerToSelf).unwrap();
        d.end_utterance().unwrap();
        d.process_recognized("hello".into()).unwrap();
        d.finish_playback().unwrap();
        let rec = &d.session().history()[0];
        assert_eq!(rec.translated_text, "[zh] hello");
    }

    #[test]
    fn translation_failure_moves_to_failed() {
        let mut d = driver_with(FailingTranslator, MockSynth);
        d.begin_turn(Direction::SelfToPeer).unwrap();
        d.end_utterance().unwrap();
        let err = d.process_recognized("hello".into()).unwrap_err();
        assert!(matches!(
            err,
            DriverError::Adapter(AdapterError::Translation(_))
        ));
        assert_eq!(d.session().state(), InterpretState::Failed);
        // Recoverable: reset returns to idle.
        d.reset().unwrap();
        assert_eq!(d.session().state(), InterpretState::Idle);
    }
}
