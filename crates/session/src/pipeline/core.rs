//! Control logic of one direction: the single writer of the direction machine and the segment book, executing their actions.
//!
//! The control thread owns a [`Core`]. Every state change goes through the
//! two pure machines (`DirectionState::on`, `SegmentBook::on`); the core holds
//! no notion of segment or direction state of its own. What it adds is the
//! bookkeeping around the clauses (text, audio waiting for its turn, tokens),
//! the execution of the machines' actions (cancel, release, report) and the
//! snapshot. It is synchronous and deterministic: tests drive it by calling
//! [`Core::handle`], without threads.
//!
//! Submodules: `segments` executes the actions of the segment book,
//! `stages` handles the events of the refine, translation and synthesis
//! stages, `audio` holds synthesised audio back for ordered release,
//! `direction` drives the direction machine and recovery, `report` builds
//! the snapshot.

mod audio;
mod direction;
mod report;
mod segments;
mod stages;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use prxvc_media::{AudioFrame, ClauseId, Envelope, MonoTime};
use prxvc_ports::{CancellationToken, VoiceSelection};

use super::events::{ClauseAudio, PipelineEvent};
use super::messages::{
    ClauseJob, Command, Commit, Inbound, MtItem, RecognitionEvent, RefineJob, TtsItem, WorkerEvent,
};
use super::params::PipelineParams;
use super::queue::{BoundedQueue, Pushed};
use super::snapshot::{
    ClauseSnapshot, ErrorSnapshot, PartialSnapshot, SkipReason, SkippedClause, SnapshotPublisher,
    Stats,
};
use super::trace::Tracer;
use super::tree::TokenTree;
use crate::direction_state::DirectionState;
use crate::interrupt::{CancelAction, InterruptTrigger, decide_interrupt};
use crate::output::DirectionConfig;
use crate::segment::ClauseEventKind;
use crate::segment_book::{SegmentBook, SegmentEvent};

/// Flags the control thread sets for the recognition thread.
#[derive(Debug, Default)]
pub(crate) struct Gate {
    /// Audio is processed only while the direction runs.
    pub(crate) active: AtomicBool,
    /// Start the next segment on a fresh stream (after a suspension).
    pub(crate) reset: AtomicBool,
    /// Reopen the engines to see whether the provider works again.
    pub(crate) probe: AtomicBool,
}

impl Gate {
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}

/// The queues and shared state the core acts on.
pub(crate) struct Wiring {
    pub(crate) tree: Arc<TokenTree>,
    pub(crate) inbox: Arc<BoundedQueue<Inbound>>,
    pub(crate) audio_in: Arc<BoundedQueue<AudioFrame>>,
    pub(crate) refine_in: Arc<BoundedQueue<Envelope<RefineJob>>>,
    pub(crate) mt_in: Arc<BoundedQueue<MtItem>>,
    pub(crate) tts_in: Arc<BoundedQueue<TtsItem>>,
    pub(crate) events: Arc<BoundedQueue<PipelineEvent>>,
    pub(crate) gate: Arc<Gate>,
    pub(crate) snapshots: SnapshotPublisher,
    pub(crate) stats: Arc<Stats>,
    pub(crate) tracer: Tracer,
}

/// Audio of a clause that is not its turn yet.
enum Held {
    Chunk(ClauseAudio),
    End,
}

/// What the core knows about a live clause besides the book's state.
struct ClauseInfo {
    token: CancellationToken,
    revision: u32,
    source_ts: MonoTime,
    /// The streaming text, the fallback of a failed refine pass.
    source_text: String,
    /// When the clause's source speech ended.
    source_end: MonoTime,
    /// The translation so far: the backlog estimate until synthesis is known.
    target_text: String,
    synthesised: Duration,
    chunks: u32,
    tts_done: bool,
    voice: Option<VoiceSelection>,
    held: VecDeque<Held>,
    held_ms: usize,
    released: bool,
}

/// The recovery of a failed direction.
struct Recovery {
    due: MonoTime,
    wait: Duration,
    probing: bool,
}

/// The control logic of one direction.
pub(crate) struct Core {
    w: Wiring,
    p: PipelineParams,
    cfg: DirectionConfig,
    state: DirectionState,
    book: SegmentBook,
    clauses: BTreeMap<ClauseId, ClauseInfo>,
    captions: VecDeque<ClauseSnapshot>,
    skips: VecDeque<SkippedClause>,
    backlog_due: Option<MonoTime>,
    partial: Option<PartialSnapshot>,
    commit_stash: Option<Commit>,
    skip_reason: Option<SkipReason>,
    skipping: bool,
    refine_failures: u32,
    last_error: Option<ErrorSnapshot>,
    recovery: Option<Recovery>,
    backlog_ms: u64,
    dirty: bool,
    finished: bool,
    events_closed: bool,
}

impl Core {
    pub(crate) fn new(w: Wiring, p: PipelineParams) -> Self {
        let cfg = DirectionConfig {
            direction: p.direction,
            policy: p.output,
        };
        Self {
            book: SegmentBook::new(p.direction),
            cfg,
            p,
            w,
            state: DirectionState::Off,
            clauses: BTreeMap::new(),
            captions: VecDeque::new(),
            skips: VecDeque::new(),
            backlog_due: None,
            partial: None,
            commit_stash: None,
            skip_reason: None,
            skipping: false,
            refine_failures: 0,
            last_error: None,
            recovery: None,
            backlog_ms: 0,
            dirty: true,
            finished: false,
            events_closed: false,
        }
    }

    /// Publish the last snapshot and close the outputs; the control thread calls it once, last.
    pub(crate) fn shutdown(&mut self) {
        self.dirty = true;
        self.after_event();
        self.w.events.close();
        self.w.snapshots.close();
        self.w.inbox.close();
    }

