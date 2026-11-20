//! Tests of the backlog rule with explicit durations and times; no clock is involved.

use prxvc_media::{Direction, SegmentId};

use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn at(n: u64) -> MonoTime {
    MonoTime::from_millis(n)
}

fn params() -> BacklogParams {
    BacklogParams {
        enabled: true,
        max_backlog: ms(6_000),
        max_lag: ms(10_000),
    }
}

fn clause(
    i: u16,
    state: ClauseState,
    playback: PlaybackState,
    audio: u64,
    ended: u64,
) -> BacklogClause {
    BacklogClause {
        clause: ClauseId::new(SegmentId::first(Direction::Outgoing), i),
        state,
        playback,
        audio: ms(audio),
        source_end: at(ended),
    }
}

fn queued(i: u16, audio: u64, ended: u64) -> BacklogClause {
    clause(
        i,
        ClauseState::Translated,
        PlaybackState::NotStarted,
        audio,
        ended,
    )
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
fn below_the_bound_nothing_is_skipped_however_old() {
    let d = decide_backlog(
        &params(),
        at(60_000),
        &[queued(0, 2_000, 0), queued(1, 3_000, 1_000)],
        false,
    );
    assert!(d.skip.is_empty());
    assert_eq!(d.skipping, SkippingChange::Unchanged);
    assert_eq!(d.backlog, ms(5_000));
}

#[test]
fn over_the_bound_but_not_stale_nothing_is_skipped_and_a_recheck_is_asked_for() {
    let clauses = [queued(0, 4_000, 5_000), queued(1, 4_000, 6_000)];
    let d = decide_backlog(&params(), at(8_000), &clauses, false);
    assert!(d.skip.is_empty());
    assert_eq!(d.backlog, ms(8_000));
    assert_eq!(d.recheck_at, Some(at(15_000)), "5 s plus the 10 s limit");
}

#[test]
fn a_long_clause_followed_by_a_short_one_is_not_skipped() {
    // The old rule skipped the long first clause because the two together exceeded the bound.
    let clauses = [queued(0, 5_500, 4_000), queued(1, 600, 4_300)];
    let d = decide_backlog(&params(), at(4_400), &clauses, false);
    assert!(d.skip.is_empty());
    assert_eq!(d.skipping, SkippingChange::Unchanged);
}

#[test]
fn stale_clauses_are_skipped_oldest_first_until_the_audio_fits() {
    let clauses = [
        queued(0, 3_000, 1_000),
        queued(1, 3_000, 2_000),
        queued(2, 3_000, 3_000),
        queued(3, 3_000, 40_000),
    ];
    let d = decide_backlog(&params(), at(41_000), &clauses, false);
    assert_eq!(skipped_indexes(&d), vec![0, 1]);
    assert_eq!(d.skipping, SkippingChange::Enter);
    assert_eq!(d.backlog, ms(6_000));
}

#[test]
fn the_pass_stops_at_the_first_clause_that_is_not_stale_so_nothing_is_reordered() {
    let clauses = [
        queued(0, 4_000, 1_000),
        queued(1, 4_000, 30_000),
        queued(2, 4_000, 31_000),
    ];
    let d = decide_backlog(&params(), at(32_000), &clauses, false);
    assert_eq!(skipped_indexes(&d), vec![0]);
}

#[test]
fn the_newest_clause_is_kept_even_when_it_alone_exceeds_the_bound() {
    let d = decide_backlog(
        &params(),
        at(100_000),
        &[queued(0, 2_000, 0), queued(1, 9_000, 1)],
        false,
    );
    assert_eq!(skipped_indexes(&d), vec![0]);
}

#[test]
fn a_playing_or_finished_clause_is_never_counted_or_skipped() {
    let clauses = [
        clause(0, ClauseState::Playing, PlaybackState::Playing, 20_000, 0),
        clause(1, ClauseState::Played, PlaybackState::Done, 20_000, 0),
        queued(2, 1_000, 0),
    ];
    let d = decide_backlog(&params(), at(100_000), &clauses, false);
    assert!(d.skip.is_empty());
    assert_eq!(d.backlog, ms(1_000));
}

#[test]
fn disabled_never_skips_and_leaves_the_condition() {
    let p = BacklogParams {
        enabled: false,
        ..params()
    };
    let clauses = [queued(0, 20_000, 0), queued(1, 20_000, 1)];
    let d = decide_backlog(&p, at(1_000_000), &clauses, false);
    assert!(d.skip.is_empty());
    assert_eq!(d.skipping, SkippingChange::Unchanged);
    assert_eq!(d.backlog, ms(40_000));
    let d = decide_backlog(&p, at(1_000_000), &clauses, true);
    assert_eq!(d.skipping, SkippingChange::Leave);
}

#[test]
fn skipping_ends_only_below_half_the_bound() {
    let p = params();
    let between = decide_backlog(&p, at(0), &[queued(0, 4_000, 0)], true);
    assert_eq!(
        between.skipping,
        SkippingChange::Unchanged,
        "4 s is above 3 s"
    );
    let below = decide_backlog(&p, at(0), &[queued(0, 2_500, 0)], true);
    assert_eq!(below.skipping, SkippingChange::Leave);
    let not_skipping = decide_backlog(&p, at(0), &[queued(0, 2_500, 0)], false);
    assert_eq!(not_skipping.skipping, SkippingChange::Unchanged);
}

#[test]
fn skipping_that_continues_does_not_enter_twice() {
    let clauses = [queued(0, 4_000, 0), queued(1, 4_000, 1)];
    let d = decide_backlog(&params(), at(50_000), &clauses, true);
    assert_eq!(skipped_indexes(&d), vec![0]);
    assert_eq!(d.skipping, SkippingChange::Unchanged);
}

#[test]
fn speech_is_estimated_from_the_target_text() {
    assert_eq!(estimate_speech(Lang::En, ""), Duration::ZERO);
    let en = estimate_speech(Lang::En, "we should meet at the office tomorrow");
    assert!(en > ms(2_000) && en < ms(3_500), "{en:?}");
    let zh = estimate_speech(Lang::Zh, "我们明天上午去机场接客人");
    assert!(zh > ms(2_000) && zh < ms(3_500), "{zh:?}");
}

#[test]
fn defaults_leave_ordinary_conversation_alone() {
    let p = BacklogParams::default();
    assert!(p.enabled);
    assert_eq!((p.max_backlog, p.max_lag), (ms(15_000), ms(20_000)));
}
