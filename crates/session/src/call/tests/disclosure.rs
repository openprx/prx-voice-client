//! The spoken notice to the other party: played once before the first translated sentence of the outgoing direction, never when it is off, again on request, through a pause, in the language of the other party, and outside the pipeline's rules.

use prxvc_media::{ClauseId, Direction, Lang, MonoTime, SegmentId};
use prxvc_ports::fakes::TtsScript;

use super::kit::{Played, Rig};
use super::translation::{clause, script_one_utterance, speak, translating_both};
use crate::call::{
    CallCommand, DisclosureSnapshot, DisclosureSpec, DisclosureState, PlaybackReport,
};
use crate::dir_set::DirSet;
use crate::output::OutputSource;

const NOTICE_TEXT: &str = "This call is being translated in real time.";

fn notice(n: u16) -> ClauseId {
    ClauseId::new(
        SegmentId::new(Direction::Outgoing, u64::MAX).expect("seq >= 1"),
        n,
    )
}

/// A rig whose outgoing direction carries the notice, with the notice's own synthesis scripted.
fn rig(enabled: bool) -> Rig {
    let rig = Rig::new();
    script_one_utterance(&rig);
    rig.world.with_translation(|t| {
        t.disclosure = Some(DisclosureSpec {
            enabled,
            text: NOTICE_TEXT.into(),
        });
        t.tts = std::mem::replace(
            &mut t.tts,
            prxvc_ports::fakes::FakeSpeechSynth::new(rig.clock.clone()),
        )
        .with_script(NOTICE_TEXT, TtsScript::chunks(2, 2400));
    });
    rig
}

fn notice_state(rig: &Rig) -> DisclosureSnapshot {
    rig.last().disclosure
}

/// Wait until the notice has been prepared.
fn prepared(rig: &mut Rig) {
    rig.until("the notice is prepared", |s| {
        matches!(
            s.disclosure.state,
            DisclosureState::Ready | DisclosureState::Failed
        )
    });
}

fn position(played: &[Played], pred: impl Fn(&Played) -> bool) -> Option<usize> {
    played.iter().position(pred)
}

#[test]
fn the_notice_is_played_once_before_the_first_outgoing_clause_and_not_before_the_next() {
    let mut rig = rig(true);
    rig.start_translating();
    translating_both(&mut rig);
    prepared(&mut rig);
    assert_eq!(notice_state(&rig).state, DisclosureState::Ready);
    let first = clause(Direction::Outgoing, 1, 0);
    speak(&rig, "outgoing", 55);
    rig.until_world("the first clause reached the lane", |w| {
        w.played("outgoing").contains(&Played::End(first))
    });
    let played = rig.world.played("outgoing");
    let notice_end = position(&played, |p| *p == Played::End(notice(0))).expect("the notice ended");
    let first_chunk = position(&played, |p| matches!(p, Played::Chunk(c, _) if *c == first))
        .expect("the clause played");
    assert!(
        notice_end < first_chunk,
        "the notice goes in before the translation: {played:?}"
    );
    assert_eq!(
        played
            .iter()
            .filter(|p| matches!(p, Played::End(c) if *c == notice(0)))
            .count(),
        1
    );
    // The mixer plays it to the end.
    rig.world.report_playback(
        "outgoing",
        PlaybackReport::Started {
            clause: notice(0),
            at: MonoTime::from_millis(10),
        },
    );
    rig.world.report_playback(
        "outgoing",
        PlaybackReport::Finished {
            clause: notice(0),
            at: MonoTime::from_millis(3_000),
        },
    );
    let s = rig.until("the notice counts as played", |s| s.disclosure.plays == 1);
    assert_eq!(s.disclosure.state, DisclosureState::Played);
    assert!(s.disclosure.last_played_s.is_some());
    // The pipeline never saw it: no rejected event, no clause for it.
    let pipeline = s.direction(Direction::Outgoing).pipeline.as_ref().unwrap();
    assert_eq!(pipeline.counters.rejected_events, 0);
    assert_eq!(pipeline.counters.clauses_committed, 1);
}

#[test]
fn nothing_is_played_when_the_notice_is_off_and_the_snapshot_says_so() {
    let mut rig = rig(false);
    rig.start_translating();
    translating_both(&mut rig);
    let first = clause(Direction::Outgoing, 1, 0);
    speak(&rig, "outgoing", 55);
    rig.until_world("the first clause reached the lane", |w| {
        w.played("outgoing").contains(&Played::End(first))
    });
    assert!(
        rig.world
            .played("outgoing")
            .iter()
            .all(|p| !matches!(p, Played::Chunk(c, _) | Played::End(c) if *c == notice(0)))
    );
    assert_eq!(notice_state(&rig), DisclosureSnapshot::default());
    // Saying it again is refused, visibly.
    rig.coord.handle(CallCommand::SayDisclosure);
    assert!(
        rig.last()
            .issues
            .iter()
            .any(|i| i.code == crate::call::CallErrorCode::CommandRejected)
    );
}

