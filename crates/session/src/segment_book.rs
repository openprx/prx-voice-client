//! Per-direction registry of segments: assigns identities, orders the release of clauses for playback, drops late results.

use std::collections::BTreeMap;

use prxvc_media::{ClauseId, Direction, MonoTime, SegmentId};

use crate::interrupt::CancelAction;
use crate::segment::{
    ClauseEventKind, ClauseState, ClauseView, Outcome, PlaybackState, SegmentEventKind,
    SegmentState,
};

/// An event of the segment book. Segments and clauses of one direction only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentEvent {
    /// The VAD opened a speech region; the book assigns the next segment id.
    SpeechStarted {
        /// Speech onset.
        at: MonoTime,
    },
    /// A recognition hypothesis for a segment.
    Hypothesis {
        /// The segment.
        segment: SegmentId,
        /// The hypothesis revision, increasing per segment.
        revision: u32,
    },
    /// A clause boundary was committed; the book assigns the clause index.
    CommitClause {
        /// The segment.
        segment: SegmentId,
        /// The hypothesis revision the clause text is frozen at.
        revision: u32,
        /// When the speech of the clause ended.
        source_end: MonoTime,
    },
    /// The segment ended (segment-end pause, length limit).
    EndSegment(SegmentId),
    /// A clause moved on in its lifecycle.
    Clause {
        /// The clause.
        clause: ClauseId,
        /// What happened to it.
        kind: ClauseEventKind,
    },
    /// An interruption decided by `interrupt::decide_interrupt`.
    Cancel(CancelAction),
}

/// Why an item was dropped as late.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LateReason {
    /// The segment finished long ago and was forgotten.
    SegmentGone,
    /// The segment was cancelled.
    SegmentCancelled,
    /// The segment finished.
    SegmentFinished,
    /// The clause is already terminal.
    ClauseFinished,
    /// The revision is lower than one already seen for the segment.
    StaleRevision,
}

/// A side effect of the segment book.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentAction {
    /// A new segment exists.
    Opened(SegmentId),
    /// A hypothesis was accepted; show it (it replaces the previous one in place).
    HypothesisAccepted {
        /// The segment.
        segment: SegmentId,
        /// Its revision.
        revision: u32,
    },
    /// A clause was committed; recognition, translation and synthesis of it may start.
    ClauseCommitted(ClauseId),
    /// A clause changed state (captions, trace).
    ClauseAdvanced {
        /// The clause.
        clause: ClauseId,
        /// Its new state.
        state: ClauseState,
    },
    /// The clause is next in playback order and its audio is ready: play it now.
    ReleaseForPlayback(ClauseId),
    /// The segment ended.
    SegmentClosed(SegmentId),
    /// Every clause of the segment is terminal.
    SegmentDone(SegmentId),
    /// Cancel provider calls, purge queues and flush playout for this scope.
    Cancel(CancelAction),
    /// An item arrived too late and was dropped; counted in `late_dropped`.
    DropLate {
        /// The segment of the item.
        segment: SegmentId,
        /// Why it was late.
        reason: LateReason,
    },
}

/// A rejected segment-book event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SegmentRejection {
    /// The event addresses the other direction.
    #[error("event for {got:?} sent to the book of {expected:?}")]
    WrongDirection {
        /// The direction of this book.
        expected: Direction,
        /// The direction of the item.
        got: Direction,
    },
    /// The segment was never opened.
    #[error("unknown segment {0}")]
    UnknownSegment(SegmentId),
    /// The segment has no such clause.
    #[error("unknown clause {0}")]
    UnknownClause(ClauseId),
    /// A new speech region started while the previous segment was still open.
    #[error("segment {0} is still open")]
    SegmentStillOpen(SegmentId),
    /// The segment table does not allow the event in the segment's state.
    #[error("segment {segment}: {kind:?} is not valid in {from:?}")]
    IllegalSegmentEvent {
        /// The segment.
        segment: SegmentId,
        /// Its state.
        from: SegmentState,
        /// The event kind.
        kind: SegmentEventKind,
    },
    /// The clause table does not allow the event in the clause's state.
    #[error("clause {clause}: {kind:?} is not valid in {from:?}")]
    IllegalClauseEvent {
        /// The clause.
        clause: ClauseId,
        /// Its state.
        from: ClauseState,
        /// The event kind.
        kind: ClauseEventKind,
    },
    /// Playback started for a clause that was not released: it is not next in order or has no audio.
    #[error("clause {0} was not released for playback")]
    NotReleased(ClauseId),
    /// A segment cannot hold more clauses.
    #[error("segment {0} has too many clauses")]
    TooManyClauses(SegmentId),
}

