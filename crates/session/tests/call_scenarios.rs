//! Readable end-to-end scripts of the call-translation machines, driven through a minimal coordinator stand-in.

mod support;

use prxvc_session::{
    Activity, CallAction, CallEvent, CallReason, CallState, CancelAction, ClauseEventKind as K,
    ClauseState, DegradeReason, DirSet, DirectionEvent, DirectionState, FaultKind, LateReason,
    OutputCause, OutputPolicy, OutputSource, Passthrough, PlaybackState, SegmentAction,
    SegmentEvent, Stage,
};
use support::{IN, OUT, Rig, released_in};

const O: OutputSource = OutputSource::Original;
const T: OutputSource = OutputSource::Translation;
const N: OutputSource = OutputSource::Nothing;

fn silence_policy() -> OutputPolicy {
    OutputPolicy {
        when_paused: Passthrough::Silence,
        on_failure: Passthrough::Silence,
    }
}

#[test]
fn a_normal_outgoing_sentence_with_two_clauses_plays_in_order() {
    let mut rig = Rig::with_defaults();
    rig.start_translating();
    assert_eq!(
        rig.heard(OUT),
        T,
        "the listener hears the translation once the direction runs"
    );
    assert_eq!(
        rig.announcements(OUT),
        vec![(O, T, OutputCause::TranslationStarted)]
    );

    rig.advance_ms(100);
    let seg = rig.speech_start(OUT);
    assert_eq!(rig.book(OUT).activity(), Activity::Recognizing);
    for rev in 1..=3 {
        rig.seg_event(
            OUT,
            SegmentEvent::Hypothesis {
                segment: seg,
                revision: rev,
            },
        )
        .expect("partial");
    }
    rig.advance_ms(900);
    let c0 = rig.commit(OUT, seg, 3);
    rig.advance_ms(1_100);
    let c1 = rig.commit(OUT, seg, 6);
    rig.end_segment(OUT, seg);

    // Clause 1 finishes synthesis first; it must wait for clause 0.
    let a = rig.ready_to_play(OUT, c1);
    assert!(released_in(&a).is_empty());
    let a = rig.ready_to_play(OUT, c0);
    assert_eq!(released_in(&a), vec![c0]);

    rig.clause(OUT, c0, &[K::PlaybackStarted]);
    assert_eq!(rig.book(OUT).activity(), Activity::Speaking);
    let a = rig.clause(OUT, c0, &[K::PlaybackFinished]);
    assert_eq!(released_in(&a), vec![c1]);
    rig.clause(OUT, c1, &[K::PlaybackStarted, K::PlaybackFinished]);

    assert_eq!(rig.book(OUT).activity(), Activity::Listening);
    assert_eq!(
        rig.book(OUT).segment_state(seg),
        None,
        "a finished segment is forgotten"
    );
    assert_eq!(rig.released[0], vec![c0, c1]);
    assert_eq!(rig.heard(OUT), T);
    assert_eq!(
        rig.announcements(OUT).len(),
        1,
        "no further change of what is heard"
    );
}