#[test]
fn say_it_again_queues_the_notice_on_the_outgoing_lane() {
    let mut rig = rig(true);
    rig.start_translating();
    translating_both(&mut rig);
    prepared(&mut rig);
    rig.coord.handle(CallCommand::SayDisclosure);
    let played = rig.world.played("outgoing");
    assert!(played.contains(&Played::End(notice(0))), "{played:?}");
    rig.coord.handle(CallCommand::SayDisclosure);
    assert!(rig.world.played("outgoing").contains(&Played::End(notice(1))));
    assert!(
        rig.world.played("incoming").iter().all(|p| !matches!(
            p,
            Played::Chunk(c, _) | Played::End(c) if c.segment().seq() == u64::MAX
        )),
        "only the outgoing lane carries it"
    );
}

#[test]
fn a_pause_before_the_first_sentence_changes_nothing_and_a_pause_during_the_notice_repeats_it() {
    let mut rig = rig(true);
    rig.start_translating();
    translating_both(&mut rig);
    prepared(&mut rig);
    rig.coord.handle(CallCommand::Pause(DirSet::BOTH));
    rig.until("paused", |s| {
        s.direction(Direction::Outgoing).hears == OutputSource::Original
    });
    rig.coord.handle(CallCommand::Resume);
    rig.until("resumed", |s| {
        s.direction(Direction::Outgoing).hears == OutputSource::Translation
    });
    assert_eq!(notice_state(&rig).plays, 0, "nothing was played while paused");
    let first = clause(Direction::Outgoing, 1, 0);
    speak(&rig, "outgoing", 55);
    rig.until_world("the first clause reached the lane", |w| {
        w.played("outgoing").contains(&Played::End(first))
    });
    assert!(rig.world.played("outgoing").contains(&Played::End(notice(0))));
    // It starts audible, then the direction is paused before it ends: it was not heard.
    rig.world.report_playback(
        "outgoing",
        PlaybackReport::Started {
            clause: notice(0),
            at: MonoTime::from_millis(5),
        },
    );
    rig.coord.handle(CallCommand::Pause(DirSet::BOTH));
    rig.until("paused again", |s| {
        s.direction(Direction::Outgoing).hears == OutputSource::Original
    });
    rig.world.report_playback(
        "outgoing",
        PlaybackReport::Finished {
            clause: notice(0),
            at: MonoTime::from_millis(2_000),
        },
    );
    let s = rig.until("the cut notice is noted", |s| s.disclosure.interrupted == 1);
    assert_eq!(s.disclosure.plays, 0, "a notice nobody heard is not a played notice");
}

#[test]
fn the_notice_is_in_the_language_of_the_other_party_and_never_reaches_translation() {
    for target in [Lang::En, Lang::Zh] {
        let mut rig = rig(true);
        rig.world.with_translation(|t| {
            t.params = Box::new(move |p| {
                if p.direction == Direction::Outgoing {
                    p.target = target;
                }
            });
        });
        rig.start_translating();
        translating_both(&mut rig);
        prepared(&mut rig);
        let (requests, mt_requests) = {
            let mut tts = None;
            let mut mt = None;
            rig.world.with_translation(|t| {
                tts = Some(t.tts.requests());
                mt = Some(t.mt.requests());
            });
            (tts.unwrap(), mt.unwrap())
        };
        let spoken = requests
            .iter()
            .find(|r| r.text == NOTICE_TEXT)
            .expect("the notice was synthesised");
        assert_eq!(spoken.lang, target, "the notice follows the other party's language");
        assert!(
            mt_requests.iter().all(|r| r.text != NOTICE_TEXT),
            "the notice is never translated"
        );
    }
}

#[test]
fn a_notice_that_cannot_be_prepared_is_a_visible_issue_and_the_translation_goes_on() {
    let mut rig = Rig::new();
    script_one_utterance(&rig);
    rig.world.with_translation(|t| {
        t.disclosure = Some(DisclosureSpec {
            enabled: true,
            text: "No script for this text".into(),
        });
    });
    rig.start_translating();
    translating_both(&mut rig);
    let s = rig.until("the failure is shown", |s| {
        s.issues
            .iter()
            .any(|i| i.code == crate::call::CallErrorCode::DisclosureUnavailable)
    });
    assert_eq!(s.disclosure.state, DisclosureState::Failed);
    let first = clause(Direction::Outgoing, 1, 0);
    speak(&rig, "outgoing", 55);
    rig.until_world("the translation still plays", |w| {
        w.played("outgoing").contains(&Played::End(first))
    });
}
