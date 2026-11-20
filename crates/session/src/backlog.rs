//! Backlog rule (pipeline-design 3.4.3): which queued clauses are skipped when a direction lags too far behind.
//!
//! The listener must not fall further and further behind, and content that has become stale may be
//! skipped rather than played late. What the listener is behind is not the length of the speech that
//! was spoken: translation and synthesis run several times faster than real time, so a clause
//! only waits for the clauses played before it. The rule therefore measures
//!
//! * the queued audio: for each clause that has not started playing, its real synthesised length
//!   where synthesis finished, otherwise an estimate from the length of its TARGET text at the
//!   synthesiser's speaking rate ([`estimate_speech`]); and
//! * the age of each queued clause: how long ago its source speech ended.
//!
//! A clause is skipped only when both say the direction is behind: the queued audio exceeds
//! `max_backlog` and the clause itself is older than `max_lag`. Clauses are looked at oldest first
//! and the first one that is not stale ends the pass (the ones after it are younger), so nothing is
//! reordered, a clause is not skipped merely because a short clause arrived after it, and the newest
//! clause is always kept. A clause that is playing is never cut. With `enabled` off nothing is ever
//! skipped: everything is played, however late.

use std::time::Duration;

use prxvc_media::text_units::units;
use prxvc_media::{ClauseId, Lang, MonoTime};

use crate::interrupt::CancelAction;
use crate::segment::{ClauseState, PlaybackState};

/// Thresholds of the backlog rule, shaped like the `[pipeline]` configuration keys
/// `backlog_skipping`, `max_backlog_ms` and `max_lag_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BacklogParams {
    /// Skip stale clauses at all. Off plays everything, however late.
    pub enabled: bool,
    /// Queued audio above which stale clauses are skipped.
    pub max_backlog: Duration,
    /// Age (since the end of its source speech) at which a queued clause is stale.
    pub max_lag: Duration,
}

impl Default for BacklogParams {
    /// Translation and synthesis run about five times faster than real time, so a backlog only
    /// builds when the speaker talks continuously for long stretches: 15 s of queued audio and a
    /// clause that waited 20 s are far outside ordinary conversation.
    fn default() -> Self {
        Self {
            enabled: true,
            max_backlog: Duration::from_secs(15),
            max_lag: Duration::from_secs(20),
        }
    }
}

/// Speaking rate of the synthesiser in comparison units (Chinese characters, other words) per second.
fn units_per_second(lang: Lang) -> f64 {
    match lang {
        Lang::Zh => 4.5,
        Lang::En => 2.6,
    }
}

/// How long `text` in `lang` takes to speak at the synthesiser's rate.
pub fn estimate_speech(lang: Lang, text: &str) -> Duration {
    let n = units(text).len();
    Duration::from_secs_f64(n as f64 / units_per_second(lang))
}

/// One clause as the backlog rule sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BacklogClause {
    /// The clause.
    pub clause: ClauseId,
    /// Its lifecycle state.
    pub state: ClauseState,
    /// Its playback state.
    pub playback: PlaybackState,
    /// How much audio the clause will occupy once played: the synthesised length where synthesis
    /// finished, otherwise the estimate from its target text (or, before it is translated, from
    /// its source text at the same speaking time).
    pub audio: Duration,
    /// When its source speech ended, on the pipeline clock.
    pub source_end: MonoTime,
}

impl BacklogClause {
    /// Whether the clause counts as queued, unplayed audio: not finished and not started playing
    /// (a playing clause is never skipped, D-P5).
    fn is_queued(&self) -> bool {
        !self.state.is_terminal()
            && self.state != ClauseState::Playing
            && self.playback == PlaybackState::NotStarted
    }
}

/// Whether the direction enters or leaves the "skipping to keep up" condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkippingChange {
    /// A clause was skipped: the direction is Degraded with `DegradeReason::BacklogSkipping`.
    Enter,
    /// The backlog fell below half the bound: the reason is cleared.
    Leave,
    /// No change.
    Unchanged,
}

/// The decision of the backlog rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BacklogDecision {
    /// The clauses to skip, oldest first, as `CancelAction::CancelClause`.
    pub skip: Vec<CancelAction>,
    /// The change of the skipping condition.
    pub skipping: SkippingChange,
    /// The queued audio after the skips.
    pub backlog: Duration,
    /// When the oldest clause that is not yet stale becomes stale, if the queued audio is over the
    /// bound by then: the caller checks again at that time.
    pub recheck_at: Option<MonoTime>,
}

/// Decide what to skip.
///
/// `clauses` are the clauses of the direction in playback order and `now` is the pipeline clock.
/// `skipping` tells whether the condition is on now; it ends (`Leave`) only when the queued audio is
/// below half of the bound, so the direction does not flap at the limit (CS-4.3.2).
pub fn decide_backlog(
    p: &BacklogParams,
    now: MonoTime,
    clauses: &[BacklogClause],
    skipping: bool,
) -> BacklogDecision {
    let queued: Vec<&BacklogClause> = clauses.iter().filter(|c| c.is_queued()).collect();
    let mut total: Duration = queued.iter().map(|c| c.audio).sum();
    let mut skip = Vec::new();
    let mut recheck_at = None;
    if p.enabled && total > p.max_backlog {
        for c in queued.iter().take(queued.len().saturating_sub(1)) {
            if total <= p.max_backlog {
                break;
            }
            let stale_at = c.source_end.saturating_add(p.max_lag);
            if now < stale_at {
                recheck_at = Some(stale_at);
                break;
            }
            total = total.saturating_sub(c.audio);
            skip.push(CancelAction::CancelClause(c.clause));
        }
    }
    let change = if !skipping && !skip.is_empty() {
        SkippingChange::Enter
    } else if skipping && skip.is_empty() && (!p.enabled || total < p.max_backlog / 2) {
        SkippingChange::Leave
    } else {
        SkippingChange::Unchanged
    };
    BacklogDecision {
        skip,
        skipping: change,
        backlog: total,
        recheck_at,
    }
}

#[cfg(test)]
mod tests;
