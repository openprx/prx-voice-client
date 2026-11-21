//! Tests of the interruption decision: what each trigger cancels and what it keeps.

use prxvc_media::{ClauseId, Direction, MonoTime, SegmentId};

use crate::fault::Stage;

use super::*;

const OUT: Direction = Direction::Outgoing;
const IN: Direction = Direction::Incoming;

fn seg(d: Direction, seq: u64) -> SegmentId {
    SegmentId::new(d, seq).expect("seq >= 1")
}

fn view(seq: u64, index: u16, state: ClauseState, ended_ms: u64) -> ClauseView {
    let playback = match state {
        ClauseState::Playing => PlaybackState::Playing,
        ClauseState::Played => PlaybackState::Done,
        _ => PlaybackState::NotStarted,
    };
    ClauseView {
        clause: ClauseId::new(seg(OUT, seq), index),
        state,
        playback,
        source_end: MonoTime::from_millis(ended_ms),
    }
}

fn params() -> InterruptParams {
    InterruptParams::default()
}

fn decide(
    trigger: InterruptTrigger,
    p: &InterruptParams,
    now_ms: u64,
    q: &[ClauseView],
) -> Vec<CancelAction> {
    decide_interrupt(trigger, p, MonoTime::from_millis(now_ms), q)
}

#[test]
fn same_speaker_drops_only_queued_clauses_older_than_the_lag_limit() {
    let queue = [
        view(1, 0, ClauseState::Playing, 1_000),
        view(1, 1, ClauseState::Synthesizing, 2_000),
        view(1, 2, ClauseState::Translating, 6_500),
        view(2, 0, ClauseState::Committed, 7_000),
    ];
    let trigger = InterruptTrigger::SameSpeakerNewSegment {
        new_segment: seg(OUT, 3),
    };
    // now = 9_500: lags are 8_500 (playing), 7_500, 3_000 (not above the limit), 2_500.
    let got = decide(trigger, &params(), 9_500, &queue);
    assert_eq!(
        got,
        vec![CancelAction::CancelClause(ClauseId::new(seg(OUT, 1), 1))]
    );
}

#[test]
fn the_lag_limit_is_strict() {
    let queue = [view(1, 0, ClauseState::Translated, 1_000)];
    let trigger = InterruptTrigger::SameSpeakerNewSegment {
        new_segment: seg(OUT, 2),
    };
    assert!(
        decide(trigger, &params(), 4_000, &queue).is_empty(),
        "exactly 3000 ms is kept"
    );
    assert_eq!(decide(trigger, &params(), 4_001, &queue).len(), 1);
}

#[test]
fn the_playing_clause_and_terminal_clauses_are_never_cancelled_by_new_speech() {
    let queue = [
        view(1, 0, ClauseState::Played, 0),
        view(1, 1, ClauseState::Playing, 0),
        view(1, 2, ClauseState::Skipped, 0),
        view(1, 3, ClauseState::Failed(Stage::Tts), 0),
        view(1, 4, ClauseState::Cancelled, 0),
    ];
    let trigger = InterruptTrigger::SameSpeakerNewSegment {
        new_segment: seg(OUT, 2),
    };
    assert!(decide(trigger, &params(), 60_000, &queue).is_empty());
}

#[test]
fn clauses_of_the_new_segment_and_of_the_other_direction_are_kept() {
    let mut other = view(1, 0, ClauseState::Translated, 0);
    other.clause = ClauseId::new(seg(IN, 1), 0);
    let queue = [view(5, 0, ClauseState::Translated, 0), other];
    let trigger = InterruptTrigger::SameSpeakerNewSegment {
        new_segment: seg(OUT, 5),
    };
    assert!(decide(trigger, &params(), 60_000, &queue).is_empty());
}

#[test]
fn barge_in_off_and_the_other_party_cancel_nothing() {
    let queue = [view(1, 0, ClauseState::Translated, 0)];
    let off = InterruptParams {
        barge_in: false,
        ..params()
    };
    let same = InterruptTrigger::SameSpeakerNewSegment {
        new_segment: seg(OUT, 2),
    };
    assert!(decide(same, &off, 60_000, &queue).is_empty());
    for speaker in [OUT, IN] {
        let other = InterruptTrigger::OtherPartySpeaks { speaker };
        assert!(decide(other, &params(), 60_000, &queue).is_empty());
    }
}

#[test]
fn a_configured_limit_replaces_the_default() {
    let queue = [view(1, 0, ClauseState::Translated, 0)];
    let strict = InterruptParams {
        max_lag: Duration::from_millis(500),
        ..params()
    };
    let trigger = InterruptTrigger::SameSpeakerNewSegment {
        new_segment: seg(OUT, 2),
    };
    assert_eq!(decide(trigger, &strict, 600, &queue).len(), 1);
    assert!(decide(trigger, &params(), 600, &queue).is_empty());
}

#[test]
fn skip_pause_suspend_and_stop_cancel_whole_directions() {
    let none: [ClauseView; 0] = [];
    let p = params();
    assert_eq!(
        decide(InterruptTrigger::SkipRequested(IN), &p, 0, &none),
        vec![CancelAction::CancelDirection(IN)]
    );
    assert_eq!(
        decide(InterruptTrigger::Paused(DirSet::one(OUT)), &p, 0, &none),
        vec![CancelAction::CancelDirection(OUT)]
    );
    assert_eq!(
        decide(InterruptTrigger::Suspended(DirSet::BOTH), &p, 0, &none),
        vec![
            CancelAction::CancelDirection(OUT),
            CancelAction::CancelDirection(IN)
        ]
    );
    assert!(decide(InterruptTrigger::Paused(DirSet::NONE), &p, 0, &none).is_empty());
    assert_eq!(
        decide(InterruptTrigger::Stop, &p, 0, &none),
        vec![
            CancelAction::CancelDirection(OUT),
            CancelAction::CancelDirection(IN)
        ]
    );
}

#[test]
fn skipping_one_direction_never_names_the_other() {
    let acts = decide(InterruptTrigger::SkipRequested(OUT), &params(), 0, &[]);
    assert!(
        acts.iter()
            .all(|a| *a == CancelAction::CancelDirection(OUT))
    );
}

#[test]
fn a_clause_that_is_completely_synthesised_is_never_dropped_as_stale() {
    let queue = [
        view(1, 1, ClauseState::Synthesizing, 2_000),
        view(1, 2, ClauseState::Synthesizing, 2_500),
    ];
    let trigger = InterruptTrigger::SameSpeakerNewSegment {
        new_segment: seg(OUT, 3),
    };
    let complete = [ClauseId::new(seg(OUT, 1), 1)];
    let got = decide_interrupt_keeping(
        trigger,
        &params(),
        MonoTime::from_millis(9_500),
        &queue,
        &complete,
    );
    assert_eq!(
        got,
        vec![CancelAction::CancelClause(ClauseId::new(seg(OUT, 1), 2))],
        "only the clause whose audio is not complete is stale"
    );
    // Without the knowledge both are dropped, as before.
    assert_eq!(decide(trigger, &params(), 9_500, &queue).len(), 2);
}
