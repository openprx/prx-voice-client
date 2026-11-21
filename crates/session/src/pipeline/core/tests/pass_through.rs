//! A final text that is already in the listener's language: when the original voice passes through, and that it never outlives its utterance.

use super::*;

const IN_TARGET: &str = "See you at the airport tomorrow";

fn two_pass() -> Harness {
    harness(|p| p.recognition_mode = crate::pipeline::params::RecognitionMode::TwoPass)
}

fn end_segment(h: &mut Harness, s: u64) {
    let e = Envelope::new(
        seg(s),
        1,
        MonoTime::from_millis(100),
        RecognitionEvent::EndSegment,
    );
    h.core.handle(Inbound::Recognition(e));
}

fn hypothesis(h: &mut Harness, s: u64, text: &str) {
    let e = Envelope::new(
        seg(s),
        1,
        MonoTime::from_millis(100),
        RecognitionEvent::Hypothesis {
            text: text.to_owned(),
            stable_prefix_len: 0,
        },
    );
    h.core.handle(Inbound::Recognition(e));
}

/// Commit a clause of segment 1 that waits for its refine pass.
fn commit_for_refine(h: &mut Harness, text: &str) {
    let c = Commit {
        text: text.to_owned(),
        source_start: MonoTime::from_millis(100),
        source_end: MonoTime::from_millis(600),
        audio: Some(Arc::new(ClauseSamples {
            samples: vec![0.0; 160],
            rate: SampleRate::HZ_16000,
        })),
    };
    let e = Envelope::new(seg(1), 1, MonoTime::from_millis(100), RecognitionEvent::Commit(c));
    h.core.handle(Inbound::Recognition(e));
}

fn passthroughs(events: &[PipelineEvent]) -> Vec<bool> {
    events
        .iter()
        .filter_map(|e| match e {
            PipelineEvent::Passthrough { on } => Some(*on),
            _ => None,
        })
        .collect()
}

#[test]
fn a_final_in_the_target_language_while_the_speaker_goes_on_passes_the_original_through() {
    let mut h = two_pass();
    h.speech(1);
    refine_with(&mut h, 0, IN_TARGET, Ok(IN_TARGET.to_owned()));
    assert_eq!(passthroughs(&h.drain()), [true]);
    assert_eq!(h.snapshot().counters.in_target_passed, 1);
    assert!(h.w.mt_in.try_pop().is_none(), "nothing is translated");
    end_segment(&mut h, 1);
    assert_eq!(passthroughs(&h.drain()), [false]);
}

#[test]
fn a_late_final_after_the_segment_ended_is_translated_and_never_switches_to_the_original() {
    let mut h = two_pass();
    h.speech(1);
    commit_for_refine(&mut h, "ok");
    end_segment(&mut h, 1);
    // The refine pass answers after the pause that ended the segment.
    let id = cl(1, 0);
    h.stage(id, StageEvent::Refined { clause: id, result: Ok(IN_TARGET.to_owned()) });
    assert!(passthroughs(&h.drain()).is_empty(), "nothing would switch it back");
    assert_eq!(h.snapshot().counters.in_target_passed, 0);
    assert!(h.w.mt_in.try_pop().is_some(), "the content is translated, not lost");
}

#[test]
fn a_late_final_of_a_segment_that_was_passed_through_is_neither_repeated_nor_stuck() {
    let mut h = two_pass();
    h.speech(1);
    hypothesis(&mut h, 1, IN_TARGET);
    assert_eq!(passthroughs(&h.drain()), [true], "the draft starts the pass-through");
    commit_for_refine(&mut h, "ok");
    end_segment(&mut h, 1);
    assert_eq!(passthroughs(&h.drain()), [false]);
    let id = cl(1, 0);
    // Refined text arrives late, still in the target language: skipped, no second pass-through.
    h.stage(id, StageEvent::Refined { clause: id, result: Ok(IN_TARGET.to_owned()) });
    assert!(passthroughs(&h.drain()).is_empty());
    assert!(h.w.mt_in.try_pop().is_none(), "the listener heard it live; no duplicate");
    assert_eq!(h.snapshot().counters.in_target_passed, 1);
}

#[test]
fn a_next_segment_that_needs_translation_does_not_inherit_the_pass_through() {
    let mut h = two_pass();
    h.speech(1);
    refine_with(&mut h, 0, IN_TARGET, Ok(IN_TARGET.to_owned()));
    end_segment(&mut h, 1);
    h.drain();
    h.speech(2);
    let c = Commit {
        text: "我们明天在机场见面".to_owned(),
        source_start: MonoTime::from_millis(100),
        source_end: MonoTime::from_millis(600),
        audio: None::<Arc<ClauseSamples>>,
    };
    let e = Envelope::new(seg(2), 1, MonoTime::from_millis(100), RecognitionEvent::Commit(c));
    h.core.handle(Inbound::Recognition(e));
    assert!(!passthroughs(&h.drain()).contains(&true));
    assert!(h.w.mt_in.try_pop().is_some());
}

#[test]
fn a_short_loanword_or_acknowledgement_is_translated_not_passed_through() {
    for text in ["OK", "yes yes", "bye", "Zoom会议", "好的", "123"] {
        let mut h = two_pass();
        h.speech(1);
        refine_with(&mut h, 0, text, Ok(text.to_owned()));
        assert!(passthroughs(&h.drain()).is_empty(), "{text}");
        assert!(h.w.mt_in.try_pop().is_some(), "{text} is translated");
        assert_eq!(h.snapshot().counters.in_target_passed, 0, "{text}");
    }
}
