//! The direction snapshot of the UI contract (pipeline-design 7.2) and the latest-wins channel that publishes it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::{Condvar, Mutex};
use prxvc_media::{ClauseId, Direction, MonoTime, SegmentId};
use prxvc_ports::MtFlag;

use crate::direction_state::{DirectionState, Reasons};
use crate::fault::{FaultKind, Stage};
use crate::output::OutputSource;
use crate::segment::ClauseState;
use crate::segment_book::Activity;

pub use super::utterance::Ledger;

/// Why a queued clause was skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkipReason {
    /// New speech of the same speaker arrived and the clause lagged more than the limit (D-P5).
    Stale,
    /// The direction had more than `pipeline.max_backlog_ms` queued and the clause was older than
    /// `pipeline.max_lag_ms` (3.4.3).
    Backlog,
    /// A queue ahead of the clause was full.
    QueueFull,
    /// The recognised text is already in the language the listener hears: nothing to translate or
    /// speak, the original voice is passed through (`pipeline.skip_source_in_target`). Not a loss.
    SourceInTarget,
}

/// The recogniser's reading of the clause being spoken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartialSnapshot {
    /// The segment.
    pub segment: SegmentId,
    /// The hypothesis revision. A newer revision replaces this one in place.
    pub revision: u32,
    /// The text of the open clause (what follows the committed clauses).
    pub text: String,
    /// Length in bytes of the prefix of `text` that will not change.
    pub stable_prefix_len: usize,
}

/// One committed clause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClauseSnapshot {
    /// The clause.
    pub clause: ClauseId,
    /// Where it is in its lifecycle.
    pub state: ClauseState,
    /// The source text.
    pub source_text: String,
    /// The translation so far (complete once the clause is translated).
    pub translation: String,
    /// Set when the clause was skipped.
    pub skipped: Option<SkipReason>,
    /// Quality flags of its translation (see [`MtFlag`]); the clause was spoken all the same.
    pub flags: Vec<MtFlag>,
    /// The first translation came back in the wrong language and the clause was translated again.
    pub retried: bool,
}

/// A clause that was skipped and will not be spoken, kept after it left the captions so that the
/// listener can see what was not said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedClause {
    /// The clause.
    pub clause: ClauseId,
    /// Its source text, for the captions.
    pub source_text: String,
    /// Why it was skipped.
    pub reason: SkipReason,
    /// How long after the end of its source speech it was skipped, in milliseconds.
    pub lag_ms: u64,
}

/// The last failure of the direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorSnapshot {
    /// The stage that failed.
    pub stage: Stage,
    /// The class of the failure.
    pub fault: FaultKind,
    /// When it was seen, on the pipeline clock.
    pub at: MonoTime,
    /// A short description without call content.
    pub detail: String,
}

/// Counters of one direction since it started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    /// Results dropped because their segment or clause was cancelled, superseded or finished.
    pub late_dropped: u64,
    /// Clauses skipped for any reason.
    pub skipped: u64,
    /// Of those, by the backlog rule.
    pub backlog_skips: u64,
    /// Of those, because they were stale when new speech began.
    pub stale_skips: u64,
    /// Audio frames dropped because the recogniser could not keep up.
    pub capture_overruns: u64,
    /// Synthesis requests stopped by the runaway guard.
    pub runaway_synth: u64,
    /// Translations flagged as not being a translation.
    pub mt_suspect: u64,
    /// Clauses that fell back to the streaming text after a failed refine pass.
    pub refine_fallbacks: u64,
    /// Clauses failed because too much audio waited for its turn.
    pub hold_overflows: u64,
    /// Hypotheses that contradicted an earlier stable prefix.
    pub stable_violations: u64,
    /// Events the direction machine or the segment book refused.
    pub rejected_events: u64,
    /// Finals whose text disagreed with text already committed: the committed text stayed, only
    /// the remainder was taken from the final, and nothing was re-spoken.
    pub final_rewrites: u64,
    /// Of those, finals that could not be aligned with the committed text by content, so that the
    /// suffix after the committed length was taken.
    pub final_unaligned: u64,
    /// Finals the recogniser flagged as contradicting a prefix it had declared stable (they may
    /// or may not touch committed text).
    pub finals_contradicting_stable: u64,
    /// Segments the recogniser ended itself at its maximum length.
    pub forced_cuts: u64,
    /// Clauses committed.
    pub clauses_committed: u64,
    /// Clauses played to the end.
    pub clauses_played: u64,
    /// Clauses that ended failed.
    pub clauses_failed: u64,
    /// Recognised units that were only filler ("嗯", "Mm.") and were neither translated nor spoken
    /// (`pipeline.drop_filler`).
    pub filler_dropped: u64,
    /// Translated pieces with nothing to say aloud (punctuation or symbols only) that were never
    /// sent to the synthesiser.
    pub unspeakable_dropped: u64,
    /// Clauses already in the language the listener hears: not translated, the original passed
    /// through (`pipeline.skip_source_in_target`).
    pub in_target_passed: u64,
    /// Translations that came back in the wrong language and were requested again.
    pub mt_retries: u64,
    /// Finished translations flagged as the wrong language (reported, not acted on).
    pub flag_wrong_language: u64,
    /// Finished translations flagged as far too long.
    pub flag_too_long: u64,
    /// Finished translations that stopped at the provider's token limit.
    pub flag_truncated: u64,
    /// Finished translations that read as a refusal.
    pub flag_refusal: u64,
    /// Finished translations that read as an answer or a chat opener.
    pub flag_answer_like: u64,
    /// Finished translations with a thinking tag left in.
    pub flag_think_residue: u64,
}

