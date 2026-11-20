//! Where the stage timestamps of a call go.
//!
//! The product's default records nothing that leaves a trace: call content never reaches a
//! log, and timings are written only when tracing is switched on explicitly.

use prxvc_ports::{TraceEvent, TraceSink};

/// Drops every event and never asks for text.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NullTrace;

impl TraceSink for NullTrace {
    fn wants_text(&self) -> bool {
        false
    }

    fn record(&self, _event: TraceEvent) {}
}