#[test]
fn same_speaker_new_sentence_keeps_the_playing_clause_and_drops_only_stale_queued_ones() {
    let mut rig = Rig::with_defaults();
    rig.start_translating();
    // Sentence 1: three clauses whose source ended at 1 s, 2 s and 8 s.
    rig.advance_ms(1_000);
    let s1 = rig.speech_start(OUT);
    let c0 = rig.commit(OUT, s1, 1);
    rig.advance_ms(1_000);
    let c1 = rig.commit(OUT, s1, 2);
    rig.advance_ms(6_000);
    let c2 = rig.commit(OUT, s1, 3);
    rig.end_segment(OUT, s1);
    rig.ready_to_play(OUT, c0);
    rig.ready_to_play(OUT, c1);
    rig.ready_to_play(OUT, c2);
    rig.clause(OUT, c0, &[K::PlaybackStarted]);

    // At 9 s the same speaker starts sentence 2: c1 lags 7 s (stale), c2 lags 1 s (kept), c0 is playing.
    rig.advance_ms(1_000);
    let s2 = rig.speech_start(OUT);

    assert_eq!(
        rig.state_of(OUT, c0),
        Some(ClauseState::Playing),
        "the clause being played is never cut"
    );
    assert_eq!(
        rig.book(OUT).playback_state(c0),
        Some(PlaybackState::Playing)
    );
    assert_eq!(
        rig.state_of(OUT, c1),
        Some(ClauseState::Skipped),
        "stale queued clause is skipped"
    );
    assert_eq!(
        rig.state_of(OUT, c2),
        Some(ClauseState::Synthesizing),
        "a younger clause is kept"
    );
    assert!(rig.log.iter().any(|l| matches!(
        l,
        support::Logged::Seg(_, SegmentAction::Cancel(CancelAction::CancelClause(c))) if *c == c1
    )));
    assert!(!rig.log.iter().any(|l| matches!(
        l,
        support::Logged::Seg(
            _,
            SegmentAction::Cancel(
                CancelAction::CancelSegment(_) | CancelAction::CancelDirection(_)
            )
        )
    )));
    assert_eq!(rig.heard(OUT), T);

    // The kept clause plays after c0, and the new sentence queues behind it.
    let a = rig.clause(OUT, c0, &[K::PlaybackFinished]);
    assert_eq!(released_in(&a), vec![c2]);
    let n0 = rig.commit(OUT, s2, 1);
    rig.advance_ms(500);
    rig.end_segment(OUT, s2);
    let a = rig.ready_to_play(OUT, n0);
    assert!(released_in(&a).is_empty(), "the new clause waits for c2");
    rig.clause(OUT, c2, &[K::PlaybackStarted]);
    let a = rig.clause(OUT, c2, &[K::PlaybackFinished]);
    assert_eq!(released_in(&a), vec![n0]);
}

#[test]
fn barge_in_off_never_drops_anything_and_a_late_clause_is_still_played() {
    let mut rig = Rig::with_defaults();
    rig.interrupt.barge_in = false;
    rig.start_translating();
    let s1 = rig.speech_start(OUT);
    let c0 = rig.commit(OUT, s1, 1);
    rig.end_segment(OUT, s1);
    rig.ready_to_play(OUT, c0);
    rig.advance_ms(30_000);
    rig.speech_start(OUT);
    assert_eq!(rig.state_of(OUT, c0), Some(ClauseState::Synthesizing));
}

#[test]
fn the_other_party_speaking_at_the_same_time_leaves_directions_independent() {
    let mut rig = Rig::with_defaults();
    rig.start_translating();
    let out_seg = rig.speech_start(OUT);
    let in_seg = rig.speech_start(IN);
    let out_c = rig.commit(OUT, out_seg, 1);
    let in_c = rig.commit(IN, in_seg, 1);
    rig.end_segment(OUT, out_seg);
    rig.end_segment(IN, in_seg);

    // The incoming clause is ready first; it is not held back by the outgoing one.
    let a = rig.ready_to_play(IN, in_c);
    assert_eq!(released_in(&a), vec![in_c]);
    let a = rig.ready_to_play(OUT, out_c);
    assert_eq!(released_in(&a), vec![out_c]);
    assert_eq!(rig.released[0], vec![out_c]);
    assert_eq!(rig.released[1], vec![in_c]);

    // Skipping the incoming lane leaves the outgoing lane alone.
    rig.call_event(CallEvent::PauseRequested(DirSet::one(IN)))
        .expect("pause incoming");
    assert_eq!(rig.state_of(OUT, out_c), Some(ClauseState::Synthesizing));
    assert_eq!(
        rig.state_of(IN, in_c),
        None,
        "the incoming queue was purged"
    );
    assert_eq!(rig.heard(OUT), T);
    assert_eq!(rig.heard(IN), O);
}

