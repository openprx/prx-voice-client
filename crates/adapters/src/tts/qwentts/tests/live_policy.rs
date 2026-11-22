//! The live policy of the runaway guard (`RunawayRetry::ShortOnce`) and the speakable-text check.

use super::*;
use crate::tts::qwentts::HoldLimit;

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
        assert!(
            cap <= crate::tts::qwentts::cap::HOLD_CAP_FRAMES,
            "{text}: cap {cap}"
        );
        let (script, attempts) = runaway_once_then_fine();
        let mut c = cfg();
        c.runaway_retry = RunawayRetry::ShortOnce;
        let (tts, _p) = ready(script, c);
        let items = drain(open(&tts, Direction::Outgoing, text).as_mut());
        assert_eq!(attempts.load(Ordering::SeqCst), 2, "{text}: retried");
        assert!(
            matches!(items.last(), Some(Ok(TtsItem::Done { .. }))),
            "{text}: {items:?}"
        );
        assert_eq!(
            (tts.stats().runaways, tts.stats().retries),
            (1, 1),
            "{text}"
        );
    }
}

#[test]
fn slow_speech_cut_by_the_cap_twice_is_accepted_at_twice_the_cap_and_a_real_runaway_still_fails() {
    // A rare name read slowly: 40 frames (3.2 s) of speech for a text whose cap is 32 frames. The
    // first two attempts are cut by the cap; the last one has room and the engine stops by itself.
    let calls: Arc<Mutex<Vec<u32>>> = Arc::default();
    let seen = Arc::clone(&calls);
    let script: Script = Box::new(move |call, ctl| {
        seen.lock().push(call.max_new_tokens);
        let natural = 40u32;
        for _ in 0..natural.min(call.max_new_tokens) {
            if ctl.should_abort() {
                return Ok(CallEnd::Aborted);
            }
            ctl.on_chunk(&vec![0.1; FRAME]);
        }
        Ok(CallEnd::Completed)
    });
    let mut c = cfg();
    c.runaway_retry = RunawayRetry::ShortOnce;
    let (tts, _p) = ready(script, c);
    let items = drain(open(&tts, Direction::Outgoing, "hello there").as_mut());
    assert!(
        matches!(items.last(), Some(Ok(TtsItem::Done { .. }))),
        "{items:?}"
    );
    assert_eq!(
        chunk_values(&items).len(),
        40,
        "the whole slow clause is spoken once"
    );
    let caps = calls.lock().clone();
    assert_eq!(caps.len(), 3, "{caps:?}");
    assert_eq!(caps[0], caps[1]);
    assert!(caps[2] >= 2 * caps[0], "{caps:?}");
    assert_eq!((tts.stats().runaways, tts.stats().retries), (2, 2));

    // Audio that does not stop even at twice the cap fails the clause, with no audio heard.
    let mut c = cfg();
    c.runaway_retry = RunawayRetry::ShortOnce;
    let (tts, _p) = ready(runaway_script(), c);
    let items = drain(open(&tts, Direction::Outgoing, "hello there").as_mut());
    assert!(
        matches!(items.as_slice(), [Err(TtsError::Runaway { .. })]),
        "{items:?}"
    );
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

// The hold limit (`pipeline.tts_hold_max_*`): only a very short clause is held back.

const THREE_WORDS: &str = "see you later";
const FOUR_WORDS: &str = "we should go there";

fn limited() -> QwenTtsConfig {
    let mut c = cfg();
    c.runaway_retry = RunawayRetry::ShortOnce;
    c.hold = HoldLimit::clamped(3, 4);
    c
}

#[test]
fn a_clause_above_the_hold_limit_streams_its_first_chunk_before_the_clause_ends() {
    let gate = Arc::new(AtomicBool::new(false));
    let opened = Arc::new(AtomicBool::new(false));
    let (g, o) = (Arc::clone(&gate), Arc::clone(&opened));
    let script: Script = Box::new(move |_, ctl| {
        ctl.on_chunk(&vec![1.0; FRAME]);
        // The clause cannot end until the consumer has the first chunk: a held clause would wait
        // for the end and never open the gate.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if g.load(Ordering::SeqCst) {
                o.store(true, Ordering::SeqCst);
                break;
            }
            std::thread::yield_now();
        }
        ctl.on_chunk(&vec![2.0; FRAME]);
        Ok(CallEnd::Completed)
    });
    let (tts, _p) = ready(script, limited());
    let mut s = open(&tts, Direction::Outgoing, FOUR_WORDS);
    let never = CancellationToken::new();
    let first = s.next(&never);
    assert!(matches!(first, Some(Ok(TtsItem::Chunk(_)))), "{first:?}");
    gate.store(true, Ordering::SeqCst);
    let rest = drain(s.as_mut());
    assert!(
        opened.load(Ordering::SeqCst),
        "the first chunk came before the clause ended"
    );
    assert!(
        matches!(rest.last(), Some(Ok(TtsItem::Done { .. }))),
        "{rest:?}"
    );
    let stats = tts.stats();
    assert_eq!((stats.held, stats.streamed), (0, 1));
}

#[test]
fn a_streamed_clause_that_runs_away_is_cut_by_the_cap_without_a_retry_and_keeps_what_played() {
    let (tts, _p) = ready(runaway_script(), limited());
    let items = drain(open(&tts, Direction::Outgoing, FOUR_WORDS).as_mut());
    assert!(
        matches!(items.last(), Some(Err(TtsError::Runaway { .. }))),
        "{items:?}"
    );
    assert!(
        !chunk_values(&items).is_empty(),
        "what was produced before the cap was delivered"
    );
    let stats = tts.stats();
    assert_eq!((stats.runaways, stats.retries), (1, 0));
    assert_eq!((stats.held, stats.streamed), (0, 1));
}

#[test]
fn a_clause_at_the_hold_limit_is_held_and_retried_exactly_as_before() {
    for (text, lang) in [
        (THREE_WORDS, Lang::En),
        ("\u{4f60}\u{597d}\u{5417}\u{ff1f}", Lang::Zh),
    ] {
        let (script, attempts) = runaway_once_then_fine();
        let (tts, _p) = ready(script, limited());
        let mut req = request(Direction::Outgoing, text, clone_of("me"));
        req.lang = lang;
        let items = drain(tts.synthesize(req).expect("stream").as_mut());
        assert_eq!(attempts.load(Ordering::SeqCst), 2, "{text}: retried");
        assert_eq!(
            chunk_values(&items),
            vec![1.0, 2.0, 3.0, 4.0, 5.0],
            "{text}"
        );
        let stats = tts.stats();
        assert_eq!((stats.runaways, stats.retries), (1, 1), "{text}");
        assert_eq!((stats.held, stats.streamed), (1, 0), "{text}");
    }
}

#[test]
fn the_legacy_hold_limit_holds_the_clause_that_the_limit_streams() {
    let (script, attempts) = runaway_once_then_fine();
    let mut c = limited();
    c.hold = HoldLimit::LEGACY;
    let (tts, _p) = ready(script, c);
    let items = drain(open(&tts, Direction::Outgoing, FOUR_WORDS).as_mut());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "held and retried as before"
    );
    assert_eq!(chunk_values(&items), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!((tts.stats().held, tts.stats().streamed), (1, 0));
}
