//! Deterministic tests of early commit: translatable units are committed from the recogniser's stable
//! prefix while the speaker is still talking (pipeline-design 3.2.1, 5.2), with the fake recogniser on
//! a manual clock. Committed text is never retracted and the final is aligned against it (3.1).

mod pipeline_rig;

use std::sync::Arc;

use prxvc_media::text_units::units;
use prxvc_media::{ClauseId, Direction, ManualClock, SegmentId};
use prxvc_ports::fakes::{AsrScript, FakeSpeechSynth, FakeTranslator, MtScript, TtsScript};
use prxvc_session::pipeline::{DirectionSnapshot, PipelineEvent};
use prxvc_session::{ClauseState, EarlyCommit};

use pipeline_rig::{Rig, growing, ms, vad_runs};

const IN: Direction = Direction::Incoming;
const OUT: Direction = Direction::Outgoing;

fn clause(dir: Direction, seg: u64, index: u16) -> ClauseId {
    ClauseId::new(SegmentId::new(dir, seg).expect("seq >= 1"), index)
}

/// `n` growing partials of `text` character by character, the last `unstable` characters not stable.
fn growing_chars(text: &str, per_char: usize, unstable: usize) -> Vec<(String, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    for upto in 1..=chars.len() {
        let end = chars.get(upto).map_or(text.len(), |(at, _)| *at);
        let stable = upto
            .checked_sub(unstable)
            .filter(|s| *s > 0)
            .map_or(0, |s| chars.get(s).map_or(text.len(), |(at, _)| *at));
        for _ in 0..per_char {
            out.push((text[..end].to_owned(), stable));
        }
    }
    out
}

/// A recogniser script for one segment: the partials, then `fin` at the pause, then an empty round.
fn script(partials: Vec<(String, usize)>, fin: &str) -> AsrScript {
    let mut asr = AsrScript::new([], fin).then_round([], "");
    asr.partials = partials;
    asr
}

fn providers(clock: &ManualClock) -> (FakeTranslator, FakeSpeechSynth) {
    (
        FakeTranslator::new(clock.clone())
            .with_default_script(MtScript::tokens(["Ok ", "then."]))
            .with_script("Hello.", MtScript::tokens(["Hello."])),
        FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(1, 2400)),
    )
}

/// One utterance: `lead` silent frames, `speech` speech frames, then a pause that ends the segment.
fn one_utterance(lead: usize, speech: usize) -> prxvc_ports::fakes::VadScript {
    vad_runs(&[(0.0, lead), (0.9, speech), (0.0, 25)])
}

struct Run {
    rig: Rig,
    dir: Direction,
}

impl Run {
    fn start(
        dir: Direction,
        asr: AsrScript,
        vad: prxvc_ports::fakes::VadScript,
        early: EarlyCommit,
    ) -> Self {
        let clock = ManualClock::new();
        let (mt, tts) = providers(&clock);
        let rig = Rig::start_with(&clock, dir, vad, asr, mt, tts, move |p| {
            p.commit.early = early;
        });
        rig.wait_running();
        Self { rig, dir }
    }

    fn texts(snap: &DirectionSnapshot) -> Vec<String> {
        snap.clauses.iter().map(|c| c.source_text.clone()).collect()
    }

    /// Everything played: the snapshot with all clauses played and the segment closed.
    fn finish(&self, clauses: usize) -> Arc<DirectionSnapshot> {
        let played = |s: &DirectionSnapshot| {
            s.clauses.len() == clauses && s.clauses.iter().all(|c| c.state == ClauseState::Played)
        };
        for _ in 0..400 {
            if self.rig.snaps.latest().is_some_and(|s| played(&s)) {
                break;
            }
            self.rig.advance(ms(100));
            for _ in 0..500 {
                std::thread::yield_now();
            }
        }
        self.rig.wait_snapshot("all clauses played", played)
    }

    fn c(&self, index: u16) -> ClauseId {
        clause(self.dir, 1, index)
    }
}

fn key_units(text: &str) -> Vec<String> {
    units(text).into_iter().map(|u| u.key).collect()
}

const EN: &str = "we should meet at the office tomorrow morning and then go home";