#[test]
fn pause_and_resume_announce_every_change_and_purge_only_the_paused_direction() {
    let mut rig = Rig::with_defaults();
    rig.start_translating();
    let seg = rig.speech_start(OUT);
    let c = rig.commit(OUT, seg, 1);
    rig.ready_to_play(OUT, c);

    rig.call_event(CallEvent::PauseRequested(DirSet::one(OUT)))
        .expect("pause");
    assert!(matches!(rig.call, CallState::Paused { .. }));
    assert_eq!(
        rig.dir_state(OUT),
        &DirectionState::Paused {
            reasons: Default::default()
        }
    );
    assert_eq!(
        rig.heard(OUT),
        O,
        "paused: the original is heard (default policy)"
    );
    assert_eq!(rig.heard(IN), T, "the other direction keeps translating");
    assert_eq!(
        rig.state_of(OUT, c),
        None,
        "the paused direction's queue is purged"
    );
    assert!(
        rig.log
            .contains(&support::Logged::Call(CallAction::ApplyPausePolicy(
                DirSet::one(OUT)
            )))
    );

    rig.call_event(CallEvent::ResumeRequested).expect("resume");
    assert_eq!(rig.call, CallState::Translating);
    assert_eq!(rig.heard(OUT), T);
    assert_eq!(
        rig.announcements(OUT),
        vec![
            (O, T, OutputCause::TranslationStarted),
            (T, O, OutputCause::Paused),
            (O, T, OutputCause::TranslationResumed),
        ]
    );
    assert_eq!(
        rig.announcements(IN).len(),
        1,
        "the incoming listener never noticed"
    );
}

#[test]
fn a_paused_direction_with_the_silence_policy_is_announced_as_nothing() {
    let mut rig = Rig::new(silence_policy(), OutputPolicy::default());
    rig.start_translating();
    rig.call_event(CallEvent::PauseRequested(DirSet::BOTH))
        .expect("pause");
    assert_eq!((rig.heard(OUT), rig.heard(IN)), (N, O));
    assert_eq!(
        rig.announcements(OUT).last(),
        Some(&(T, N, OutputCause::Paused))
    );
}

#[test]
fn a_provider_failure_in_one_direction_falls_back_to_the_original_with_an_announcement_and_recovers()
 {
    let mut rig = Rig::with_defaults();
    rig.start_translating();
    let seg = rig.speech_start(OUT);
    let c = rig.commit(OUT, seg, 1);
    rig.ready_to_play(OUT, c);

    // The TTS of the outgoing direction dies.
    rig.dir_event(
        OUT,
        DirectionEvent::StageFailed {
            stage: Stage::Tts,
            fault: FaultKind::Crashed,
        },
    )
    .expect("failure");
    rig.call_event(CallEvent::DirectionFailed {
        direction: OUT,
        stage: Stage::Tts,
        fault: FaultKind::Crashed,
    })
    .expect("call learns of it");
    assert_eq!(
        rig.heard(OUT),
        O,
        "the remote party hears the owner's own voice (D-P3)"
    );
    assert_eq!(rig.heard(IN), T, "the incoming direction is unaffected");
    assert_eq!(
        rig.announcements(OUT).last(),
        Some(&(T, O, OutputCause::Failed))
    );
    assert!(
        rig.log
            .contains(&support::Logged::Call(CallAction::ApplyFailurePolicy(
                DirSet::one(OUT)
            )))
    );
    assert!(rig.log.iter().any(|l| matches!(
        l,
        support::Logged::Dir(
            OUT,
            prxvc_session::DirectionAction::ReportFailure {
                stage: Stage::Tts,
                ..
            }
        )
    )));
    assert_eq!(
        rig.state_of(OUT, c),
        None,
        "the failed direction's work was cancelled"
    );
    assert_eq!(
        rig.call,
        CallState::Degraded {
            reasons: [CallReason::DirectionFailed(OUT)].into()
        }
    );

    // The user retries; the fallback keeps playing until the stages are ready.
    rig.dir_event(OUT, DirectionEvent::Retry).expect("retry");
    assert_eq!(rig.heard(OUT), O);
    rig.dir_event(OUT, DirectionEvent::StagesReady)
        .expect("ready");
    assert_eq!(rig.heard(OUT), T);
    rig.call_event(CallEvent::DirectionHealthy(OUT))
        .expect("healthy");
    assert_eq!(rig.call, CallState::Translating);
    assert!(
        rig.log
            .contains(&support::Logged::Call(CallAction::RestoreTranslatedOutput(
                OUT
            )))
    );
    assert_eq!(
        rig.announcements(OUT),
        vec![
            (O, T, OutputCause::TranslationStarted),
            (T, O, OutputCause::Failed),
            (O, T, OutputCause::TranslationStarted),
        ]
    );
}

