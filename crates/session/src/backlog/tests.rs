//! Tests of the backlog rule with explicit durations; no clock is involved.

use prxvc_media::{Direction, SegmentId};

use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn clause(i: u16, state: ClauseState, playback: PlaybackState, audio: u64) -> BacklogClause {
    BacklogClause {
        clause: ClauseId::new(SegmentId::first(Direction::Outgoing), i),
        state,
        playback,
        audio: ms(audio),
    }
}

fn queued(i: u16, audio: u64) -> BacklogClause {
    clause(i, ClauseState::Translated, PlaybackState::NotStarted, audio)
}

fn skipped_indexes(d: &BacklogDecision) -> Vec<u16> {
    d.skip
        .iter()
        .map(|a| match a {
            CancelAction::CancelClause(c) => c.index(),
            other => panic!("unexpected action {other:?}"),
        })
        .collect()
}

#[test]
fn below_the_bound_nothing_is_skipped() {
    let d = decide_backlog(ms(6_000), &[queued(0, 2_000), queued(1, 3_000)], false);
    assert!(d.skip.is_empty());
    assert_eq!(d.skipping, SkippingChange::Unchanged);
}

#[test]
fn above_the_bound_the_oldest_are_skipped_until_it_fits() {
    let clauses = [
        queued(0, 3_000),
        queued(1, 3_000),
        queued(2, 3_000),
        queued(3, 3_000),
    ];
    let d = decide_backlog(ms(6_000), &clauses, false);
    assert_eq!(skipped_indexes(&d), vec![0, 1]);
    assert_eq!(d.skipping, SkippingChange::Enter);
}

#[test]
fn the_newest_clause_is_kept_even_when_it_alone_exceeds_the_bound() {
    let d = decide_backlog(ms(6_000), &[queued(0, 2_000), queued(1, 9_000)], false);
    assert_eq!(skipped_indexes(&d), vec![0]);
}

#[test]
fn a_playing_or_finished_clause_is_never_counted_or_skipped() {
    let clauses = [
        clause(0, ClauseState::Playing, PlaybackState::Playing, 20_000),
        clause(1, ClauseState::Played, PlaybackState::Done, 20_000),
        queued(2, 1_000),
    ];
    let d = decide_backlog(ms(6_000), &clauses, false);
    assert!(d.skip.is_empty());
}

#[test]
fn skipping_ends_only_below_half_the_bound() {
    let max = ms(6_000);
    let between = decide_backlog(max, &[queued(0, 4_000)], true);
    assert_eq!(
        between.skipping,
        SkippingChange::Unchanged,
        "4 s is above 3 s"
    );
    let below = decide_backlog(max, &[queued(0, 2_500)], true);
    assert_eq!(below.skipping, SkippingChange::Leave);
    let not_skipping = decide_backlog(max, &[queued(0, 2_500)], false);
    assert_eq!(not_skipping.skipping, SkippingChange::Unchanged);
}

#[test]
fn skipping_that_continues_does_not_enter_twice() {
    let clauses = [queued(0, 4_000), queued(1, 4_000)];
    let d = decide_backlog(ms(6_000), &clauses, true);
    assert_eq!(skipped_indexes(&d), vec![0]);
    assert_eq!(d.skipping, SkippingChange::Unchanged);
}
