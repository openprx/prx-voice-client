//! Sentence memory of the translation stage: what was spoken of the current source sentence, so that the next unit is translated from the start of the sentence and speaks only what follows (incremental-retranslation 4.6).

use prxvc_media::text_units::units;
use prxvc_media::{ClauseId, SegmentId};

use crate::commit_policy::{MAX_SENTENCE_UNITS, join_sentence};

/// The part of a source sentence translated so far, and its translation from the start of the
/// sentence. Kept by the translation stage between the units of one segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SentenceMemory {
    segment: SegmentId,
    next_index: u16,
    /// The source text of the sentence so far, as the requests were made.
    source: String,
    /// The translation of `source` as a whole.
    target_full: String,
}

impl SentenceMemory {
    /// What stands after the unit `clause`, whose request had the source text `source` (the whole
    /// sentence so far) and was translated as `target_full`.
    pub(crate) fn after(clause: ClauseId, source: String, target_full: String) -> Self {
        Self {
            segment: clause.segment(),
            next_index: clause.index().saturating_add(1),
            source,
            target_full,
        }
    }

    /// Whether `clause` is the unit this memory expects next and the sentence is not too long to
    /// go on. A gap in the chain (a failed, skipped or dropped unit) or a longer sentence makes the
    /// memory useless: the unit then stands alone.
    pub(crate) fn continues_with(&self, clause: ClauseId) -> bool {
        clause.segment() == self.segment
            && clause.index() == self.next_index
            && units(&self.source).len() <= MAX_SENTENCE_UNITS
    }

    /// The source text of the request for the unit with the text `unit`: the sentence from its start.
    pub(crate) fn request_text(&self, unit: &str) -> String {
        join_sentence(&self.source, unit)
    }

    /// The translation of the sentence so far, to compare a longer translation against.
    pub(crate) fn target_full(&self) -> &str {
        &self.target_full
    }
}

#[cfg(test)]
mod tests {
    use prxvc_media::Direction;

    use super::*;

    fn clause(index: u16) -> ClauseId {
        ClauseId::new(
            SegmentId::new(Direction::Incoming, 1).expect("seq >= 1"),
            index,
        )
    }

    #[test]
    fn the_memory_expects_the_next_unit_of_the_same_segment() {
        let m = SentenceMemory::after(clause(2), "we meet".into(), "我们见面".into());
        assert!(m.continues_with(clause(3)));
        assert!(!m.continues_with(clause(2)), "the same unit again");
        assert!(!m.continues_with(clause(4)), "a gap in the chain");
        let other = ClauseId::new(SegmentId::new(Direction::Incoming, 2).expect("seq >= 1"), 3);
        assert!(!m.continues_with(other), "another segment");
    }

    #[test]
    fn a_sentence_over_the_limit_stands_alone_again() {
        let long = vec!["word"; MAX_SENTENCE_UNITS + 1].join(" ");
        let m = SentenceMemory::after(clause(0), long, "x".into());
        assert!(!m.continues_with(clause(1)));
        let ok = vec!["word"; MAX_SENTENCE_UNITS].join(" ");
        assert!(SentenceMemory::after(clause(0), ok, "x".into()).continues_with(clause(1)));
    }

    #[test]
    fn the_request_is_the_sentence_so_far_and_the_unit() {
        let m = SentenceMemory::after(clause(0), "we meet".into(), "我们见面".into());
        assert_eq!(m.request_text("at nine"), "we meet at nine");
        assert_eq!(m.target_full(), "我们见面");
    }
}
