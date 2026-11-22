//! Ageing of the units of one long sentence: an early unit is aged from the end of its segment's newest clause, so that the barge-in and backlog rules do not drop the middle of a sentence that is still being said.

use super::*;

/// Commit a clause of segment `s` that ends at `end_ms`; `continues` says whether it ends its sentence.
fn commit_unit(h: &mut Harness, s: u64, text: &str, start_ms: u64, end_ms: u64, continues: bool) {
    let c = Commit {
        text: text.to_owned(),
        source_start: MonoTime::from_millis(start_ms),
        source_end: MonoTime::from_millis(end_ms),
        audio: None::<Arc<ClauseSamples>>,
        reason: CommitReason::Punctuation,
        continues,
    };
    let e = Envelope::new(
        seg(s),
        1,
        MonoTime::from_millis(100),
        RecognitionEvent::Commit(c),
    );
    h.core.handle(Inbound::Recognition(e));
}

/// New speech of the same speaker, beginning at `at_ms`.
fn new_speech(h: &mut Harness, s: u64, at_ms: u64) {
    h.w.tree.open_segment(seg(s));
    let e = Envelope::new(
        seg(s),
        0,
        MonoTime::from_millis(at_ms),
        RecognitionEvent::SpeechStarted,
    );
    h.core.handle(Inbound::Recognition(e));
}

/// A long sentence cut early: unit 0 ended at 1 s, the last unit of the segment at 5 s.
fn long_sentence(h: &mut Harness, early_continues: bool) {
    h.speech(1);
    commit_unit(h, 1, "我们明天下午三点", 100, 1_000, early_continues);
    commit_unit(h, 1, "在会议室等你过来", 1_000, 5_000, false);
}

#[test]
fn an_early_unit_of_a_long_sentence_is_not_dropped_as_stale_while_the_end_of_its_sentence_is_young()
{
    let mut h = harness(|_| {});
    long_sentence(&mut h, true);
    // 4.5 s: unit 0 alone would have lagged 3.5 s, over the 3 s limit; its sentence ended 0.5 s ago.
    new_speech(&mut h, 2, 4_500);
    let snap = h.snapshot();
    assert_eq!(snap.counters.stale_skips, 0);
    assert_eq!(snap.clauses[0].state, ClauseState::Recognized);
    assert_eq!(snap.clauses[1].state, ClauseState::Recognized);
}

#[test]
fn the_same_units_without_continuation_are_aged_by_their_own_speech_as_before() {
    let mut h = harness(|_| {});
    long_sentence(&mut h, false);
    new_speech(&mut h, 2, 4_500);
    let snap = h.snapshot();
    assert_eq!(snap.clauses[0].state, ClauseState::Skipped);
    assert_eq!(snap.clauses[0].skipped, Some(SkipReason::Stale));
    assert_eq!(
        snap.clauses[1].state,
        ClauseState::Recognized,
        "its own lag is under the limit"
    );
}

#[test]
fn a_unit_that_continues_is_dropped_when_the_end_of_its_sentence_is_old_too() {
    let mut h = harness(|_| {});
    long_sentence(&mut h, true);
    new_speech(&mut h, 2, 9_000);
    let snap = h.snapshot();
    assert_eq!(snap.clauses[0].skipped, Some(SkipReason::Stale));
    assert_eq!(snap.clauses[1].skipped, Some(SkipReason::Stale));
}

#[test]
fn the_backlog_rule_ages_an_early_unit_from_the_end_of_its_sentence() {
    let run = |early_continues: bool| {
        let mut h = backlog_harness(true);
        h.speech(1);
        commit_unit(&mut h, 1, LONG, 100, 1_000, early_continues);
        commit_unit(&mut h, 1, LONG, 1_000, 5_000, early_continues);
        commit_unit(&mut h, 1, LONG, 5_000, 9_000, false);
        h.clock.advance(Duration::from_millis(12_000));
        h.core.on_timer(h.clock.now());
        h.snapshot()
    };
    let aged_by_itself = run(false);
    assert_eq!(aged_by_itself.clauses[0].skipped, Some(SkipReason::Backlog));
    let with_its_sentence = run(true);
    assert_eq!(
        with_its_sentence.counters.backlog_skips, 0,
        "the sentence ended 3 s ago, under the 10 s limit"
    );
    assert_eq!(with_its_sentence.clauses[0].state, ClauseState::Recognized);
}