    /// Whether the machine reached `Off` after a stop: the thread can end.
    pub(crate) fn is_finished(&self) -> bool {
        self.finished
    }

    /// Handle one inbound item, then check the backlog and publish the snapshot if anything changed.
    pub(crate) fn handle(&mut self, inbound: Inbound) {
        match inbound {
            Inbound::Recognition(e) => self.on_recognition(e),
            Inbound::Stage(e) => self.on_stage(e),
            Inbound::Worker(w) => self.on_worker(w),
            Inbound::Command(c) => self.on_command(c),
        }
        self.after_event();
    }

    fn after_event(&mut self) {
        self.check_backlog();
        if self.dirty {
            self.dirty = false;
            let snapshot = self.snapshot();
            self.w.snapshots.publish(snapshot);
        }
    }

    fn on_recognition(&mut self, e: Envelope<RecognitionEvent>) {
        let (segment, revision, onset) = (e.segment, e.revision, e.source_ts);
        match e.payload {
            RecognitionEvent::SpeechStarted => {
                self.apply_segment(SegmentEvent::SpeechStarted { at: onset });
                let live = self
                    .w
                    .tree
                    .segment(segment)
                    .is_some_and(|t| !t.is_cancelled());
                if live {
                    self.barge_in(segment, onset);
                } else {
                    // Cancelled before the book heard of it: the recogniser
                    // abandoned it, so it must not stay open in the book.
                    self.cancel_with(CancelAction::CancelSegment(segment), SkipReason::Stale);
                }
            }
            RecognitionEvent::Hypothesis {
                text,
                stable_prefix_len,
            } => {
                if self.apply_segment(SegmentEvent::Hypothesis { segment, revision }) {
                    self.partial = Some(PartialSnapshot {
                        segment,
                        revision,
                        text,
                        stable_prefix_len,
                    });
                }
            }
            RecognitionEvent::Commit(commit) => {
                let source_end = commit.source_end;
                self.commit_stash = Some(commit);
                self.apply_segment(SegmentEvent::CommitClause {
                    segment,
                    revision,
                    source_end,
                });
                self.commit_stash = None;
                self.partial = None;
            }
            RecognitionEvent::EndSegment => {
                self.partial = None;
                self.apply_segment(SegmentEvent::EndSegment(segment));
            }
        }
        self.dirty = true;
    }

    /// New speech of the same speaker: drop clauses that lag too far behind (D-P5).
    fn barge_in(&mut self, segment: prxvc_media::SegmentId, at: MonoTime) {
        let actions = decide_interrupt(
            InterruptTrigger::SameSpeakerNewSegment {
                new_segment: segment,
            },
            &self.p.interrupt,
            at,
            &self.book.clause_views(),
        );
        for action in actions {
            self.cancel_with(action, SkipReason::Stale);
        }
    }

    /// Feed a cancellation to the book, remembering why a clause is skipped.
    fn cancel_with(&mut self, action: CancelAction, reason: SkipReason) {
        self.skip_reason = Some(reason);
        self.apply_segment(SegmentEvent::Cancel(action));
        self.skip_reason = None;
    }

    fn on_command(&mut self, c: Command) {
        match c {
            Command::Start => self.direction_event(crate::direction_state::DirectionEvent::Start),
            Command::Pause => self.direction_event(crate::direction_state::DirectionEvent::Pause),
            Command::Resume => self.direction_event(crate::direction_state::DirectionEvent::Resume),
            Command::Suspend => {
                self.direction_event(crate::direction_state::DirectionEvent::Suspend);
            }
            Command::Skip => {
                for a in decide_interrupt(
                    InterruptTrigger::SkipRequested(self.p.direction),
                    &self.p.interrupt,
                    MonoTime::ZERO,
                    &[],
                ) {
                    self.cancel_with(a, SkipReason::Stale);
                }
            }
            Command::CancelSegment(s) => {
                self.cancel_with(CancelAction::CancelSegment(s), SkipReason::Stale);
            }
            Command::Retry => self.begin_retry(),
            Command::PlaybackStarted { clause, at } => self.playback_started(clause, at),
            Command::PlaybackFinished { clause, at } => self.playback_finished(clause, at),
            Command::Joined { clean } => self.joined(clean),
            Command::Stop => self.direction_event(crate::direction_state::DirectionEvent::Stop),
        }
        self.dirty = true;
    }

    /// Apply a clause event to the book and execute what it asks; `false`
    /// when the book refused or the item was late.
    fn apply_clause(&mut self, clause: ClauseId, kind: ClauseEventKind) -> bool {
        self.apply_segment(SegmentEvent::Clause { clause, kind })
    }

    /// Put an event on the caller's queue. Blocks while the caller does not
    /// drain; ends when the queue closes (stop) or the call is cancelled.
    fn emit(&mut self, event: PipelineEvent) {
        if self.events_closed {
            return;
        }
        let call = self.w.tree.call().clone();
        match self.w.events.push(event, Some(&call)) {
            Pushed::Accepted | Pushed::Evicted(_) => {}
            Pushed::Rejected(_) | Pushed::Closed(_) | Pushed::Cancelled(_) => {
                self.events_closed = true;
            }
        }
    }

    fn live_clause_job(&self, clause: ClauseId, text: String) -> Option<Envelope<ClauseJob>> {
        let info = self.clauses.get(&clause)?;
        Some(Envelope::new(
            clause.segment(),
            info.revision,
            info.source_ts,
            ClauseJob {
                clause,
                text,
                token: info.token.clone(),
            },
        ))
    }

    /// The `WorkerEvent` handler lives with the direction machine.
    fn on_worker(&mut self, w: WorkerEvent) {
        self.worker_event(w);
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests;