impl Counters {
    /// The count of one flag kind.
    pub fn flag(&self, flag: MtFlag) -> u64 {
        match flag {
            MtFlag::WrongLanguage => self.flag_wrong_language,
            MtFlag::TooLong => self.flag_too_long,
            MtFlag::Truncated => self.flag_truncated,
            MtFlag::Refusal => self.flag_refusal,
            MtFlag::AnswerLike => self.flag_answer_like,
            MtFlag::ThinkResidue => self.flag_think_residue,
        }
    }

    /// All flagged translations of every kind.
    pub fn flags_total(&self) -> u64 {
        MtFlag::ALL.iter().map(|f| self.flag(*f)).sum()
    }
}

/// Everything the UI shows of one direction, published on change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectionSnapshot {
    /// Increases by one with every publication.
    pub version: u64,
    /// The direction.
    pub direction: Direction,
    /// The state of the direction machine.
    pub state: DirectionState,
    /// Why the direction is reduced, if it is.
    pub reasons: Reasons,
    /// What the listener hears now.
    pub output: OutputSource,
    /// Listening, recognizing or speaking.
    pub activity: Activity,
    /// The clause being spoken, with its stable prefix.
    pub partial: Option<PartialSnapshot>,
    /// The most recent clauses, oldest first.
    pub clauses: Vec<ClauseSnapshot>,
    /// Counters.
    pub counters: Counters,
    /// Audio queued for playback and not yet played: real synthesised length where known, else the
    /// speaking time of the target text.
    pub backlog_ms: u64,
    /// The most recent skipped clauses, oldest first, with their source text.
    pub recent_skips: Vec<SkippedClause>,
    /// The last failure, kept after recovery.
    pub last_error: Option<ErrorSnapshot>,
    /// The figures of the utterances finished so far (counts and latency percentiles).
    pub ledger: Ledger,
}

/// Atomic counters shared by the threads of one pipeline.
#[derive(Debug, Default)]
pub(crate) struct Stats {
    pub(crate) late_dropped: AtomicU64,
    pub(crate) skipped: AtomicU64,
    pub(crate) backlog_skips: AtomicU64,
    pub(crate) stale_skips: AtomicU64,
    pub(crate) capture_overruns: AtomicU64,
    pub(crate) runaway_synth: AtomicU64,
    pub(crate) mt_suspect: AtomicU64,
    pub(crate) refine_fallbacks: AtomicU64,
    pub(crate) hold_overflows: AtomicU64,
    pub(crate) stable_violations: AtomicU64,
    pub(crate) rejected_events: AtomicU64,
    pub(crate) final_rewrites: AtomicU64,
    pub(crate) final_unaligned: AtomicU64,
    pub(crate) finals_contradicting_stable: AtomicU64,
    pub(crate) forced_cuts: AtomicU64,
    pub(crate) clauses_committed: AtomicU64,
    pub(crate) clauses_played: AtomicU64,
    pub(crate) clauses_failed: AtomicU64,
    pub(crate) filler_dropped: AtomicU64,
    pub(crate) unspeakable_dropped: AtomicU64,
    pub(crate) in_target_passed: AtomicU64,
    pub(crate) mt_retries: AtomicU64,
    pub(crate) flag_wrong_language: AtomicU64,
    pub(crate) flag_too_long: AtomicU64,
    pub(crate) flag_truncated: AtomicU64,
    pub(crate) flag_refusal: AtomicU64,
    pub(crate) flag_answer_like: AtomicU64,
    pub(crate) flag_think_residue: AtomicU64,
    /// Frames the recogniser consumed; tests use it to pace their input.
    pub(crate) frames_consumed: AtomicU64,
}

impl Stats {
    pub(crate) fn add(counter: &AtomicU64, n: u64) {
        counter.fetch_add(n, Ordering::Relaxed);
    }

