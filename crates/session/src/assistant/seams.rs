//! The two seams the loop needs from the host beyond the ports: where the reply's audio goes, and the half-duplex gate.
//!
//! Both are implemented by the composition root over the audio crate (`AssistantPlayer` and
//! `EchoGate`); the loop, which lives in layer L2, names neither (CS-1.5.2).

use prxvc_media::{MonoTime, PcmChunk};

/// Where the audio of a reply is played.
pub trait ReplyOutput: Send {
    /// Queue a chunk of the reply for playing.
    ///
    /// # Errors
    /// The device refused it; the text is for the user, never for the log of call content.
    fn play(&mut self, chunk: &PcmChunk) -> Result<(), String>;

    /// No more audio comes for this reply: let the device play what it holds, tail included.
    fn end_stream(&mut self);

    /// Throw away what has not been played and silence the device.
    fn flush(&mut self);

    /// Nothing is left to play.
    fn is_drained(&self) -> bool;
}

/// What the gate says about the microphone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicState {
    /// The assistant's voice may be in the microphone: its audio is dropped.
    Closed,
    /// The microphone may be listened to. `just_reopened` is true exactly once after a closed
    /// period: what the recogniser accumulated may hold the tail of the assistant's voice.
    Open {
        /// The gate has just reopened.
        just_reopened: bool,
    },
}

/// Half duplex: keeps the assistant's own voice out of the recogniser while it plays.
pub trait MicGate: Send {
    /// Reply audio is about to play or plays now.
    fn reply_activity(&mut self, now: MonoTime);

    /// The reply's audio is complete; what is queued still keeps the gate closed.
    fn reply_ended(&mut self, now: MonoTime);

    /// The reply was cut and the output flushed.
    fn flushed(&mut self, now: MonoTime);

    /// Tell the gate whether the output is drained; called every turn of the loop.
    fn update(&mut self, output_drained: bool, now: MonoTime);

    /// The state of the microphone now.
    fn poll(&mut self, now: MonoTime) -> MicState;
}

/// A gate that never closes: headphones, or no echo protection.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpenGate;

impl MicGate for OpenGate {
    fn reply_activity(&mut self, _now: MonoTime) {}
    fn reply_ended(&mut self, _now: MonoTime) {}
    fn flushed(&mut self, _now: MonoTime) {}
    fn update(&mut self, _output_drained: bool, _now: MonoTime) {}
    fn poll(&mut self, _now: MonoTime) -> MicState {
        MicState::Open {
            just_reopened: false,
        }
    }
}
