//! The cancellation tree of one direction: epoch, segment and clause tokens, and the cancellation of each scope.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use prxvc_media::{ClauseId, SegmentId};
use prxvc_ports::CancellationToken;

struct SegmentScope {
    token: CancellationToken,
    clauses: HashMap<u16, CancellationToken>,
}

struct State {
    /// The direction scope. Cancelling a direction cancels this token and
    /// replaces it by a fresh child of the call token, so the direction can
    /// carry on with new segments.
    epoch: CancellationToken,
    segments: HashMap<u64, SegmentScope>,
}

/// call token, direction epoch, segment, clause (CS-2.4.5).
///
/// Cancelling a scope cancels every provider request below it through the
/// tokens those requests hold. Locks are held only to copy or replace tokens,
/// never across a provider call.
pub struct TokenTree {
    call: CancellationToken,
    state: Mutex<State>,
}

impl TokenTree {
    /// The tree of one direction below `call`.
    pub fn new(call: CancellationToken) -> Arc<Self> {
        let epoch = call.child_token();
        Arc::new(Self {
            call,
            state: Mutex::new(State {
                epoch,
                segments: HashMap::new(),
            }),
        })
    }

    /// The call token: cancelled by stop only.
    pub fn call(&self) -> &CancellationToken {
        &self.call
    }

    /// The current direction token.
    pub fn direction(&self) -> CancellationToken {
        self.state.lock().epoch.clone()
    }

    /// Open the scope of `segment` and return its token.
    pub fn open_segment(&self, segment: SegmentId) -> CancellationToken {
        let mut s = self.state.lock();
        let token = s.epoch.child_token();
        s.segments.insert(
            segment.seq(),
            SegmentScope {
                token: token.clone(),
                clauses: HashMap::new(),
            },
        );
        token
    }

    /// The token of a segment that is still known; `None` once it was forgotten or cancelled with its direction.
    pub fn segment(&self, segment: SegmentId) -> Option<CancellationToken> {
        self.state
            .lock()
            .segments
            .get(&segment.seq())
            .map(|s| s.token.clone())
    }

    /// The token of `clause`, created on first use below its segment's token.
    /// For a segment that is gone the token comes back already cancelled.
    pub fn clause(&self, clause: ClauseId) -> CancellationToken {
        let mut s = self.state.lock();
        match s.segments.get_mut(&clause.segment().seq()) {
            Some(scope) => {
                let parent = scope.token.clone();
                scope
                    .clauses
                    .entry(clause.index())
                    .or_insert_with(|| parent.child_token())
                    .clone()
            }
            None => {
                let dead = CancellationToken::new();
                dead.cancel();
                dead
            }
        }
    }

    /// Cancel one clause only.
    pub fn cancel_clause(&self, clause: ClauseId) {
        let token = self
            .state
            .lock()
            .segments
            .get(&clause.segment().seq())
            .and_then(|s| s.clauses.get(&clause.index()).cloned());
        if let Some(token) = token {
            token.cancel();
        }
    }

    /// Cancel a segment and everything below it.
    pub fn cancel_segment(&self, segment: SegmentId) {
        let token = self.segment(segment);
        if let Some(token) = token {
            token.cancel();
        }
    }

    /// Cancel every segment of the direction and start a new epoch.
    pub fn cancel_direction(&self) {
        let old = {
            let mut s = self.state.lock();
            let old = std::mem::replace(&mut s.epoch, self.call.child_token());
            s.segments.clear();
            old
        };
        old.cancel();
    }

    /// Forget a finished segment.
    pub fn forget_segment(&self, segment: SegmentId) {
        self.state.lock().segments.remove(&segment.seq());
    }

    /// Segments with a scope now.
    #[cfg(test)]
    pub fn open_segments(&self) -> usize {
        self.state.lock().segments.len()
    }
}

#[cfg(test)]
mod tests {
    use prxvc_media::Direction;

    use super::*;

    fn seg(n: u64) -> SegmentId {
        SegmentId::new(Direction::Outgoing, n).expect("seq >= 1")
    }

    #[test]
    fn cancelling_a_clause_leaves_its_segment_and_siblings_alone() {
        let tree = TokenTree::new(CancellationToken::new());
        let seg_token = tree.open_segment(seg(1));
        let c0 = tree.clause(ClauseId::new(seg(1), 0));
        let c1 = tree.clause(ClauseId::new(seg(1), 1));
        tree.cancel_clause(ClauseId::new(seg(1), 0));
        assert!(c0.is_cancelled());
        assert!(!c1.is_cancelled() && !seg_token.is_cancelled());
    }

    #[test]
    fn cancelling_a_segment_cancels_its_clauses_but_not_other_segments() {
        let tree = TokenTree::new(CancellationToken::new());
        tree.open_segment(seg(1));
        let other = tree.open_segment(seg(2));
        let c0 = tree.clause(ClauseId::new(seg(1), 0));
        tree.cancel_segment(seg(1));
        assert!(c0.is_cancelled());
        assert!(!other.is_cancelled());
        assert!(
            tree.clause(ClauseId::new(seg(1), 5)).is_cancelled(),
            "a clause created below a cancelled segment starts cancelled"
        );
    }

    #[test]
    fn cancelling_the_direction_cancels_everything_and_starts_a_new_epoch() {
        let call = CancellationToken::new();
        let tree = TokenTree::new(call.clone());
        let s1 = tree.open_segment(seg(1));
        let epoch = tree.direction();
        tree.cancel_direction();
        assert!(s1.is_cancelled() && epoch.is_cancelled());
        assert!(
            !tree.direction().is_cancelled(),
            "the direction can carry on"
        );
        assert!(!call.is_cancelled(), "the call is untouched");
        assert!(tree.segment(seg(1)).is_none());
        let s2 = tree.open_segment(seg(2));
        assert!(!s2.is_cancelled());
    }

    #[test]
    fn the_call_token_cancels_every_direction_scope() {
        let call = CancellationToken::new();
        let tree = TokenTree::new(call.clone());
        let s1 = tree.open_segment(seg(1));
        call.cancel();
        assert!(s1.is_cancelled());
    }

    #[test]
    fn two_trees_below_one_call_are_independent_until_the_call_is_cancelled() {
        let call = CancellationToken::new();
        let a = TokenTree::new(call.child_token());
        let b = TokenTree::new(call.child_token());
        let sa = a.open_segment(seg(1));
        let sb = b.open_segment(seg(1));
        a.cancel_direction();
        assert!(sa.is_cancelled() && !sb.is_cancelled());
    }
}
