//! The live policy of the runaway guard (`RunawayRetry::ShortOnce`) and the speakable-text check.

use super::*;

const LONG: &str = "we should meet at the office tomorrow morning and then go home"; // cap well above the floor

#[test]
fn the_live_policy_holds_and_retries_a_short_clause_so_no_runaway_audio_is_ever_heard() {
    let (script, attempts) = runaway_once_then_fine();
    let mut c = cfg();
    c.runaway_retry = RunawayRetry::ShortOnce;
    let (tts, _p) = ready(script, c);
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(chunk_values(&items), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
    // A second runaway in a row fails the clause with no audio at all.
    let mut c = cfg();
    c.runaway_retry = RunawayRetry::ShortOnce;
    let (tts, _p) = ready(runaway_script(), c);
    let items = drain(open(&tts, Direction::Outgoing, SHORT).as_mut());
    assert!(
        matches!(items.as_slice(), [Err(TtsError::Runaway { .. })]),
        "{items:?}"
    );
}

#[test]
fn the_live_policy_retries_every_clause_it_holds_not_only_the_shortest() {
    // The call of 2026-11-21 ran away on clauses of four words (a cap just above the old floor of
    // 40 frames) and the old rule retried only a clause at the floor: six clauses failed without
    // a retry. Every length up to the hold limit is retried now.
    for text in [
        "good",
        "hello there",
        "we should go there",
        "we should meet at the office tomorrow",
        "we should meet at the office tomorrow morning",
    ] {
        let cap = crate::tts::qwentts::cap::cap_frames(text, Lang::En);
        assert!(cap <= crate::tts::qwentts::cap::HOLD_CAP_FRAMES, "{text}: cap {cap}");
        let (script, attempts) = runaway_once_then_fine();
        let mut c = cfg();
        c.runaway_retry = RunawayRetry::ShortOnce;
        let (tts, _p) = ready(script, c);
        let items = drain(open(&tts, Direction::Outgoing, text).as_mut());
        assert_eq!(attempts.load(Ordering::SeqCst), 2, "{text}: retried");
        assert!(matches!(items.last(), Some(Ok(TtsItem::Done { .. }))), "{text}: {items:?}");
        assert_eq!((tts.stats().runaways, tts.stats().retries), (1, 1), "{text}");
    }
}

#[test]
fn the_live_policy_streams_a_long_clause_without_a_retry() {
    let (script, attempts) = runaway_once_then_fine();
    let mut c = cfg();
    c.runaway_retry = RunawayRetry::ShortOnce;
    let (tts, _p) = ready(script, c);
    let items = drain(open(&tts, Direction::Outgoing, LONG).as_mut());
    assert!(matches!(items.last(), Some(Err(TtsError::Runaway { .. }))));
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "no retry for a long clause"
    );
}

#[test]
fn text_with_nothing_to_say_is_rejected_before_the_engine() {
    let (script, attempts) = runaway_once_then_fine();
    let (tts, _p) = ready(script, cfg());
    for text in ["...", "。", "  ", "\"\""] {
        let req = TtsRequest {
            clause: ClauseId::new(SegmentId::first(Direction::Outgoing), 0),
            text: text.to_owned(),
            lang: Lang::En,
            voice: VoiceSelection::Preset {
                name: "preset-a".to_owned(),
            },
        };
        assert!(
            matches!(tts.synthesize(req), Err(TtsError::Rejected { status: 400 })),
            "{text:?}"
        );
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
}
