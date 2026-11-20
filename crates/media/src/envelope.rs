//! The envelope every item carries after segmentation, and the rule that drops results which arrive late.

use std::collections::HashMap;

use crate::ids::SegmentId;
use crate::time::MonoTime;

/// An item of one segment: the payload with its segment, revision and the
/// time speech began.
///
/// `revision` increases per segment as a recogniser revises its text; once a
/// clause is committed its revision is frozen.
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope<T> {
    /// The segment the payload belongs to.
    pub segment: SegmentId,
    /// The revision of the payload within its segment.
    pub revision: u32,
    /// Speech onset of the segment, on the pipeline clock.
    pub source_ts: MonoTime,
    /// The item.
    pub payload: T,
}

/// Orders envelopes: by segment, then by revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnvelopeKey {
    /// The segment.
    pub segment: SegmentId,
    /// The revision within the segment.
    pub revision: u32,
}

impl<T> Envelope<T> {
    /// Wrap `payload`.
    pub fn new(segment: SegmentId, revision: u32, source_ts: MonoTime, payload: T) -> Self {
        Self {
            segment,
            revision,
            source_ts,
            payload,
        }
    }

    /// The ordering key of this envelope.
    pub fn key(&self) -> EnvelopeKey {
        EnvelopeKey {
            segment: self.segment,
            revision: self.revision,
        }
    }

    /// The same envelope around a different payload.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Envelope<U> {
        Envelope {
            segment: self.segment,
            revision: self.revision,
            source_ts: self.source_ts,
            payload: f(self.payload),
        }
    }
}

/// Remembers the highest revision seen per segment and rejects older ones.
///
/// A result whose revision is lower than the last seen for its segment is
/// late and must be dropped, never re-enqueued. An equal revision is accepted
/// (a final repeats the revision of the partial it confirms).
#[derive(Clone, Debug, Default)]
pub struct RevisionTracker {
    last: HashMap<SegmentId, u32>,
}

impl RevisionTracker {
    /// A tracker that has seen nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `revision` for `segment`; `false` means the item is late.
    pub fn accept(&mut self, segment: SegmentId, revision: u32) -> bool {
        let last = self.last.entry(segment).or_insert(revision);
        if revision < *last {
            return false;
        }
        *last = revision;
        true
    }

    /// [`RevisionTracker::accept`] for an envelope.
    pub fn accept_envelope<T>(&mut self, e: &Envelope<T>) -> bool {
        self.accept(e.segment, e.revision)
    }

    /// Forget a finished segment.
    pub fn forget(&mut self, segment: SegmentId) {
        self.last.remove(&segment);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::Direction;

    fn seg(seq: u64) -> SegmentId {
        SegmentId::new(Direction::Outgoing, seq).expect("seq >= 1")
    }

    #[test]
    fn keys_order_by_segment_then_revision() {
        let t = MonoTime::ZERO;
        let a = Envelope::new(seg(1), 3, t, "a");
        let b = Envelope::new(seg(1), 4, t, "b");
        let c = Envelope::new(seg(2), 1, t, "c");
        assert!(a.key() < b.key());
        assert!(
            b.key() < c.key(),
            "a later segment sorts after any revision of an earlier one"
        );
    }

    #[test]
    fn map_keeps_the_envelope_fields() {
        let e = Envelope::new(seg(5), 2, MonoTime::from_millis(9), 21).map(|n| n * 2);
        assert_eq!(e, Envelope::new(seg(5), 2, MonoTime::from_millis(9), 42));
    }

    #[test]
    fn a_lower_revision_is_late_and_an_equal_one_is_accepted() {
        let mut tracker = RevisionTracker::new();
        assert!(tracker.accept(seg(1), 1));
        assert!(tracker.accept(seg(1), 2));
        assert!(!tracker.accept(seg(1), 1), "revision 1 after 2 is late");
        assert!(
            tracker.accept(seg(1), 2),
            "a final repeats the revision it confirms"
        );
        assert!(tracker.accept(seg(1), 3));
    }

    #[test]
    fn segments_are_tracked_independently_and_forgotten() {
        let mut tracker = RevisionTracker::new();
        assert!(tracker.accept(seg(1), 5));
        assert!(tracker.accept(seg(2), 1), "another segment starts fresh");
        assert!(!tracker.accept(seg(1), 4));
        tracker.forget(seg(1));
        assert!(tracker.accept(seg(1), 1));
    }

    #[test]
    fn envelopes_go_through_the_tracker() {
        let mut tracker = RevisionTracker::new();
        let t = MonoTime::ZERO;
        assert!(tracker.accept_envelope(&Envelope::new(seg(1), 2, t, ())));
        assert!(!tracker.accept_envelope(&Envelope::new(seg(1), 1, t, ())));
    }
}
