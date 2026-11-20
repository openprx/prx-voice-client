//! Exhaustive tests of the segment and clause tables: every (state, event kind) pair has an explicit expectation.

use super::*;
use ClauseEventKind as K;
use ClauseState as C;
use SegmentEventKind as SK;
use SegmentState as S;

const SEGMENT_KINDS: [SegmentEventKind; 5] = [
    SK::Hypothesis,
    SK::CommitClause,
    SK::Close,
    SK::Finish,
    SK::Cancel,
];

fn segment_expected(state: SegmentState, kind: SegmentEventKind) -> Outcome<SegmentState> {
    use Outcome::{Illegal, Late, Next, Stay};
    match (state, kind) {
        (S::Open, SK::Hypothesis) => Stay,
        (S::Open, SK::CommitClause) => Stay,
        (S::Open, SK::Close) => Next(S::Closed),
        (S::Open, SK::Finish) => Illegal,
        (S::Open, SK::Cancel) => Next(S::Cancelled),
        (S::Closed, SK::Hypothesis) => Stay,
        (S::Closed, SK::CommitClause) => Illegal,
        (S::Closed, SK::Close) => Illegal,
        (S::Closed, SK::Finish) => Next(S::Done),
        (S::Closed, SK::Cancel) => Next(S::Cancelled),
        (S::Done | S::Cancelled, SK::Cancel) => Stay,
        (S::Done | S::Cancelled, _) => Late,
    }
}

fn kind_index(k: SegmentEventKind) -> usize {
    match k {
        SK::Hypothesis => 0,
        SK::CommitClause => 1,
        SK::Close => 2,
        SK::Finish => 3,
        SK::Cancel => 4,
    }
}

#[test]
fn segment_table_covers_every_pair() {
    assert_eq!(
        SEGMENT_KINDS.map(kind_index),
        [0, 1, 2, 3, 4],
        "a kind is missing from the list"
    );
    for state in SegmentState::ALL {
        for kind in SEGMENT_KINDS {
            assert_eq!(
                state.apply(kind),
                segment_expected(state, kind),
                "{state:?} + {kind:?}"
            );
        }
    }
    assert!(S::Done.is_terminal() && S::Cancelled.is_terminal());
    assert!(!S::Open.is_terminal() && !S::Closed.is_terminal());
}

const CLAUSE_KINDS: [ClauseEventKind; 10] = [
    K::Recognized,
    K::TranslationStarted,
    K::Translated,
    K::SynthesisStarted,
    K::AudioReady,
    K::PlaybackStarted,
    K::PlaybackFinished,
    K::Failed(Stage::Mt),
    K::Skip,
    K::Cancel,
];

fn clause_kind_index(k: ClauseEventKind) -> usize {
    match k {
        K::Recognized => 0,
        K::TranslationStarted => 1,
        K::Translated => 2,
        K::SynthesisStarted => 3,
        K::AudioReady => 4,
        K::PlaybackStarted => 5,
        K::PlaybackFinished => 6,
        K::Failed(_) => 7,
        K::Skip => 8,
        K::Cancel => 9,
    }
}

/// The forward step of each non-terminal state; everything else is decided by the rules below.
fn forward(state: ClauseState) -> Option<(ClauseEventKind, ClauseState)> {
    match state {
        C::Committed => Some((K::Recognized, C::Recognized)),
        C::Recognized => Some((K::TranslationStarted, C::Translating)),
        C::Translating => Some((K::Translated, C::Translated)),
        C::Translated => Some((K::SynthesisStarted, C::Synthesizing)),
        C::Synthesizing => Some((K::PlaybackStarted, C::Playing)),
        C::Playing => Some((K::PlaybackFinished, C::Played)),
        _ => None,
    }
}

fn clause_expected(state: ClauseState, kind: ClauseEventKind) -> Outcome<ClauseState> {
    use Outcome::{Illegal, Late, Next, Stay};
    if state.is_terminal() {
        return if matches!(kind, K::Skip | K::Cancel) {
            Stay
        } else {
            Late
        };
    }
    match kind {
        K::Cancel => Next(C::Cancelled),
        K::Skip if state == C::Playing => Illegal,
        K::Skip => Next(C::Skipped),
        K::Failed(stage) => Next(C::Failed(stage)),
        K::AudioReady if state == C::Synthesizing => Stay,
        K::Translated if state == C::Synthesizing => Stay,
        K::SynthesisStarted if state == C::Translating => Next(C::Synthesizing),
        k => match forward(state) {
            Some((expected, to)) if expected == k => Next(to),
            _ => Illegal,
        },
    }
}

#[test]
fn clause_table_covers_every_pair() {
    assert_eq!(
        CLAUSE_KINDS.map(clause_kind_index),
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
    let mut ordinals: Vec<usize> = ClauseState::ALL.iter().map(|s| s.ordinal()).collect();
    ordinals.sort_unstable();
    assert_eq!(
        ordinals,
        (0..10).collect::<Vec<_>>(),
        "ALL must hold one state per ordinal"
    );
    for state in ClauseState::ALL {
        for kind in CLAUSE_KINDS {
            assert_eq!(
                state.apply(kind),
                clause_expected(state, kind),
                "{state:?} + {kind:?}"
            );
        }
    }
}

#[test]
fn a_playing_clause_is_never_skipped_only_cancelled() {
    assert_eq!(C::Playing.apply(K::Skip), Outcome::Illegal);
    assert_eq!(C::Playing.apply(K::Cancel), Outcome::Next(C::Cancelled));
}

#[test]
fn the_happy_path_visits_every_state_in_order() {
    let mut s = C::Committed;
    let mut seen = vec![s];
    while let Some((kind, _)) = forward(s) {
        if let Outcome::Next(n) = s.apply(kind) {
            s = n;
            seen.push(s);
        }
    }
    assert_eq!(
        seen,
        vec![
            C::Committed,
            C::Recognized,
            C::Translating,
            C::Translated,
            C::Synthesizing,
            C::Playing,
            C::Played
        ]
    );
}

#[test]
fn a_failure_is_terminal_and_names_its_stage() {
    assert_eq!(
        C::Translating.apply(K::Failed(Stage::Mt)),
        Outcome::Next(C::Failed(Stage::Mt))
    );
    assert!(C::Failed(Stage::Mt).is_terminal());
}

#[test]
fn early_synthesis_may_start_before_the_translation_completes() {
    let started = C::Translating.apply(K::SynthesisStarted);
    assert_eq!(started, Outcome::Next(C::Synthesizing));
    assert_eq!(
        C::Synthesizing.apply(K::Translated),
        Outcome::Stay,
        "the translation completing later changes nothing"
    );
    assert_eq!(
        C::Recognized.apply(K::SynthesisStarted),
        Outcome::Illegal,
        "synthesis never starts before translation does"
    );
}
