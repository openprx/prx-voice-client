//! Identifiers of the call pipeline: the direction, the segment and the clause.

use std::fmt;

/// Which way a stream of speech goes.
///
/// `Outgoing` is the local user speaking to the remote party; `Incoming` is
/// the remote party speaking to the local user.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    /// Local user to remote party.
    Outgoing,
    /// Remote party to local user.
    Incoming,
}

impl Direction {
    /// The other direction.
    pub const fn opposite(self) -> Self {
        match self {
            Self::Outgoing => Self::Incoming,
            Self::Incoming => Self::Outgoing,
        }
    }
}

/// One speech region of one direction. `seq` starts at 1 per call session and
/// strictly increases per direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SegmentId {
    direction: Direction,
    seq: u64,
}

impl SegmentId {
    /// The segment `seq` of `direction`; `None` for `seq == 0`.
    pub fn new(direction: Direction, seq: u64) -> Option<Self> {
        (seq >= 1).then_some(Self { direction, seq })
    }

    /// The first segment of `direction`.
    pub const fn first(direction: Direction) -> Self {
        Self { direction, seq: 1 }
    }

    /// The segment that follows this one in the same direction.
    pub fn next(self) -> Self {
        Self {
            direction: self.direction,
            seq: self.seq.saturating_add(1),
        }
    }

    /// The direction of the segment.
    pub const fn direction(self) -> Direction {
        self.direction
    }

    /// The sequence number, from 1.
    pub const fn seq(self) -> u64 {
        self.seq
    }
}

impl fmt::Display for SegmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = match self.direction {
            Direction::Outgoing => "out",
            Direction::Incoming => "in",
        };
        write!(f, "{d}#{}", self.seq)
    }
}

/// One clause of a segment; `index` counts from 0 in speaking order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClauseId {
    segment: SegmentId,
    index: u16,
}

impl ClauseId {
    /// The clause `index` of `segment`.
    pub const fn new(segment: SegmentId, index: u16) -> Self {
        Self { segment, index }
    }

    /// The clause that follows this one; `None` when the index would overflow.
    pub fn next(self) -> Option<Self> {
        self.index.checked_add(1).map(|index| Self {
            segment: self.segment,
            index,
        })
    }

    /// The segment this clause belongs to.
    pub const fn segment(self) -> SegmentId {
        self.segment
    }

    /// The position of the clause in its segment, from 0.
    pub const fn index(self) -> u16 {
        self.index
    }
}

impl fmt::Display for ClauseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.segment, self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_sequence_starts_at_one_and_zero_is_rejected() {
        assert_eq!(SegmentId::new(Direction::Outgoing, 0), None);
        let first = SegmentId::first(Direction::Outgoing);
        assert_eq!(first.seq(), 1);
        assert_eq!(SegmentId::new(Direction::Outgoing, 1), Some(first));
        assert_eq!(first.next().seq(), 2);
        assert_eq!(first.next().direction(), Direction::Outgoing);
    }

    #[test]
    fn segments_order_by_direction_then_sequence() {
        let out1 = SegmentId::first(Direction::Outgoing);
        let in1 = SegmentId::first(Direction::Incoming);
        assert!(out1 < out1.next());
        assert!(out1.next() < in1, "direction is the major key");
        assert_eq!(Direction::Outgoing.opposite(), Direction::Incoming);
    }

    #[test]
    fn clauses_order_inside_their_segment() {
        let seg = SegmentId::first(Direction::Incoming);
        let c0 = ClauseId::new(seg, 0);
        let c1 = c0.next().expect("index 1");
        assert!(c0 < c1);
        assert!(c1 < ClauseId::new(seg.next(), 0));
        assert_eq!(c1.segment(), seg);
        assert_eq!(c1.index(), 1);
        assert_eq!(ClauseId::new(seg, u16::MAX).next(), None);
        assert_eq!(c1.to_string(), "in#1.1");
    }
}
