//! Tests of the assistant's loop on the fakes of every port and an injected clock: turn taking,
//! streaming partials, interruption, stopping, filler, every typed fault and the half-duplex gate.
//! The audio of each test is pushed by the test itself, so nothing depends on how fast the loop runs.

mod faults;
mod gate;
mod replies;
mod rig;
mod turns;
