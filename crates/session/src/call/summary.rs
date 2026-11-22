//! The summary of a call, logged once when it ends: how long it ran and, per direction, what the utterances came to. Counts and figures only; no spoken text.
//!
//! The figures are the pipeline's last snapshot of each direction (taken when its pipeline stops)
//! and the path's playback figures: gaps in the translated speech (the translated lane ran dry
//! while a clause was still being synthesised or queued, never the normal end of a clause) with
//! their total and worst length, and, as diagnostics only, how often the pump fed the sink late
//! (`sink_late_feeds`: one device buffer partly zero-filled, a click at worst) and how often the
//! capture lane starved (`capture_lane_starved`: the capture delivered nothing for longer than the
//! lane's queue held), plus what the capture tap's re-creations cost (`tap_rebuilds`). A direction without a pipeline (audio only, or one that never started) shows only
//! the diagnostics.

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
    /// What the path's playback did.
    pub playback: PlaybackTotals,
    /// The spoken notice to the other party (the outgoing direction only).
    pub disclosure: Option<DisclosureSnapshot>,
}

/// The figures of a path's playback, from its last stats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PlaybackTotals {
    /// Gaps in the translated speech.
    pub gaps: u64,
    /// Milliseconds of silence in them, in total.
    pub gap_ms: u64,
    /// The longest gap, in milliseconds.
    pub worst_gap_ms: u64,
    /// Times the device callback found the sink's queue empty before its buffer was full: the
    /// pump did not top the 40 ms up in time (diagnostic; at most one device buffer of silence).
    pub sink_dry: u64,
    /// Times the capture (original) lane starved (diagnostic: silent while a translation plays).
    pub lane_dry: u64,
    /// Re-creations of the capture tap after silence no signal ever preceded.
    pub tap_silence: u64,
    /// Re-creations of the capture tap after a stall.
    pub tap_stall: u64,
    /// Creations of the capture tap because the application came back.
    pub tap_return: u64,
    /// The longest capture interruption by a re-creation, in milliseconds.
    pub capture_interruption_ms: u64,
}

impl PlaybackTotals {
    pub(super) fn of(x: &super::host::PathStats) -> Self {
        Self {
            gaps: x.translated_underruns,
            gap_ms: x.translated_gap_ms,
            worst_gap_ms: x.translated_worst_gap_ms,
            sink_dry: x.sink_underruns,
            lane_dry: x.lane_underruns,
            tap_silence: x.tap_rebuilds_silence,
            tap_stall: x.tap_rebuilds_stall,
            tap_return: x.tap_rebuilds_return,
            capture_interruption_ms: x.capture_interruption_ms,
        }
    }

    fn text(&self) -> String {
        format!(
            "underruns={} gap_ms(total/worst)={}/{} {}",
            self.gaps,
            self.gap_ms,
            self.worst_gap_ms,
            self.diagnostics()
        )
    }

    /// The diagnostic counters, which are not audible gaps.
    fn diagnostics(&self) -> String {
        format!(
            "sink_late_feeds={} capture_lane_starved={} tap_rebuilds(silence/stall/return)={}/{}/{} \
             capture_interruption_ms={}",
            self.sink_dry,
            self.lane_dry,
            self.tap_silence,
            self.tap_stall,
            self.tap_return,
            self.capture_interruption_ms
        )
    }
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
            return format!("no translation, {}{notice}", self.playback.diagnostics());
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
             skips(stale={} backlog={} other={}) passed_through={} self_echo_dropped={} suspects={} flags({}) \
             mt_retries={} synth_runaways={} {} \
             start_to_first_audio_ms(median/p95)={} end_to_first_audio_ms(median/p95)={} \
             queue_wait_ms(median/p95)={} catch_up(clauses={} max_speed={}%) \
             retranslation(units={} drafts={} hits={} stale={} failed={} src_rewrites={} \
             continued={} diverged={} empty={}) \
             first_commit_ms(median/p95)={} draft_ms(median/p95)={}{}",
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
            c.self_echo_dropped,
            c.mt_suspect,
            if flags.is_empty() { "-".to_owned() } else { flags.join(",") },
            c.mt_retries,
            c.runaway_synth,
            self.playback.text(),
            percentiles(s.ledger.start_to_first_audio),
            percentiles(s.ledger.end_to_first_audio),
            percentiles(s.ledger.queue_wait),
            s.ledger.sped_clauses,
            s.ledger.max_speed_pct,
            c.retranslated_units,
            c.drafts_engine_calls,
            c.drafts_cache_hits,
            c.drafts_stale,
            c.drafts_failed,
            c.retranslate_src_rewrites,
            c.continued,
            c.continuation_diverged,
            c.remainder_empty,
            percentiles(s.ledger.first_commit),
            percentiles(s.ledger.draft_ms),
            notice,
        )
    }
}