#[test]
fn stable_text_with_no_punctuation_commits_while_the_speaker_goes_on() {
    let asr = script(growing(EN, 3, 6), EN);
    let run = Run::start(IN, asr, one_utterance(10, 70), EarlyCommit::Chunk);
    // 10 silent frames and 45 of the 70 speech frames: the speaker is still talking.
    run.rig.feed(10 + 45);
    let snap = run
        .rig
        .wait_snapshot("the first unit", |s| !s.clauses.is_empty());
    assert_eq!(Run::texts(&snap), ["we should meet at the office"]);
    assert!(
        snap.partial.is_some() || snap.activity != prxvc_session::Activity::Listening,
        "the segment is still open"
    );
    run.rig.feed(15 + 25 + 25);
    let snap = run.finish(2);
    // The remainder at the pause is the second unit; nothing is repeated or lost.
    assert_eq!(
        Run::texts(&snap),
        [
            "we should meet at the office",
            "tomorrow morning and then go home"
        ]
    );
    let joined = Run::texts(&snap).join(" ");
    assert_eq!(key_units(&joined), key_units(EN));
    assert_eq!(snap.counters.final_rewrites, 0);
    assert_eq!(snap.counters.stable_violations, 0);
    assert_eq!(snap.counters.late_dropped, 0);
    assert_eq!(snap.counters.rejected_events, 0);
    assert_eq!(
        run.rig.recorder.audio_clause_order(),
        vec![run.c(0), run.c(1)]
    );
    // The second unit was translated with the first (same segment) as context.
    let requests = run.rig.fakes.mt.requests();
    let second = requests
        .iter()
        .find(|r| r.text == "tomorrow morning and then go home")
        .expect("second request");
    assert_eq!(
        second.context.last().map(|(s, _)| s.as_str()),
        Some("we should meet at the office")
    );
    assert!(run.rig.stop().clean);
}

#[test]
fn segment_end_mode_commits_only_at_the_pause() {
    let asr = script(growing(EN, 3, 6), EN);
    let run = Run::start(IN, asr, one_utterance(10, 70), EarlyCommit::SegmentEnd);
    run.rig.feed(10 + 60);
    assert!(
        run.rig.latest().clauses.is_empty(),
        "nothing is committed while the speaker talks"
    );
    run.rig.feed(10 + 25 + 25);
    let snap = run.finish(1);
    assert_eq!(Run::texts(&snap), [EN]);
    assert!(run.rig.stop().clean);
}

#[test]
fn a_recogniser_with_finals_only_is_unchanged_by_early_commit() {
    // No stable prefix at all (every partial declares zero bytes).
    let partials: Vec<(String, usize)> =
        growing(EN, 3, 6).into_iter().map(|(t, _)| (t, 0)).collect();
    let run = Run::start(
        IN,
        script(partials, EN),
        one_utterance(10, 70),
        EarlyCommit::Chunk,
    );
    run.rig.feed(10 + 70);
    assert!(run.rig.latest().clauses.is_empty());
    run.rig.feed(25 + 25);
    let snap = run.finish(1);
    assert_eq!(Run::texts(&snap), [EN]);
    assert!(run.rig.stop().clean);
}

#[test]
fn punctuation_mode_commits_at_the_mark_and_not_without_one() {
    let text = "if it rains tomorrow, we will move the meeting to friday and tell everyone";
    let run = Run::start(
        IN,
        script(growing(text, 3, 6), text),
        one_utterance(10, 100),
        EarlyCommit::Punctuation,
    );
    run.rig.feed(10 + 60);
    let snap = run
        .rig
        .wait_snapshot("the unit at the comma", |s| !s.clauses.is_empty());
    assert_eq!(Run::texts(&snap), ["if it rains tomorrow,"]);
    // Nothing else is committed before the pause: no mark, no chunk rule in this mode.
    run.rig.feed(40);
    assert_eq!(run.rig.latest().clauses.len(), 1);
    run.rig.feed(25 + 25);
    let snap = run.finish(2);
    assert_eq!(
        Run::texts(&snap),
        [
            "if it rains tomorrow,",
            "we will move the meeting to friday and tell everyone"
        ]
    );
    assert!(run.rig.stop().clean);
}

#[test]
fn a_chinese_utterance_commits_at_the_clause_mark_and_then_by_chunk() {
    let text = "如果明天下雨，我们就改期，然后通知大家明天不用来公司了";
    let run = Run::start(
        OUT,
        script(growing_chars(text, 2, 6), text),
        one_utterance(10, 120),
        EarlyCommit::Chunk,
    );
    run.rig.feed(10 + 100);
    let snap = run.rig.wait_snapshot("units", |s| s.clauses.len() >= 2);
    let texts = Run::texts(&snap);
    assert_eq!(texts[0], "如果明天下雨，");
    run.rig.feed(20 + 25 + 25);
    let snap = run.finish(texts.len().max(Run::texts(&run.rig.latest()).len()));
    let all = Run::texts(&snap);
    assert_eq!(key_units(&all.join("")), key_units(text));
    assert_eq!(snap.counters.final_rewrites, 0);
    assert_eq!(snap.counters.stable_violations, 0);
    assert!(run.rig.stop().clean);
}

