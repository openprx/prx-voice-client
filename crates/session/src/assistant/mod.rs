//! The assistant's voice loop (work package CT-20): click the orb, talk, hear the answer.
//!
//! One loop thread owns the microphone and the state machine and runs the direction pipeline's own
//! recognition stage (voice activity, segments, streaming recognition, the filler filter); two
//! short-lived workers per reply stream the dialogue backend's text into the synthesiser clause by
//! clause. The loop plays the audio through the host's output and keeps the half-duplex gate in step.
//!
//! * `machine`: the pure state machine (idle, listening, thinking, speaking) with its table.
//! * `engine`: [`start`], the loop and [`AssistantHandle`].
//! * `reply`: the two workers of one reply.
//! * `events`: what the loop tells the host, and the commands it takes.
//! * `fault`: the typed faults.
//! * `seams`: the output and the gate the host provides.
//! * `record`: the session record the History window shows.
//!
//! Everything a provider does is behind a `prxvc-ports` trait; a provider that cannot serve is a
//! typed fault, never a stand-in.

mod engine;
mod events;
mod fault;
mod machine;
mod record;
mod reply;
mod seams;

#[cfg(test)]
mod tests;

pub use engine::{
    AssistantDeps, AssistantHandle, AssistantParams, StartError, recognition_params, start,
};
pub use events::{AssistantEvent, Phase};
pub use fault::AssistantFault;
pub use machine::{AssistantAction, AssistantInput, AssistantState};
pub use record::SessionRecorder;
pub use seams::{MicGate, MicState, OpenGate, ReplyOutput};