#[test]
fn a_failure_with_the_silence_policy_is_audibly_defined_and_stays_silent_through_the_retry() {
    let mut rig = Rig::new(silence_policy(), OutputPolicy::default());
    rig.start_translating();
    rig.dir_event(
        OUT,
        DirectionEvent::StageFailed {
            stage: Stage::Mt,
            fault: FaultKind::Timeout,
        },
    )
    .expect("fail");
    assert_eq!(rig.heard(OUT), N);
    rig.dir_event(OUT, DirectionEvent::Retry).expect("retry");
    assert_eq!(
        rig.heard(OUT),
        N,
        "a retry does not leak the original while the stages start"
    );
    rig.dir_event(OUT, DirectionEvent::StagesReady)
        .expect("ready");
    assert_eq!(rig.heard(OUT), T);
}

#[test]
fn a_degradation_keeps_translating_and_is_reported_not_switched() {
    let mut rig = Rig::with_defaults();
    rig.start_translating();
    rig.dir_event(
        OUT,
        DirectionEvent::StageDegraded(DegradeReason::BacklogSkipping),
    )
    .expect("degrade");
    rig.call_event(CallEvent::DirectionDegraded {
        direction: OUT,
        reason: DegradeReason::BacklogSkipping,
    })
    .expect("call");
    assert_eq!(rig.heard(OUT), T, "degraded is not a switch of output");
    assert_eq!(rig.announcements(OUT).len(), 1);
    assert!(matches!(rig.call, CallState::Degraded { .. }));
}

#[test]
fn a_late_asr_revision_after_commit_is_dropped_and_the_clause_plays_unchanged() {
    let mut rig = Rig::with_defaults();
    rig.start_translating();
    let seg = rig.speech_start(OUT);
    rig.seg_event(
        OUT,
        SegmentEvent::Hypothesis {
            segment: seg,
            revision: 5,
        },
    )
    .expect("rev 5");
    let c = rig.commit(OUT, seg, 5);
    let a = rig
        .seg_event(
            OUT,
            SegmentEvent::Hypothesis {
                segment: seg,
                revision: 4,
            },
        )
        .expect("late is not a rejection");
    assert_eq!(
        a,
        vec![SegmentAction::DropLate {
            segment: seg,
            reason: LateReason::StaleRevision
        }]
    );
    assert_eq!(rig.book(OUT).late_dropped(), 1);
    assert_eq!(rig.book(OUT).clause_revision(c), Some(5));
    rig.end_segment(OUT, seg);
    let a = rig.ready_to_play(OUT, c);
    assert_eq!(released_in(&a), vec![c]);
}

#[test]
fn stop_during_synthesis_cancels_everything_and_late_audio_is_dropped() {
    let mut rig = Rig::with_defaults();
    rig.start_translating();
    let s_out = rig.speech_start(OUT);
    let c_out = rig.commit(OUT, s_out, 1);
    rig.clause(
        OUT,
        c_out,
        &[
            K::Recognized,
            K::TranslationStarted,
            K::Translated,
            K::SynthesisStarted,
        ],
    );
    let s_in = rig.speech_start(IN);
    let c_in = rig.commit(IN, s_in, 1);
    rig.clause(IN, c_in, &[K::Recognized, K::TranslationStarted]);

    rig.call_event(CallEvent::StopRequested).expect("stop");
    assert_eq!(rig.call, CallState::Stopping);
    for d in [OUT, IN] {
        assert_eq!(rig.dir_state(d), &DirectionState::Off, "stages joined");
        assert!(
            rig.book(d).clause_views().is_empty(),
            "{d:?}: every segment cancelled"
        );
        assert_eq!(rig.heard(d), O, "routing restore leaves the original");
    }
    assert!(
        rig.log
            .contains(&support::Logged::Call(CallAction::CancelAll))
    );

    // The synthesiser answers after the stop: nothing may come out of it.
    let late = rig
        .seg_event(
            OUT,
            SegmentEvent::Clause {
                clause: c_out,
                kind: K::AudioReady,
            },
        )
        .expect("late, not rejected");
    assert!(matches!(late[..], [SegmentAction::DropLate { .. }]));
    rig.call_event(CallEvent::Stopped).expect("stopped");
    assert_eq!(rig.call, CallState::Idle);
    assert!(
        rig.released.iter().all(Vec::is_empty),
        "nothing was released for playback"
    );
}