#[test]
fn numbers_times_dates_and_names_stay_inside_one_unit() {
    for (dir, text, whole) in [
        (
            IN,
            "the budget for the whole project is about 3.5 million dollars and we agreed to it",
            "3.5 million",
        ),
        (
            IN,
            "the train leaves tomorrow morning at 10:30 from the main station near the river",
            "10:30",
        ),
        (
            IN,
            "we agreed to hold the review meeting on March 3rd at the office with everyone",
            "March 3rd",
        ),
        (
            OUT,
            "明天上午我们要和王建国一起去机场接客人然后回公司开会讨论项目",
            "王建国",
        ),
    ] {
        let partials = if dir == IN {
            growing(text, 3, 6)
        } else {
            growing_chars(text, 2, 6)
        };
        let run = Run::start(
            dir,
            script(partials, text),
            one_utterance(10, 140),
            EarlyCommit::Chunk,
        );
        run.rig.feed(10 + 140 + 25 + 25);
        let snap = run.rig.wait_snapshot("all committed", |s| {
            let joined: Vec<String> = s.clauses.iter().map(|c| c.source_text.clone()).collect();
            key_units(&joined.join(" ")) == key_units(text)
        });
        let texts = Run::texts(&snap);
        assert!(
            texts.len() >= 2,
            "{texts:?}: the text was committed in parts"
        );
        assert!(
            texts.iter().any(|t| t.contains(whole)),
            "{whole:?} was split across {texts:?}"
        );
        assert!(run.rig.stop().clean);
    }
}

#[test]
fn a_rewrite_outside_the_stable_prefix_does_not_disturb_the_commits() {
    // The unstable tail keeps changing; the stable prefix and the final agree.
    let base = "we should meet at the office tomorrow morning";
    let mut partials = Vec::new();
    for (i, tail) in [
        "and then",
        "and than go",
        "and then go ho",
        "or then go home",
    ]
    .iter()
    .cycle()
    .take(40)
    .enumerate()
    {
        let text = format!("{base} {tail}");
        let _ = i;
        partials.push((text, "we should meet at the office".len()));
    }
    let fin = "we should meet at the office tomorrow morning and then go home";
    let run = Run::start(
        IN,
        script(partials, fin),
        one_utterance(10, 60),
        EarlyCommit::Chunk,
    );
    run.rig.feed(10 + 60 + 25 + 25);
    let snap = run.finish(2);
    assert_eq!(
        Run::texts(&snap),
        [
            "we should meet at the office",
            "tomorrow morning and then go home"
        ]
    );
    assert_eq!(snap.counters.stable_violations, 0);
    assert_eq!(snap.counters.final_rewrites, 0);
    assert!(run.rig.stop().clean);
}

#[test]
fn a_final_that_disagrees_with_a_committed_unit_is_counted_and_the_unit_stays() {
    let fin = "we should meat at the office tomorrow morning and then go home";
    let run = Run::start(
        IN,
        script(growing(EN, 3, 6), fin),
        one_utterance(10, 70),
        EarlyCommit::Chunk,
    );
    run.rig.feed(10 + 70 + 25 + 25);
    let snap = run.finish(2);
    assert_eq!(
        Run::texts(&snap),
        [
            "we should meet at the office",
            "tomorrow morning and then go home"
        ],
        "the committed unit stays as it was said to the listener"
    );
    assert_eq!(snap.counters.final_rewrites, 1);
    assert_eq!(snap.counters.final_unaligned, 0);
    assert!(run.rig.stop().clean);
}

#[test]
fn the_maximum_wait_commits_a_smaller_chunk_of_slowly_stabilising_text() {
    let text = "we should meet at the office";
    // Only the first four words are stable for a long time.
    let partials = vec![(text.to_owned(), "we should meet at".len()); 200];
    let run = Run::start(
        IN,
        script(partials, text),
        one_utterance(10, 200),
        EarlyCommit::Chunk,
    );
    // 1.9 s of audio: below the maximum wait of 2.5 s.
    run.rig.feed(10 + 50);
    assert!(run.rig.latest().clauses.is_empty());
    run.rig.feed(60);
    let snap = run
        .rig
        .wait_snapshot("the late unit", |s| !s.clauses.is_empty());
    assert_eq!(Run::texts(&snap)[0], "we should meet");
    run.rig.feed(80 + 25 + 25);
    assert!(run.rig.stop().clean);
}

