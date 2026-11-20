//! The playback controls the assistant flow needs on top of the `AudioSink` port: end of a stream and an exact drain signal.

use std::time::Duration;

use prxvc_ports::AudioSink;

use crate::output::OutputStats;

/// An [`AudioSink`] that plays utterance-shaped streams.
///
/// The port itself knows only "write, flush, how much is queued". The echo
/// gate and the assistant playback driver also need to know when a reply has
/// really been played out, and the resampler must be told when a reply ends so
/// that its delayed tail is not lost.
pub trait StreamSink: AudioSink {
    /// The current stream has no more audio for now. The resampler tail is
    /// queued, and running dry afterwards is a normal end and not an underrun.
    fn end_stream(&mut self);

    /// Whether everything written has been played (nothing is queued).
    fn is_drained(&self) -> bool;

    /// Wait until [`is_drained`](Self::is_drained) or `timeout` passes.
    /// Returns whether the sink is drained.
    fn wait_drained(&self, timeout: Duration) -> bool;

    /// Underrun and overrun counters.
    fn stats(&self) -> OutputStats;
}