#[test]
fn a_provider_failure_before_translation_starts_leaves_the_call_audible_with_the_failure_policy() {
    let mut rig = Rig::with_defaults();
    rig.call_event(CallEvent::StartRequested {
        target: None,
        translate: true,
    })
    .expect("start");
    rig.call_event(CallEvent::ProvidersFailed {
        role: prxvc_session::ProviderRole::Tts,
        fault: FaultKind::Unavailable,
    })
    .expect("early failure is remembered");
    rig.call_event(CallEvent::AudioRunning).expect("audio");
    assert_eq!(
        rig.call,
        CallState::Degraded {
            reasons: [CallReason::Providers(prxvc_session::ProviderRole::Tts)].into()
        }
    );
    assert!(
        rig.log
            .contains(&support::Logged::Call(CallAction::ApplyFailurePolicy(
                DirSet::BOTH
            )))
    );
    assert_eq!(
        (rig.heard(OUT), rig.heard(IN)),
        (O, O),
        "never started, so the original stays"
    );
}

#[test]
fn sleep_suspends_the_directions_and_wake_resumes_them() {
    let mut rig = Rig::with_defaults();
    rig.start_translating();
    rig.call_event(CallEvent::SystemWillSleep).expect("sleep");
    assert!(matches!(rig.call, CallState::Recovering { .. }));
    for d in [OUT, IN] {
        assert!(matches!(rig.dir_state(d), DirectionState::Suspended { .. }));
        assert_eq!(rig.heard(d), O);
    }
    rig.call_event(CallEvent::SystemDidWake).expect("wake");
    rig.call_event(CallEvent::EndpointsRestored)
        .expect("restored");
    assert_eq!(rig.call, CallState::Translating);
    assert_eq!((rig.heard(OUT), rig.heard(IN)), (T, T));
    assert_eq!(
        rig.announcements(IN),
        vec![
            (O, T, OutputCause::TranslationStarted),
            (T, O, OutputCause::Suspended),
            (O, T, OutputCause::TranslationResumed),
        ]
    );
}

#[test]
fn recovery_that_times_out_ends_degraded_with_a_named_endpoint() {
    let mut rig = Rig::with_defaults();
    rig.start_translating();
    rig.call_event(CallEvent::EndpointLost(
        prxvc_session::Endpoint::RemoteOutput,
    ))
    .expect("lost");
    assert_eq!(rig.heard(OUT), O);
    rig.call_event(CallEvent::RecoveryTimeout {
        missing: prxvc_session::Endpoint::RemoteOutput,
    })
    .expect("timeout");
    assert_eq!(
        rig.call,
        CallState::Degraded {
            reasons: [CallReason::EndpointMissing(
                prxvc_session::Endpoint::RemoteOutput
            )]
            .into()
        }
    );
    // Stop is still legal from the degraded call and ends in Idle.
    rig.call_event(CallEvent::StopRequested).expect("stop");
    rig.call_event(CallEvent::Stopped).expect("stopped");
    assert_eq!(rig.call, CallState::Idle);
}

#[test]
fn illegal_events_are_rejected_without_changing_anything() {
    let mut rig = Rig::with_defaults();
    let err = rig
        .call_event(CallEvent::ResumeRequested)
        .expect_err("nothing to resume");
    assert_eq!(err.from, CallState::Idle);
    assert_eq!(rig.call, CallState::Idle);
    let blocker = Rig::blocker();
    rig.call_event(CallEvent::StartRequested {
        target: None,
        translate: false,
    })
    .expect("start");
    rig.call_event(CallEvent::PreflightFailed(blocker))
        .expect("blocked");
    assert_eq!(rig.call, CallState::Idle);
    assert!(rig.dir_event(OUT, DirectionEvent::Resume).is_err());
    assert_eq!(rig.dir_state(OUT), &DirectionState::Off);
}