#[test]
fn units_of_one_segment_are_translated_and_spoken_in_order_whatever_each_takes() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script(
            "we should meet at the office",
            MtScript::tokens(["We ", "meet ", "at ", "the ", "office."]),
        )
        .with_default_script(MtScript::tokens(["Then."]))
        .with_script("Hello.", MtScript::tokens(["Hello."]));
    let tts = FakeSpeechSynth::new(clock.clone())
        .with_script("We meet at the office.", TtsScript::chunks(4, 2400))
        .with_default_script(TtsScript::chunks(1, 2400));
    let asr = script(growing(EN, 3, 6), EN);
    let rig = Rig::start_with(&clock, IN, one_utterance(10, 70), asr, mt, tts, |p| {
        p.commit.early = EarlyCommit::Chunk;
    });
    rig.wait_running();
    rig.feed(10 + 70 + 25 + 25);
    rig.advance(ms(500));
    rig.wait_events("both units spoken", |e| {
        e.iter()
            .filter(|x| matches!(x, PipelineEvent::AudioEnd(_)))
            .count()
            == 2
    });
    let (c0, c1) = (clause(IN, 1, 0), clause(IN, 1, 1));
    assert_eq!(rig.recorder.audio_clause_order(), vec![c0, c1]);
    assert_eq!(rig.recorder.chunks_of(c0), 4);
    assert_eq!(rig.recorder.chunks_of(c1), 1);
    assert!(rig.stop().clean);
}

#[test]
fn with_a_refine_pass_early_units_and_the_rest_of_their_round_keep_the_streaming_text() {
    use prxvc_ports::fakes::{FakeRefineAsr, FakeTraceSink, FakeVadProvider, RefineScript};
    use prxvc_ports::CancellationToken;
    use prxvc_session::pipeline::{PipelineProviders, RecognitionMode};
    use pipeline_rig::{Fakes, params};

    let clock = ManualClock::new();
    let (mt, tts) = providers(&clock);
    let refine = FakeRefineAsr::new(
        clock.clone(),
        RefineScript::new("refined text", std::time::Duration::ZERO),
    );
    let fakes = Fakes {
        vad: FakeVadProvider::new(one_utterance(10, 70)),
        asr: prxvc_ports::fakes::FakeStreamingAsr::new(
            clock.clone(),
            script(growing(EN, 3, 6), EN),
        ),
        mt,
        tts,
    };
    let providers = PipelineProviders {
        vad: Arc::new(fakes.vad.clone()),
        asr: Arc::new(fakes.asr.clone()),
        refine: Some(Arc::new(refine.clone())),
        mt: Arc::new(fakes.mt.clone()),
        tts: Arc::new(fakes.tts.clone()),
    };
    let mut p = params(IN);
    p.recognition_mode = RecognitionMode::TwoPass;
    p.commit.early = EarlyCommit::Chunk;
    let rig = Rig::launch_with_providers(
        clock,
        fakes,
        providers,
        p,
        &CancellationToken::new(),
        FakeTraceSink::new(false),
    );
    rig.wait_running();
    rig.feed(10 + 70 + 25 + 25);
    let run = Run { rig, dir: IN };
    let snap = run.finish(2);
    assert_eq!(
        Run::texts(&snap),
        ["we should meet at the office", "tomorrow morning and then go home"],
        "the clip of a cut chosen from text would not match the audio: no refine for them"
    );
    assert!(refine.audio_lens().is_empty());
    assert!(run.rig.stop().clean);
}

/// Partials of `text` up to `upto` words, then partials of `rewritten` from there on with nothing
/// declared stable: the recogniser rewrote a word inside the part that was already committed.
fn rewriting_partials(text: &str, rewritten: &str, upto: usize) -> Vec<(String, usize)> {
    let mut partials = growing(text, 3, 6);
    partials.truncate(upto * 3);
    let later = growing(rewritten, 3, 6);
    partials.extend(later.into_iter().skip(upto * 3).map(|(t, _)| (t, 0)));
    partials
}

