//! Lifecycle tables of a speech segment and its clauses (pipeline-design table 3.8), as pure functions of state and event kind.

use prxvc_media::{ClauseId, MonoTime};

use crate::fault::Stage;

/// The commit state of a segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SegmentState {
    /// Speech is going on; clauses can still be committed.
    Open,
    /// The segment ended; its clauses are still being processed.
    Closed,
    /// Every clause was played, skipped, failed or cancelled.
    Done,
    /// Cancelled; nothing of it may produce output any more.
    Cancelled,
}

/// An event kind of the segment table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SegmentEventKind {
    /// A recognition hypothesis (partial or final) for the segment.
    Hypothesis,
    /// A clause boundary was committed.
    CommitClause,
    /// The segment ended.
    Close,
    /// Every clause of a closed segment is terminal.
    Finish,
    /// The segment is cancelled.
    Cancel,
}

/// What an event kind does in a state, for both tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome<S> {
    /// Move to the state.
    Next(S),
    /// Legal and changes nothing.
    Stay,
    /// The target is finished or cancelled: a late result, to be dropped and counted.
    Late,
    /// Not legal in this state: a typed rejection.
    Illegal,
}

impl SegmentState {
    /// Every state, for the exhaustive tests.
    pub const ALL: [Self; 4] = [Self::Open, Self::Closed, Self::Done, Self::Cancelled];

    /// Whether the segment can no longer change.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Cancelled)
    }

    /// The segment table.
    pub const fn apply(self, kind: SegmentEventKind) -> Outcome<Self> {
        use SegmentEventKind as K;
        use SegmentState as S;
        match (self, kind) {
            (S::Done | S::Cancelled, K::Cancel) => Outcome::Stay,
            (S::Done | S::Cancelled, _) => Outcome::Late,
            (_, K::Cancel) => Outcome::Next(S::Cancelled),
            (S::Open | S::Closed, K::Hypothesis) => Outcome::Stay,
            (S::Open, K::CommitClause) => Outcome::Stay,
            (S::Open, K::Close) => Outcome::Next(S::Closed),
            (S::Closed, K::Finish) => Outcome::Next(S::Done),
            (S::Open, K::Finish) | (S::Closed, K::CommitClause | K::Close) => Outcome::Illegal,
        }
    }
}

/// The lifecycle state of a clause.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClauseState {
    /// The boundary is committed; the text is not final yet (refine pass).
    Committed,
    /// The source text is final.
    Recognized,
    /// Translation runs.
    Translating,
    /// The translation is complete.
    Translated,
    /// Synthesis runs.
    Synthesizing,
    /// Playback began.
    Playing,
    /// Playback finished.
    Played,
    /// Cancelled with its segment; if it was playing, the rest is cut.
    Cancelled,
    /// Skipped to keep up (stale, backlog); never started playing.
    Skipped,
    /// A stage failed.
    Failed(Stage),
}

/// An event kind of the clause table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClauseEventKind {
    /// The source text is final.
    Recognized,
    /// Translation was requested.
    TranslationStarted,
    /// Translation completed.
    Translated,
    /// Synthesis was requested.
    SynthesisStarted,
    /// The first synthesised audio is ready.
    AudioReady,
    /// Playout began.
    PlaybackStarted,
    /// Playout finished.
    PlaybackFinished,
    /// A stage failed.
    Failed(Stage),
    /// Drop the clause because it is stale or the pipeline lags.
    Skip,
    /// Cancel the clause.
    Cancel,
}

/// What a clause has done on the playback side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaybackState {
    /// Nothing was played.
    NotStarted,
    /// Playing now.
    Playing,
    /// Played completely.
    Done,
    /// Cut off while playing; the remainder is never replayed.
    Cut,
}

impl ClauseState {
    /// One representative of every state, for the exhaustive tests.
    pub const ALL: [Self; 10] = [
        Self::Committed,
        Self::Recognized,
        Self::Translating,
        Self::Translated,
        Self::Synthesizing,
        Self::Playing,
        Self::Played,
        Self::Cancelled,
        Self::Skipped,
        Self::Failed(Stage::Tts),
    ];

    /// Position in declaration order; written as an exhaustive `match` so that a new state does not compile until it is listed.
    pub const fn ordinal(self) -> usize {
        match self {
            Self::Committed => 0,
            Self::Recognized => 1,
            Self::Translating => 2,
            Self::Translated => 3,
            Self::Synthesizing => 4,
            Self::Playing => 5,
            Self::Played => 6,
            Self::Cancelled => 7,
            Self::Skipped => 8,
            Self::Failed(_) => 9,
        }
    }

    /// Whether the clause can no longer change or produce output.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Played | Self::Cancelled | Self::Skipped | Self::Failed(_)
        )
    }

    /// The clause table.
    ///
    /// A terminal clause answers every progress event with `Late` (a result
    /// that arrived after the clause was dropped) and ignores a repeated
    /// `Skip` or `Cancel`. A `Skip` is illegal on a playing clause: playing
    /// audio is never cut by a skip (D-P5), only by `Cancel`.
    pub const fn apply(self, kind: ClauseEventKind) -> Outcome<Self> {
        use ClauseEventKind as K;
        use ClauseState as S;
        if self.is_terminal() {
            return match kind {
                K::Skip | K::Cancel => Outcome::Stay,
                _ => Outcome::Late,
            };
        }
        match (self, kind) {
            (_, K::Cancel) => Outcome::Next(S::Cancelled),
            (S::Playing, K::Skip) => Outcome::Illegal,
            (_, K::Skip) => Outcome::Next(S::Skipped),
            (_, K::Failed(stage)) => Outcome::Next(S::Failed(stage)),
            (S::Committed, K::Recognized) => Outcome::Next(S::Recognized),
            (S::Recognized, K::TranslationStarted) => Outcome::Next(S::Translating),
            (S::Translating, K::Translated) => Outcome::Next(S::Translated),
            (S::Translated, K::SynthesisStarted) => Outcome::Next(S::Synthesizing),
            (S::Synthesizing, K::AudioReady) => Outcome::Stay,
            (S::Synthesizing, K::PlaybackStarted) => Outcome::Next(S::Playing),
            (S::Playing, K::PlaybackFinished) => Outcome::Next(S::Played),
            _ => Outcome::Illegal,
        }
    }
}

/// A read-only view of one clause, for decisions that need the queue (see `interrupt`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClauseView {
    /// The clause.
    pub clause: ClauseId,
    /// Its lifecycle state.
    pub state: ClauseState,
    /// Its playback state.
    pub playback: PlaybackState,
    /// When the speech of this clause ended.
    pub source_end: MonoTime,
}

#[cfg(test)]
mod tests;
