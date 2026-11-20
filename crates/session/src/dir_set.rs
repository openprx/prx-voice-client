//! A set of directions, used where a call command or action addresses one direction or both.

use prxvc_media::Direction;

/// Zero, one or both directions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DirSet {
    outgoing: bool,
    incoming: bool,
}

impl DirSet {
    /// No direction.
    pub const NONE: Self = Self {
        outgoing: false,
        incoming: false,
    };
    /// Both directions.
    pub const BOTH: Self = Self {
        outgoing: true,
        incoming: true,
    };

    /// Only `d`.
    pub const fn one(d: Direction) -> Self {
        match d {
            Direction::Outgoing => Self {
                outgoing: true,
                incoming: false,
            },
            Direction::Incoming => Self {
                outgoing: false,
                incoming: true,
            },
        }
    }

    /// Whether `d` is in the set.
    pub const fn contains(self, d: Direction) -> bool {
        match d {
            Direction::Outgoing => self.outgoing,
            Direction::Incoming => self.incoming,
        }
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        !self.outgoing && !self.incoming
    }

    /// Everything in `self` or `other`.
    pub const fn union(self, other: Self) -> Self {
        Self {
            outgoing: self.outgoing || other.outgoing,
            incoming: self.incoming || other.incoming,
        }
    }

    /// Everything in `self` that is not in `other`.
    pub const fn minus(self, other: Self) -> Self {
        Self {
            outgoing: self.outgoing && !other.outgoing,
            incoming: self.incoming && !other.incoming,
        }
    }

    /// The members, outgoing first.
    pub fn iter(self) -> impl Iterator<Item = Direction> {
        [Direction::Outgoing, Direction::Incoming]
            .into_iter()
            .filter(move |d| self.contains(*d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_algebra() {
        let out = DirSet::one(Direction::Outgoing);
        let inc = DirSet::one(Direction::Incoming);
        assert!(DirSet::NONE.is_empty());
        assert_eq!(out.union(inc), DirSet::BOTH);
        assert_eq!(DirSet::BOTH.minus(out), inc);
        assert_eq!(out.minus(out), DirSet::NONE);
        assert!(out.contains(Direction::Outgoing) && !out.contains(Direction::Incoming));
        assert_eq!(
            DirSet::BOTH.iter().collect::<Vec<_>>(),
            vec![Direction::Outgoing, Direction::Incoming]
        );
        assert_eq!(DirSet::NONE.iter().count(), 0);
    }
}