#[test]
fn a_partial_that_rewrites_committed_text_keeps_the_boundary_after_the_committed_words() {
    let text = "if it rains tomorrow, we will move the meeting to friday and tell everyone";
    // "rains" becomes "raining" inside the committed part: the byte offset of the commit now falls
    // inside "tomorrow".
    let rewritten = "if it raining tomorrow, we will move the meeting to friday and tell everyone";
    let run = Run::start(
        IN,
        script(rewriting_partials(text, rewritten, 10), rewritten),
        one_utterance(10, 100),
        EarlyCommit::Punctuation,
    );
    // While the speaker still talks, the open clause starts after the committed words.
    run.rig.feed(10 + 100);
    let snap = run.rig.wait_snapshot("the rewritten partial", |s| {
        s.partial
            .as_ref()
            .is_some_and(|p| p.text.ends_with("everyone"))
    });
    assert_eq!(
        snap.partial.as_ref().map(|p| p.text.as_str()),
        Some("we will move the meeting to friday and tell everyone")
    );
    run.rig.feed(25 + 25);
    let snap = run.finish(2);
    assert_eq!(
        Run::texts(&snap),
        [
            "if it rains tomorrow,",
            "we will move the meeting to friday and tell everyone"
        ]
    );
    assert_eq!(snap.counters.stable_violations, 1);
    assert!(run.rig.stop().clean);
}

#[test]
fn a_chinese_partial_that_rewrites_committed_text_keeps_the_boundary_after_the_committed_words() {
    let text = "如果明天下雨，我们就改期然后通知大家明天不用来公司了";
    // A character inside the committed part is dropped: the committed byte offset now falls in
    // the middle of the next unit, which is cut from its first character.
    let rewritten = "如果明天雨，我们就改期然后通知大家明天不用来公司了";
    let mut partials = growing_chars(text, 2, 6);
    partials.truncate(13 * 2);
    partials.extend(
        growing_chars(rewritten, 2, 6)
            .into_iter()
            .skip(14 * 2)
            .map(|(t, _)| (t, 0)),
    );
    let run = Run::start(
        OUT,
        script(partials, rewritten),
        one_utterance(10, 120),
        EarlyCommit::Punctuation,
    );
    run.rig.feed(10 + 120 + 25 + 25);
    let snap = run.finish(2);
    assert_eq!(
        Run::texts(&snap),
        ["如果明天下雨，", "我们就改期然后通知大家明天不用来公司了"]
    );
    assert_eq!(snap.counters.stable_violations, 1);
    assert!(run.rig.stop().clean);
}

#[test]
fn with_continuation_on_the_second_unit_is_translated_from_the_start_of_the_sentence() {
    let text = "if it rains tomorrow, we will move the meeting to friday and tell everyone.";
    let whole = "如果明天下雨，我们将把会议改到周五，并通知大家。";
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script(
            "if it rains tomorrow,",
            MtScript::tokens(["如果明天下雨，"]),
        )
        .with_script(text, MtScript::tokens([whole]));
    let tts = FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(1, 2400));
    let rig = Rig::start_with(
        &clock,
        IN,
        one_utterance(10, 100),
        script(growing(text, 3, 6), text),
        mt,
        tts,
        |p| {
            p.commit.early = EarlyCommit::Punctuation;
            p.retranslate.enabled = true;
            p.retranslate.continuation = true;
        },
    );
    rig.wait_running();
    let run = Run { rig, dir: IN };
    run.rig.feed(10 + 100 + 25 + 25);
    let snap = run.finish(2);
    assert_eq!(
        Run::texts(&snap),
        [
            "if it rains tomorrow,",
            "we will move the meeting to friday and tell everyone."
        ]
    );
    // The second unit was asked for from the start of its sentence and speaks only what follows
    // the first; nothing is repeated.
    let requests = run.rig.fakes.mt.requests();
    let asked: Vec<&str> = requests.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(asked, ["if it rains tomorrow,", text]);
    let translations: Vec<&str> = snap
        .clauses
        .iter()
        .map(|c| c.translation.as_str())
        .collect();
    assert_eq!(
        translations,
        ["如果明天下雨，", "我们将把会议改到周五，并通知大家。"]
    );
    assert_eq!(snap.counters.continued, 1);
    assert_eq!(snap.counters.continuation_diverged, 0);
    assert_eq!(
        run.rig.recorder.audio_clause_order(),
        vec![run.c(0), run.c(1)]
    );
    assert!(run.rig.stop().clean);
}
