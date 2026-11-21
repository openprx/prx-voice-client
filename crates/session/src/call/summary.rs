//! The summary of a call, logged once when it ends: how long it ran and, per direction, what the utterances came to. Counts and figures only; no spoken text.
//!
//! The figures are the pipeline's last snapshot of each direction (taken when its pipeline stops)
//! and the path's underruns. A direction without a pipeline (audio only, or one that never
//! started) shows only its underruns.

use std::sync::Arc;
use std::time::Duration;

use prxvc_media::Direction;
use prxvc_ports::MtFlag;

use super::coordinator::{BOTH, Coordinator, slot};
use super::disclosure::{DisclosureSnapshot, DisclosureState};
use crate::pipeline::{DirectionSnapshot, Percentiles};

/// What one direction did during the call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DirectionTotals {
    pub pipeline: Option<Arc<DirectionSnapshot>>,
    /// Times playback ran dry (sink and translated lane).
    pub underruns: u64,
    /// The spoken notice to the other party (the outgoing direction only).
    pub disclosure: Option<DisclosureSnapshot>,
}

fn percentiles(p: Percentiles) -> String {
    if p.n == 0 {
        "-".to_owned()
    } else {
        format!("{}/{} (n={})", p.median_ms, p.p95_ms, p.n)
    }
}

/// What became of the spoken notice, in one short text.
fn notice_text(n: &DisclosureSnapshot) -> String {
    let state = match n.state {
        DisclosureState::Off => "off",
        DisclosureState::Preparing => "never ready",
        DisclosureState::Ready => "not played",
        DisclosureState::Playing => "cut off",
        DisclosureState::Played => "played",
        DisclosureState::Failed => "failed",
    };
    format!("{state} played={} interrupted={}", n.plays, n.interrupted)
}

impl DirectionTotals {
    /// The direction's part of the summary line.
    pub(super) fn line(&self) -> String {
        let notice = match &self.disclosure {
            None => String::new(),
            Some(n) => format!(" notice({})", notice_text(n)),
        };
        let Some(s) = &self.pipeline else {
            return format!("no translation, underruns={}{notice}", self.underruns);
        };
        let c = &s.counters;
        let other_skips = c.skipped.saturating_sub(c.backlog_skips + c.stale_skips);
        let flags: Vec<String> = MtFlag::ALL
            .iter()
            .filter(|f| c.flag(**f) > 0)
            .map(|f| format!("{}={}", f.name(), c.flag(*f)))
            .collect();
        format!(
            "utterances={} clauses={} played={} failed={} filler_dropped={} unspeakable_dropped={} \
             skips(stale={} backlog={} other={}) passed_through={} suspects={} flags({}) \
             mt_retries={} synth_runaways={} underruns={} \
             start_to_first_audio_ms(median/p95)={} end_to_first_audio_ms(median/p95)={}{}",
            s.ledger.utterances,
            c.clauses_committed,
            c.clauses_played,
            c.clauses_failed,
            c.filler_dropped,
            c.unspeakable_dropped,
            c.stale_skips,
            c.backlog_skips,
            other_skips,
            c.in_target_passed,
            c.mt_suspect,
            if flags.is_empty() { "-".to_owned() } else { flags.join(",") },
            c.mt_retries,
            c.runaway_synth,
            self.underruns,
            percentiles(s.ledger.start_to_first_audio),
            percentiles(s.ledger.end_to_first_audio),
            notice,
        )
    }
}

impl Coordinator {
    /// Remember what a direction's pipeline came to, as it stops (its snapshot is gone afterwards).
    pub(super) fn capture_totals(&mut self, d: Direction) {
        let lane = &self.lanes[slot(d)];
        let pipeline = lane.running.as_ref().and_then(|r| r.snapshot());
        let underruns = lane
            .stats
            .as_ref()
            .map_or(0, |x| x.underruns + x.translated_underruns);
        let disclosure = (d == Direction::Outgoing)
            .then(|| {
                self.disclosure
                    .as_ref()
                    .map(|n| n.snapshot(self.call_began))
            })
            .flatten();
        self.lanes[slot(d)].totals = DirectionTotals {
            pipeline,
            underruns,
            disclosure,
        };
    }

    /// Log the summary of the call that just ended.
    pub(super) fn log_call_summary(&mut self) {
        let Some(began) = self.call_began.take() else {
            return;
        };
        let duration: Duration = self.now().saturating_since(began);
        let [out, inc] = BOTH.map(|d| {
            let lane = &self.lanes[slot(d)];
            lane.totals.line()
        });
        tracing::info!(
            duration_s = duration.as_secs(),
            translated = self.translate,
            outgoing = %out,
            incoming = %inc,
            "call summary"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::Counters;

    fn snapshot(counters: Counters) -> Arc<DirectionSnapshot> {
        use crate::direction_state::{DirectionState, Reasons};
        use crate::output::OutputSource;
        use crate::segment_book::Activity;
        Arc::new(DirectionSnapshot {
            version: 1,
            direction: Direction::Incoming,
            state: DirectionState::Off,
            reasons: Reasons::default(),
            output: OutputSource::Original,
            activity: Activity::Listening,
            partial: None,
            clauses: Vec::new(),
            counters,
            backlog_ms: 0,
            recent_skips: Vec::new(),
            last_error: None,
            ledger: crate::pipeline::Ledger {
                utterances: 7,
                start_to_first_audio: Percentiles {
                    n: 7,
                    median_ms: 2300,
                    p95_ms: 4100,
                },
                end_to_first_audio: Percentiles::default(),
            },
        })
    }

    #[test]
    fn a_direction_without_a_pipeline_shows_only_its_underruns() {
        let t = DirectionTotals {
            pipeline: None,
            underruns: 3,
            disclosure: None,
        };
        assert_eq!(t.line(), "no translation, underruns=3");
    }

    #[test]
    fn the_line_names_every_figure_the_owner_asked_for() {
        let t = DirectionTotals {
            pipeline: Some(snapshot(Counters {
                clauses_committed: 9,
                clauses_played: 8,
                clauses_failed: 1,
                filler_dropped: 4,
                skipped: 5,
                stale_skips: 2,
                backlog_skips: 1,
                in_target_passed: 2,
                mt_suspect: 1,
                flag_wrong_language: 3,
                mt_retries: 2,
                runaway_synth: 1,
                ..Counters::default()
            })),
            underruns: 6,
            disclosure: Some(DisclosureSnapshot {
                enabled: true,
                state: DisclosureState::Played,
                plays: 2,
                interrupted: 1,
                last_played_s: Some(9),
            }),
        };
        let line = t.line();
        for part in [
            "utterances=7",
            "clauses=9",
            "played=8",
            "failed=1",
            "filler_dropped=4",
            "skips(stale=2 backlog=1 other=2)",
            "passed_through=2",
            "suspects=1",
            "flags(wrong_language=3)",
            "mt_retries=2",
            "synth_runaways=1",
            "underruns=6",
            "start_to_first_audio_ms(median/p95)=2300/4100 (n=7)",
            "end_to_first_audio_ms(median/p95)=-",
            "notice(played played=2 interrupted=1)",
        ] {
            assert!(line.contains(part), "{part} missing from: {line}");
        }
    }
}