impl Coordinator {
    /// Remember what a direction's pipeline came to, as it stops (its snapshot is gone afterwards).
    pub(super) fn capture_totals(&mut self, d: Direction) {
        let lane = &self.lanes[slot(d)];
        let pipeline = lane.running.as_ref().and_then(|r| r.snapshot());
        let playback = lane.stats.as_ref().map(PlaybackTotals::of).unwrap_or_default();
        let disclosure = (d == Direction::Outgoing)
            .then(|| {
                self.disclosure
                    .as_ref()
                    .map(|n| n.snapshot(self.call_began))
            })
            .flatten();
        self.lanes[slot(d)].totals = DirectionTotals {
            pipeline,
            playback,
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
                queue_wait: Percentiles {
                    n: 5,
                    median_ms: 1500,
                    p95_ms: 3900,
                },
                first_commit: Percentiles {
                    n: 6,
                    median_ms: 1_900,
                    p95_ms: 2_700,
                },
                draft_ms: Percentiles {
                    n: 40,
                    median_ms: 85,
                    p95_ms: 130,
                },
                sped_clauses: 4,
                max_speed_pct: 125,
            },
        })
    }

    #[test]
    fn the_tap_counters_of_a_path_reach_the_summary() {
        let stats = super::super::host::PathStats {
            tap_rebuilds_silence: 1,
            tap_rebuilds_stall: 2,
            tap_rebuilds_return: 3,
            capture_interruption_ms: 640,
            sink_underruns: 4,
            lane_underruns: 5,
            ..Default::default()
        };
        let totals = PlaybackTotals::of(&stats);
        assert_eq!(
            totals.text(),
            "underruns=0 gap_ms(total/worst)=0/0 sink_late_feeds=4 capture_lane_starved=5 \
             tap_rebuilds(silence/stall/return)=1/2/3 capture_interruption_ms=640"
        );
    }

    #[test]
    fn a_direction_without_a_pipeline_shows_only_the_diagnostics() {
        let t = DirectionTotals {
            pipeline: None,
            playback: PlaybackTotals {
                sink_dry: 3,
                lane_dry: 2,
                tap_stall: 1,
                capture_interruption_ms: 420,
                ..PlaybackTotals::default()
            },
            disclosure: None,
        };
        assert_eq!(
            t.line(),
            "no translation, sink_late_feeds=3 capture_lane_starved=2 \
             tap_rebuilds(silence/stall/return)=0/1/0 capture_interruption_ms=420"
        );
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
                retranslated_units: 3,
                drafts_engine_calls: 21,
                drafts_cache_hits: 9,
                drafts_stale: 4,
                drafts_failed: 2,
                retranslate_src_rewrites: 1,
                continued: 5,
                continuation_diverged: 2,
                remainder_empty: 1,
                ..Counters::default()
            })),
            playback: PlaybackTotals {
                gaps: 6,
                gap_ms: 840,
                worst_gap_ms: 310,
                sink_dry: 1,
                lane_dry: 90,
                tap_silence: 2,
                tap_stall: 0,
                tap_return: 1,
                capture_interruption_ms: 1_800,
            },
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
            "underruns=6 gap_ms(total/worst)=840/310 sink_late_feeds=1 capture_lane_starved=90",
            "tap_rebuilds(silence/stall/return)=2/0/1 capture_interruption_ms=1800",
            "start_to_first_audio_ms(median/p95)=2300/4100 (n=7)",
            "end_to_first_audio_ms(median/p95)=-",
            "queue_wait_ms(median/p95)=1500/3900 (n=5)",
            "catch_up(clauses=4 max_speed=125%)",
            "retranslation(units=3 drafts=21 hits=9 stale=4 failed=2 src_rewrites=1 continued=5 diverged=2 empty=1)",
            "first_commit_ms(median/p95)=1900/2700 (n=6)",
            "draft_ms(median/p95)=85/130 (n=40)",
            "notice(played played=2 interrupted=1)",
        ] {
            assert!(line.contains(part), "{part} missing from: {line}");
        }
    }
}
