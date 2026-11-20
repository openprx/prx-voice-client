//! Tests of the segment book: identities, ordering of playback release, late results, cancellation and rejections.

use super::*;
use crate::fault::Stage;
use ClauseEventKind as K;

const OUT: Direction = Direction::Outgoing;
const IN: Direction = Direction::Incoming;

fn t(ms: u64) -> MonoTime {
    MonoTime::from_millis(ms)
}

fn seg(seq: u64) -> SegmentId {
    SegmentId::new(OUT, seq).expect("seq >= 1")
}

fn clause(seq: u64, index: u16) -> ClauseId {
    ClauseId::new(seg(seq), index)
}

fn ev(b: &mut SegmentBook, e: SegmentEvent) -> Vec<SegmentAction> {
    b.on(e).unwrap_or_else(|r| panic!("{e:?} rejected: {r}"))
}

fn progress(b: &mut SegmentBook, c: ClauseId, kinds: &[ClauseEventKind]) -> Vec<SegmentAction> {
    kinds
        .iter()
        .flat_map(|k| {
            ev(
                b,
                SegmentEvent::Clause {
                    clause: c,
                    kind: *k,
                },
            )
        })
        .collect()
}

/// Open a segment and commit `n` clauses ending at `end_ms`.
fn segment_with(b: &mut SegmentBook, n: u16, end_ms: u64) -> SegmentId {
    let opened = ev(
        b,
        SegmentEvent::SpeechStarted {
            at: t(end_ms.saturating_sub(500)),
        },
    );
    let SegmentAction::Opened(id) = opened[0] else {
        panic!("no Opened: {opened:?}")
    };
    for _ in 0..n {
        ev(
            b,
            SegmentEvent::CommitClause {
                segment: id,
                revision: 1,
                source_end: t(end_ms),
            },
        );
    }
    ev(b, SegmentEvent::EndSegment(id));
    id
}

const TO_SYNTH: [ClauseEventKind; 4] = [
    K::Recognized,
    K::TranslationStarted,
    K::Translated,
    K::SynthesisStarted,
];