/// What a direction is doing, derived from its segments (a snapshot field, not a machine state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// Nothing is in flight.
    Listening,
    /// Speech or clauses before playback are in flight.
    Recognizing,
    /// A translated clause is playing.
    Speaking,
}

#[derive(Clone, Debug)]
struct ClauseRecord {
    revision: u32,
    state: ClauseState,
    playback: PlaybackState,
    audio_ready: bool,
    released: bool,
    source_end: MonoTime,
}

#[derive(Clone, Debug)]
struct SegmentRecord {
    onset: MonoTime,
    state: SegmentState,
    last_revision: Option<u32>,
    clauses: Vec<ClauseRecord>,
}

/// Where an item's segment stands.
enum Located {
    Live(u64),
    Late(LateReason),
}

/// Segments and clauses of one direction.
///
/// Rejections leave the book unchanged. Late results are not rejections:
/// they are dropped, counted and reported as [`SegmentAction::DropLate`].
#[derive(Clone, Debug)]
pub struct SegmentBook {
    direction: Direction,
    next_seq: u64,
    segments: BTreeMap<u64, SegmentRecord>,
    late_dropped: u64,
}

type Handled = Result<Vec<SegmentAction>, SegmentRejection>;

impl SegmentBook {
    /// An empty book for `direction`; the first segment is `seq` 1.
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            next_seq: 1,
            segments: BTreeMap::new(),
            late_dropped: 0,
        }
    }

    /// The direction of the book.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// How many items were dropped as late.
    pub fn late_dropped(&self) -> u64 {
        self.late_dropped
    }

    /// Apply `event`; the actions are what the owner must now do.
    pub fn on(&mut self, event: SegmentEvent) -> Handled {
        let mut actions = match event {
            SegmentEvent::SpeechStarted { at } => self.open_segment(at)?,
            SegmentEvent::Hypothesis { segment, revision } => self.hypothesis(segment, revision)?,
            SegmentEvent::CommitClause {
                segment,
                revision,
                source_end,
            } => self.commit_clause(segment, revision, source_end)?,
            SegmentEvent::EndSegment(segment) => self.end_segment(segment)?,
            SegmentEvent::Clause { clause, kind } => self.clause_event(clause, kind)?,
            SegmentEvent::Cancel(CancelAction::CancelClause(c)) => {
                self.clause_event(c, ClauseEventKind::Skip)?
            }
            SegmentEvent::Cancel(CancelAction::CancelSegment(s)) => self.cancel_segment(s)?,
            SegmentEvent::Cancel(CancelAction::CancelDirection(d)) => self.cancel_direction(d)?,
        };
        self.settle(&mut actions);
        Ok(actions)
    }

    /// The state of a live segment; `None` once it was forgotten.
    pub fn segment_state(&self, segment: SegmentId) -> Option<SegmentState> {
        self.segments.get(&segment.seq()).map(|s| s.state)
    }

    /// Speech onset of a live segment (the `source_ts` of its envelopes).
    pub fn segment_onset(&self, segment: SegmentId) -> Option<MonoTime> {
        self.segments.get(&segment.seq()).map(|s| s.onset)
    }

    /// The hypothesis revision a clause was frozen at.
    pub fn clause_revision(&self, clause: ClauseId) -> Option<u32> {
        self.clause_record(clause).map(|c| c.revision)
    }

    /// The state of a clause of a live segment.
    pub fn clause_state(&self, clause: ClauseId) -> Option<ClauseState> {
        self.clause_record(clause).map(|c| c.state)
    }

    /// The playback state of a clause of a live segment.
    pub fn playback_state(&self, clause: ClauseId) -> Option<PlaybackState> {
        self.clause_record(clause).map(|c| c.playback)
    }

    /// Every clause of every live segment, in playback order.
    pub fn clause_views(&self) -> Vec<ClauseView> {
        let mut views = Vec::new();
        for (seq, seg) in &self.segments {
            let segment = self.id(*seq);
            for (i, c) in seg.clauses.iter().enumerate() {
                let index = u16::try_from(i).unwrap_or(u16::MAX);
                views.push(ClauseView {
                    clause: ClauseId::new(segment, index),
                    state: c.state,
                    playback: c.playback,
                    source_end: c.source_end,
                });
            }
        }
        views
    }

    /// What the direction is doing now.
    pub fn activity(&self) -> Activity {
        let mut busy = false;
        for seg in self.segments.values() {
            busy |= seg.state == SegmentState::Open;
            for c in &seg.clauses {
                if c.state == ClauseState::Playing {
                    return Activity::Speaking;
                }
                busy |= !c.state.is_terminal();
            }
        }
        if busy {
            Activity::Recognizing
        } else {
            Activity::Listening
        }
    }

    fn id(&self, seq: u64) -> SegmentId {
        segment_id(self.direction, seq)
    }

    fn clause_record(&self, clause: ClauseId) -> Option<&ClauseRecord> {
        let seg = self.segments.get(&clause.segment().seq())?;
        seg.clauses.get(usize::from(clause.index()))
    }

    fn locate(&self, segment: SegmentId) -> Result<Located, SegmentRejection> {
        if segment.direction() != self.direction {
            return Err(SegmentRejection::WrongDirection {
                expected: self.direction,
                got: segment.direction(),
            });
        }
        let seq = segment.seq();
        if seq >= self.next_seq {
            return Err(SegmentRejection::UnknownSegment(segment));
        }
        Ok(match self.segments.get(&seq) {
            None => Located::Late(LateReason::SegmentGone),
            Some(s) => match s.state {
                SegmentState::Cancelled => Located::Late(LateReason::SegmentCancelled),
                SegmentState::Done => Located::Late(LateReason::SegmentFinished),
                SegmentState::Open | SegmentState::Closed => Located::Live(seq),
            },
        })
    }

    fn late(&mut self, segment: SegmentId, reason: LateReason) -> Vec<SegmentAction> {
        self.late_dropped += 1;
        vec![SegmentAction::DropLate { segment, reason }]
    }

    fn open_segment(&mut self, at: MonoTime) -> Handled {
        if let Some((seq, last)) = self.segments.iter().next_back()
            && last.state == SegmentState::Open
        {
            return Err(SegmentRejection::SegmentStillOpen(self.id(*seq)));
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.segments.insert(
            seq,
            SegmentRecord {
                onset: at,
                state: SegmentState::Open,
                last_revision: None,
                clauses: Vec::new(),
            },
        );
        Ok(vec![SegmentAction::Opened(self.id(seq))])
    }

    fn hypothesis(&mut self, segment: SegmentId, revision: u32) -> Handled {
        let seq = match self.locate(segment)? {
            Located::Late(reason) => return Ok(self.late(segment, reason)),
            Located::Live(seq) => seq,
        };
        let Some(seg) = self.segments.get_mut(&seq) else {
            return Ok(self.late(segment, LateReason::SegmentGone));
        };
        if seg.last_revision.is_some_and(|last| revision < last) {
            return Ok(self.late(segment, LateReason::StaleRevision));
        }
        seg.last_revision = Some(revision);
        Ok(vec![SegmentAction::HypothesisAccepted {
            segment,
            revision,
        }])
    }

    fn commit_clause(
        &mut self,
        segment: SegmentId,
        revision: u32,
        source_end: MonoTime,
    ) -> Handled {
        let seq = match self.locate(segment)? {
            Located::Late(reason) => return Ok(self.late(segment, reason)),
            Located::Live(seq) => seq,
        };
        let Some(seg) = self.segments.get_mut(&seq) else {
            return Ok(self.late(segment, LateReason::SegmentGone));
        };
        if seg.state.apply(SegmentEventKind::CommitClause) == Outcome::Illegal {
            return Err(SegmentRejection::IllegalSegmentEvent {
                segment,
                from: seg.state,
                kind: SegmentEventKind::CommitClause,
            });
        }
        let index = u16::try_from(seg.clauses.len())
            .map_err(|_| SegmentRejection::TooManyClauses(segment))?;
        seg.clauses.push(ClauseRecord {
            revision,
            state: ClauseState::Committed,
            playback: PlaybackState::NotStarted,
            audio_ready: false,
            released: false,
            source_end,
        });
        Ok(vec![SegmentAction::ClauseCommitted(ClauseId::new(
            segment, index,
        ))])
    }

    fn end_segment(&mut self, segment: SegmentId) -> Handled {
        let seq = match self.locate(segment)? {
            Located::Late(reason) => return Ok(self.late(segment, reason)),
            Located::Live(seq) => seq,
        };
        let Some(seg) = self.segments.get_mut(&seq) else {
            return Ok(self.late(segment, LateReason::SegmentGone));
        };
        match seg.state.apply(SegmentEventKind::Close) {
            Outcome::Next(next) => {
                seg.state = next;
                Ok(vec![SegmentAction::SegmentClosed(segment)])
            }
            _ => Err(SegmentRejection::IllegalSegmentEvent {
                segment,
                from: seg.state,
                kind: SegmentEventKind::Close,
            }),
        }
    }

    fn clause_event(&mut self, clause: ClauseId, kind: ClauseEventKind) -> Handled {
        let segment = clause.segment();
        let finishing = matches!(kind, ClauseEventKind::Skip | ClauseEventKind::Cancel);
        let seq = match self.locate(segment)? {
            Located::Late(_) if finishing => return Ok(Vec::new()),
            Located::Late(reason) => return Ok(self.late(segment, reason)),
            Located::Live(seq) => seq,
        };
        let Some(rec) = self
            .segments
            .get_mut(&seq)
            .and_then(|s| s.clauses.get_mut(usize::from(clause.index())))
        else {
            return Err(SegmentRejection::UnknownClause(clause));
        };
        let from = rec.state;
        if kind == ClauseEventKind::PlaybackStarted && !from.is_terminal() && !rec.released {
            return Err(SegmentRejection::NotReleased(clause));
        }
        match from.apply(kind) {
            Outcome::Illegal => Err(SegmentRejection::IllegalClauseEvent { clause, from, kind }),
            Outcome::Stay => {
                rec.audio_ready |= kind == ClauseEventKind::AudioReady;
                Ok(Vec::new())
            }
            Outcome::Late => Ok(self.late(segment, LateReason::ClauseFinished)),
            Outcome::Next(next) => {
                set_state(rec, next);
                let mut actions = vec![SegmentAction::ClauseAdvanced {
                    clause,
                    state: next,
                }];
                if matches!(next, ClauseState::Skipped | ClauseState::Cancelled) {
                    actions.push(SegmentAction::Cancel(CancelAction::CancelClause(clause)));
                }
                Ok(actions)
            }
        }
    }

    fn cancel_segment(&mut self, segment: SegmentId) -> Handled {
        let seq = match self.locate(segment)? {
            Located::Late(_) => return Ok(Vec::new()),
            Located::Live(seq) => seq,
        };
        let mut actions = self.cancel_live(seq);
        actions.push(SegmentAction::Cancel(CancelAction::CancelSegment(segment)));
        Ok(actions)
    }

    fn cancel_direction(&mut self, direction: Direction) -> Handled {
        if direction != self.direction {
            return Err(SegmentRejection::WrongDirection {
                expected: self.direction,
                got: direction,
            });
        }
        let live: Vec<u64> = self
            .segments
            .iter()
            .filter(|(_, s)| !s.state.is_terminal())
            .map(|(seq, _)| *seq)
            .collect();
        let mut actions = vec![SegmentAction::Cancel(CancelAction::CancelDirection(
            direction,
        ))];
        for seq in live {
            actions.extend(self.cancel_live(seq));
        }
        Ok(actions)
    }

    /// Cancel a live segment and every non-terminal clause; returns the clause notices.
    fn cancel_live(&mut self, seq: u64) -> Vec<SegmentAction> {
        let segment = self.id(seq);
        let mut actions = Vec::new();
        if let Some(seg) = self.segments.get_mut(&seq) {
            seg.state = SegmentState::Cancelled;
            for (i, rec) in seg.clauses.iter_mut().enumerate() {
                if rec.state.is_terminal() {
                    continue;
                }
                set_state(rec, ClauseState::Cancelled);
                let index = u16::try_from(i).unwrap_or(u16::MAX);
                actions.push(SegmentAction::ClauseAdvanced {
                    clause: ClauseId::new(segment, index),
                    state: ClauseState::Cancelled,
                });
            }
        }
        actions
    }

    /// Close out finished segments, forget old ones, and release the next clause for playback.
    fn settle(&mut self, actions: &mut Vec<SegmentAction>) {
        let seqs: Vec<u64> = self.segments.keys().copied().collect();
        for seq in seqs {
            let id = self.id(seq);
            if let Some(seg) = self.segments.get_mut(&seq)
                && seg.state == SegmentState::Closed
                && seg.clauses.iter().all(|c| c.state.is_terminal())
            {
                seg.state = SegmentState::Done;
                actions.push(SegmentAction::SegmentDone(id));
            }
        }
        while let Some(entry) = self.segments.first_entry() {
            if !entry.get().state.is_terminal() {
                break;
            }
            entry.remove();
        }
        self.release_head(actions);
    }

    /// Release the first non-terminal clause if its audio is ready; later clauses wait for it.
    fn release_head(&mut self, actions: &mut Vec<SegmentAction>) {
        let head = self.segments.iter_mut().find_map(|(seq, seg)| {
            seg.clauses
                .iter_mut()
                .enumerate()
                .find(|(_, c)| !c.state.is_terminal())
                .map(|(i, c)| (*seq, i, c))
        });
        let Some((seq, index, rec)) = head else {
            return;
        };
        if rec.state == ClauseState::Synthesizing && rec.audio_ready && !rec.released {
            rec.released = true;
            let index = u16::try_from(index).unwrap_or(u16::MAX);
            let clause = ClauseId::new(segment_id(self.direction, seq), index);
            actions.push(SegmentAction::ReleaseForPlayback(clause));
        }
    }
}

/// The id of segment `seq`; `seq` always comes from a segment the book opened, so it is at least 1.
fn segment_id(direction: Direction, seq: u64) -> SegmentId {
    SegmentId::new(direction, seq).unwrap_or_else(|| SegmentId::first(direction))
}

/// Set a clause state and keep its playback state in step; a playing clause that ends any way but `Played` is cut.
fn set_state(rec: &mut ClauseRecord, next: ClauseState) {
    rec.playback = match (rec.playback, next) {
        (_, ClauseState::Playing) => PlaybackState::Playing,
        (_, ClauseState::Played) => PlaybackState::Done,
        (PlaybackState::Playing, ClauseState::Cancelled | ClauseState::Failed(_)) => {
            PlaybackState::Cut
        }
        (other, _) => other,
    };
    rec.state = next;
}

#[cfg(test)]
mod tests;
