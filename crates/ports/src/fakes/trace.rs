//! Fake trace sink that keeps every event in memory.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::trace::{TraceEvent, TraceSink, TraceStage};

/// A [`TraceSink`] that records events for a test to read. Clones share the record.
#[derive(Clone, Debug)]
pub struct FakeTraceSink {
    wants_text: bool,
    events: Arc<Mutex<Vec<TraceEvent>>>,
}

impl FakeTraceSink {
    /// A sink that asks for text (or not) in the events it receives.
    pub fn new(wants_text: bool) -> Self {
        Self {
            wants_text,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Every event recorded, oldest first.
    pub fn events(&self) -> Vec<TraceEvent> {
        self.events.lock().clone()
    }

    /// The stages recorded, oldest first.
    pub fn stages(&self) -> Vec<TraceStage> {
        self.events.lock().iter().map(|e| e.stage).collect()
    }
}

impl TraceSink for FakeTraceSink {
    fn wants_text(&self) -> bool {
        self.wants_text
    }

    fn record(&self, ev: TraceEvent) {
        self.events.lock().push(ev);
    }
}

#[cfg(test)]
mod tests {
    use prxvc_media::{ClauseId, Direction, MonoTime, SegmentId};

    use super::*;

    fn event(stage: TraceStage, ms: u64, text: Option<&str>) -> TraceEvent {
        TraceEvent {
            clause: ClauseId::new(SegmentId::first(Direction::Outgoing), 0),
            at: MonoTime::from_millis(ms),
            stage,
            text: text.map(str::to_owned),
        }
    }

    #[test]
    fn events_are_kept_in_order_and_shared_between_clones() {
        let sink = FakeTraceSink::new(true);
        let as_port: Arc<dyn TraceSink> = Arc::new(sink.clone());
        assert!(as_port.wants_text());
        as_port.record(event(TraceStage::SpeechStart, 0, None));
        as_port.record(event(TraceStage::ClauseCommitted, 400, Some("hello")));
        assert_eq!(
            sink.stages(),
            vec![TraceStage::SpeechStart, TraceStage::ClauseCommitted]
        );
        assert_eq!(sink.events()[1].text.as_deref(), Some("hello"));
    }

    #[test]
    fn a_sink_that_does_not_want_text_says_so() {
        assert!(!FakeTraceSink::new(false).wants_text());
    }
}