fn releases(actions: &[SegmentAction]) -> Vec<ClauseId> {
    actions
        .iter()
        .filter_map(|a| {
            if let SegmentAction::ReleaseForPlayback(c) = a {
                Some(*c)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn segments_get_increasing_ids_from_one_and_clauses_get_increasing_indexes() {
    let mut b = SegmentBook::new(OUT);
    let a = ev(&mut b, SegmentEvent::SpeechStarted { at: t(10) });
    assert_eq!(a, vec![SegmentAction::Opened(seg(1))]);
    assert_eq!(b.segment_onset(seg(1)), Some(t(10)));
    let c0 = ev(
        &mut b,
        SegmentEvent::CommitClause {
            segment: seg(1),
            revision: 4,
            source_end: t(900),
        },
    );
    let c1 = ev(
        &mut b,
        SegmentEvent::CommitClause {
            segment: seg(1),
            revision: 7,
            source_end: t(1_800),
        },
    );
    assert_eq!(c0, vec![SegmentAction::ClauseCommitted(clause(1, 0))]);
    assert_eq!(c1, vec![SegmentAction::ClauseCommitted(clause(1, 1))]);
    assert_eq!(b.clause_revision(clause(1, 1)), Some(7));
    assert_eq!(
        ev(&mut b, SegmentEvent::EndSegment(seg(1))),
        vec![SegmentAction::SegmentClosed(seg(1))]
    );
    let next = ev(&mut b, SegmentEvent::SpeechStarted { at: t(3_000) });
    assert_eq!(next, vec![SegmentAction::Opened(seg(2))]);
}

#[test]
fn a_new_speech_region_while_the_previous_segment_is_open_is_rejected() {
    let mut b = SegmentBook::new(OUT);
    ev(&mut b, SegmentEvent::SpeechStarted { at: t(0) });
    assert_eq!(
        b.on(SegmentEvent::SpeechStarted { at: t(1) }),
        Err(SegmentRejection::SegmentStillOpen(seg(1)))
    );
}

#[test]
fn a_stale_hypothesis_revision_is_dropped_and_counted_an_equal_one_is_accepted() {
    let mut b = SegmentBook::new(OUT);
    ev(&mut b, SegmentEvent::SpeechStarted { at: t(0) });
    let h = |revision| SegmentEvent::Hypothesis {
        segment: seg(1),
        revision,
    };
    assert_eq!(
        ev(&mut b, h(3)),
        vec![SegmentAction::HypothesisAccepted {
            segment: seg(1),
            revision: 3
        }]
    );
    assert_eq!(
        ev(&mut b, h(2)),
        vec![SegmentAction::DropLate {
            segment: seg(1),
            reason: LateReason::StaleRevision
        }]
    );
    assert_eq!(b.late_dropped(), 1);
    assert_eq!(
        ev(&mut b, h(3)).len(),
        1,
        "a final repeats the revision it confirms"
    );
}

#[test]
fn a_late_asr_revision_after_commit_never_touches_the_committed_clause() {
    let mut b = SegmentBook::new(OUT);
    ev(&mut b, SegmentEvent::SpeechStarted { at: t(0) });
    ev(
        &mut b,
        SegmentEvent::Hypothesis {
            segment: seg(1),
            revision: 5,
        },
    );
    ev(
        &mut b,
        SegmentEvent::CommitClause {
            segment: seg(1),
            revision: 5,
            source_end: t(800),
        },
    );
    let late = ev(
        &mut b,
        SegmentEvent::Hypothesis {
            segment: seg(1),
            revision: 4,
        },
    );
    assert_eq!(
        late,
        vec![SegmentAction::DropLate {
            segment: seg(1),
            reason: LateReason::StaleRevision
        }]
    );
    assert_eq!(b.clause_state(clause(1, 0)), Some(ClauseState::Committed));
    assert_eq!(
        b.clause_revision(clause(1, 0)),
        Some(5),
        "the clause revision is frozen"
    );
}

#[test]
fn clauses_are_released_for_playback_strictly_in_order() {
    let mut b = SegmentBook::new(OUT);
    segment_with(&mut b, 2, 1_000);
    // Clause 1 is ready first: it must wait for clause 0.
    progress(&mut b, clause(1, 1), &TO_SYNTH);
    let a = progress(&mut b, clause(1, 1), &[K::AudioReady]);
    assert!(
        releases(&a).is_empty(),
        "clause 1 must not overtake clause 0: {a:?}"
    );
    progress(&mut b, clause(1, 0), &TO_SYNTH);
    let a = progress(&mut b, clause(1, 0), &[K::AudioReady]);
    assert_eq!(releases(&a), vec![clause(1, 0)]);
    assert_eq!(
        b.on(SegmentEvent::Clause {
            clause: clause(1, 1),
            kind: K::PlaybackStarted
        }),
        Err(SegmentRejection::NotReleased(clause(1, 1)))
    );
    progress(&mut b, clause(1, 0), &[K::PlaybackStarted]);
    assert_eq!(b.activity(), Activity::Speaking);
    let a = progress(&mut b, clause(1, 0), &[K::PlaybackFinished]);
    assert_eq!(
        releases(&a),
        vec![clause(1, 1)],
        "finishing clause 0 releases clause 1"
    );
}

#[test]
fn skipping_the_head_releases_the_next_ready_clause() {
    let mut b = SegmentBook::new(OUT);
    segment_with(&mut b, 2, 1_000);
    progress(&mut b, clause(1, 1), &TO_SYNTH);
    progress(&mut b, clause(1, 1), &[K::AudioReady]);
    let a = ev(
        &mut b,
        SegmentEvent::Cancel(CancelAction::CancelClause(clause(1, 0))),
    );
    assert!(
        a.contains(&SegmentAction::Cancel(CancelAction::CancelClause(clause(
            1, 0
        ))))
    );
    assert_eq!(releases(&a), vec![clause(1, 1)]);
}

#[test]
fn playback_order_holds_across_segments() {
    let mut b = SegmentBook::new(OUT);
    segment_with(&mut b, 1, 1_000);
    segment_with(&mut b, 1, 2_000);
    progress(&mut b, clause(2, 0), &TO_SYNTH);
    let a = progress(&mut b, clause(2, 0), &[K::AudioReady]);
    assert!(
        releases(&a).is_empty(),
        "the first segment's clause is still ahead"
    );
    progress(&mut b, clause(1, 0), &[K::Failed(Stage::Tts)]);
    let a = ev(
        &mut b,
        SegmentEvent::Clause {
            clause: clause(1, 0),
            kind: K::Skip,
        },
    );
    assert!(a.is_empty(), "skipping a terminal clause changes nothing");
    assert_eq!(
        b.clause_state(clause(2, 0)),
        Some(ClauseState::Synthesizing)
    );
}

#[test]
fn a_result_for_a_skipped_clause_is_dropped_as_late() {
    let mut b = SegmentBook::new(OUT);
    segment_with(&mut b, 2, 1_000);
    progress(
        &mut b,
        clause(1, 0),
        &[K::Recognized, K::TranslationStarted],
    );
    ev(
        &mut b,
        SegmentEvent::Cancel(CancelAction::CancelClause(clause(1, 0))),
    );
    let a = ev(
        &mut b,
        SegmentEvent::Clause {
            clause: clause(1, 0),
            kind: K::Translated,
        },
    );
    assert_eq!(
        a,
        vec![SegmentAction::DropLate {
            segment: seg(1),
            reason: LateReason::ClauseFinished
        }]
    );
    assert_eq!(b.late_dropped(), 1);
}

#[test]
fn a_result_for_a_finished_segment_behind_a_live_one_is_late_as_finished() {
    let mut b = SegmentBook::new(OUT);
    segment_with(&mut b, 1, 1_000);
    segment_with(&mut b, 1, 2_000);
    ev(
        &mut b,
        SegmentEvent::Cancel(CancelAction::CancelClause(clause(2, 0))),
    );
    assert_eq!(b.segment_state(seg(2)), Some(SegmentState::Done));
    let a = ev(
        &mut b,
        SegmentEvent::Clause {
            clause: clause(2, 0),
            kind: K::Translated,
        },
    );
    assert_eq!(
        a,
        vec![SegmentAction::DropLate {
            segment: seg(2),
            reason: LateReason::SegmentFinished
        }]
    );
}

#[test]
fn cancelling_a_segment_cancels_its_clauses_and_later_results_are_late() {
    let mut b = SegmentBook::new(OUT);
    let hold = segment_with(&mut b, 1, 1_000);
    segment_with(&mut b, 2, 2_000);
    progress(&mut b, clause(2, 0), &TO_SYNTH);
    let a = ev(
        &mut b,
        SegmentEvent::Cancel(CancelAction::CancelSegment(seg(2))),
    );
    assert_eq!(
        a,
        vec![
            SegmentAction::ClauseAdvanced {
                clause: clause(2, 0),
                state: ClauseState::Cancelled
            },
            SegmentAction::ClauseAdvanced {
                clause: clause(2, 1),
                state: ClauseState::Cancelled
            },
            SegmentAction::Cancel(CancelAction::CancelSegment(seg(2))),
        ]
    );
    let late = ev(
        &mut b,
        SegmentEvent::Clause {
            clause: clause(2, 0),
            kind: K::AudioReady,
        },
    );
    assert!(matches!(
        late[0],
        SegmentAction::DropLate {
            reason: LateReason::SegmentCancelled,
            ..
        }
    ));
    assert_eq!(
        b.segment_state(hold),
        Some(SegmentState::Closed),
        "other segments are untouched"
    );
}

#[test]
fn cancelling_a_playing_clause_cuts_it_and_never_replays_it() {
    let mut b = SegmentBook::new(OUT);
    segment_with(&mut b, 1, 1_000);
    progress(&mut b, clause(1, 0), &TO_SYNTH);
    progress(&mut b, clause(1, 0), &[K::AudioReady]);
    progress(&mut b, clause(1, 0), &[K::PlaybackStarted]);
    assert_eq!(b.playback_state(clause(1, 0)), Some(PlaybackState::Playing));
    assert_eq!(
        b.on(SegmentEvent::Cancel(CancelAction::CancelClause(clause(
            1, 0
        )))),
        Err(SegmentRejection::IllegalClauseEvent {
            clause: clause(1, 0),
            from: ClauseState::Playing,
            kind: K::Skip
        }),
        "a skip never cuts the clause being played (D-P5)"
    );
    ev(
        &mut b,
        SegmentEvent::Cancel(CancelAction::CancelSegment(seg(1))),
    );
    assert_eq!(
        b.clause_state(clause(1, 0)),
        None,
        "a finished segment is forgotten"
    );
    let late = ev(
        &mut b,
        SegmentEvent::Clause {
            clause: clause(1, 0),
            kind: K::PlaybackFinished,
        },
    );
    assert!(matches!(
        late[0],
        SegmentAction::DropLate {
            reason: LateReason::SegmentGone,
            ..
        }
    ));
}

#[test]
fn a_playing_clause_that_is_cancelled_is_marked_cut() {
    let mut b = SegmentBook::new(OUT);
    segment_with(&mut b, 2, 1_000);
    progress(&mut b, clause(1, 0), &TO_SYNTH);
    progress(
        &mut b,
        clause(1, 0),
        &[K::AudioReady, K::PlaybackStarted, K::Cancel],
    );
    assert_eq!(b.clause_state(clause(1, 0)), Some(ClauseState::Cancelled));
    assert_eq!(b.playback_state(clause(1, 0)), Some(PlaybackState::Cut));
    assert_eq!(
        b.playback_state(clause(1, 1)),
        Some(PlaybackState::NotStarted)
    );
}

#[test]
fn cancel_direction_cancels_every_live_segment_and_rejects_the_other_direction() {
    let mut b = SegmentBook::new(OUT);
    segment_with(&mut b, 1, 1_000);
    segment_with(&mut b, 1, 2_000);
    let a = ev(
        &mut b,
        SegmentEvent::Cancel(CancelAction::CancelDirection(OUT)),
    );
    assert_eq!(
        a[0],
        SegmentAction::Cancel(CancelAction::CancelDirection(OUT))
    );
    assert_eq!(
        b.clause_views(),
        vec![],
        "everything is cancelled and forgotten"
    );
    assert_eq!(b.activity(), Activity::Listening);
    assert_eq!(
        b.on(SegmentEvent::Cancel(CancelAction::CancelDirection(IN))),
        Err(SegmentRejection::WrongDirection {
            expected: OUT,
            got: IN
        })
    );
}

#[test]
fn a_segment_is_done_when_every_clause_is_terminal_and_old_ones_are_forgotten() {
    let mut b = SegmentBook::new(OUT);
    segment_with(&mut b, 1, 1_000);
    progress(&mut b, clause(1, 0), &TO_SYNTH);
    progress(&mut b, clause(1, 0), &[K::AudioReady, K::PlaybackStarted]);
    let a = progress(&mut b, clause(1, 0), &[K::PlaybackFinished]);
    assert!(a.contains(&SegmentAction::SegmentDone(seg(1))));
    assert_eq!(b.segment_state(seg(1)), None);
    let late = ev(
        &mut b,
        SegmentEvent::Hypothesis {
            segment: seg(1),
            revision: 9,
        },
    );
    assert_eq!(
        late,
        vec![SegmentAction::DropLate {
            segment: seg(1),
            reason: LateReason::SegmentGone
        }]
    );
}

#[test]
fn an_empty_segment_finishes_at_once() {
    let mut b = SegmentBook::new(OUT);
    ev(&mut b, SegmentEvent::SpeechStarted { at: t(0) });
    let a = ev(&mut b, SegmentEvent::EndSegment(seg(1)));
    assert_eq!(
        a,
        vec![
            SegmentAction::SegmentClosed(seg(1)),
            SegmentAction::SegmentDone(seg(1))
        ]
    );
}

#[test]
fn rejections_leave_the_book_unchanged() {
    let mut b = SegmentBook::new(OUT);
    segment_with(&mut b, 1, 1_000);
    let before = b.clause_views();
    let other_dir = SegmentId::first(IN);
    for bad in [
        SegmentEvent::Hypothesis {
            segment: other_dir,
            revision: 1,
        },
        SegmentEvent::EndSegment(seg(9)),
        SegmentEvent::EndSegment(seg(1)),
        SegmentEvent::CommitClause {
            segment: seg(1),
            revision: 1,
            source_end: t(1),
        },
        SegmentEvent::Clause {
            clause: clause(1, 5),
            kind: K::Recognized,
        },
        SegmentEvent::Clause {
            clause: clause(1, 0),
            kind: K::Translated,
        },
    ] {
        assert!(b.on(bad).is_err(), "{bad:?} must be rejected");
    }
    assert_eq!(b.clause_views(), before);
    assert_eq!(b.late_dropped(), 0, "rejections are not late results");
}

#[test]
fn activity_follows_the_pipeline() {
    let mut b = SegmentBook::new(OUT);
    assert_eq!(b.activity(), Activity::Listening);
    ev(&mut b, SegmentEvent::SpeechStarted { at: t(0) });
    assert_eq!(b.activity(), Activity::Recognizing);
}