    pub(crate) fn counters(&self, book_late: u64) -> Counters {
        let get = |c: &AtomicU64| c.load(Ordering::Relaxed);
        Counters {
            late_dropped: get(&self.late_dropped) + book_late,
            skipped: get(&self.skipped),
            backlog_skips: get(&self.backlog_skips),
            stale_skips: get(&self.stale_skips),
            capture_overruns: get(&self.capture_overruns),
            runaway_synth: get(&self.runaway_synth),
            mt_suspect: get(&self.mt_suspect),
            refine_fallbacks: get(&self.refine_fallbacks),
            hold_overflows: get(&self.hold_overflows),
            stable_violations: get(&self.stable_violations),
            rejected_events: get(&self.rejected_events),
            final_rewrites: get(&self.final_rewrites),
            final_unaligned: get(&self.final_unaligned),
            finals_contradicting_stable: get(&self.finals_contradicting_stable),
            forced_cuts: get(&self.forced_cuts),
            clauses_committed: get(&self.clauses_committed),
            clauses_played: get(&self.clauses_played),
            clauses_failed: get(&self.clauses_failed),
            filler_dropped: get(&self.filler_dropped),
            unspeakable_dropped: get(&self.unspeakable_dropped),
            in_target_passed: get(&self.in_target_passed),
            mt_retries: get(&self.mt_retries),
            flag_wrong_language: get(&self.flag_wrong_language),
            flag_too_long: get(&self.flag_too_long),
            flag_truncated: get(&self.flag_truncated),
            flag_refusal: get(&self.flag_refusal),
            flag_answer_like: get(&self.flag_answer_like),
            flag_think_residue: get(&self.flag_think_residue),
        }
    }

    /// Count one flagged translation of `flag`.
    pub(crate) fn add_flag(&self, flag: MtFlag) {
        let counter = match flag {
            MtFlag::WrongLanguage => &self.flag_wrong_language,
            MtFlag::TooLong => &self.flag_too_long,
            MtFlag::Truncated => &self.flag_truncated,
            MtFlag::Refusal => &self.flag_refusal,
            MtFlag::AnswerLike => &self.flag_answer_like,
            MtFlag::ThinkResidue => &self.flag_think_residue,
        };
        Self::add(counter, 1);
    }
}

struct Slot {
    version: u64,
    snapshot: Option<Arc<DirectionSnapshot>>,
    closed: bool,
}

struct Shared {
    slot: Mutex<Slot>,
    changed: Condvar,
}

/// The writing end of the snapshot channel: one slot, the newest publication replaces the previous one.
pub(crate) struct SnapshotPublisher(Arc<Shared>);

/// A reading end of the snapshot channel; clones read the same slot.
#[derive(Clone)]
pub struct SnapshotSubscriber(Arc<Shared>);

/// A snapshot channel with one slot and latest-wins overflow (a bounded
/// channel of capacity one that drops the oldest): a slow UI sees the newest
/// state and never delays the pipeline.
pub(crate) fn snapshot_channel() -> (SnapshotPublisher, SnapshotSubscriber) {
    let shared = Arc::new(Shared {
        slot: Mutex::new(Slot {
            version: 0,
            snapshot: None,
            closed: false,
        }),
        changed: Condvar::new(),
    });
    (
        SnapshotPublisher(Arc::clone(&shared)),
        SnapshotSubscriber(shared),
    )
}

impl SnapshotPublisher {
    /// Replace the slot's content; `snapshot.version` is set here.
    pub(crate) fn publish(&self, mut snapshot: DirectionSnapshot) {
        let mut slot = self.0.slot.lock();
        slot.version += 1;
        snapshot.version = slot.version;
        slot.snapshot = Some(Arc::new(snapshot));
        drop(slot);
        self.0.changed.notify_all();
    }

    /// No further publication will come.
    pub(crate) fn close(&self) {
        self.0.slot.lock().closed = true;
        self.0.changed.notify_all();
    }
}

impl SnapshotSubscriber {
    /// The newest snapshot, if any was published.
    pub fn latest(&self) -> Option<Arc<DirectionSnapshot>> {
        self.0.slot.lock().snapshot.clone()
    }

    /// Block until a snapshot newer than version `after` exists and return
    /// it; `None` when the channel closed with nothing newer.
    pub fn wait_newer(&self, after: u64) -> Option<Arc<DirectionSnapshot>> {
        let mut slot = self.0.slot.lock();
        loop {
            if slot.version > after {
                return slot.snapshot.clone();
            }
            if slot.closed {
                return None;
            }
            self.0.changed.wait(&mut slot);
        }
    }

    /// Block until `ready` accepts a snapshot newer than the last one examined.
    /// `None` when the channel closed first.
    pub fn wait_for(
        &self,
        mut ready: impl FnMut(&DirectionSnapshot) -> bool,
    ) -> Option<Arc<DirectionSnapshot>> {
        let mut seen = 0;
        while let Some(s) = self.wait_newer(seen) {
            if ready(&s) {
                return Some(s);
            }
            seen = s.version;
        }
        None
    }
}
