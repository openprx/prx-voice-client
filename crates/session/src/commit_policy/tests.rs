//! Table-driven tests of the commit rules with explicit durations; no clock is involved.

use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn input<'a>(text: &'a str) -> RecognitionInput<'a> {
    RecognitionInput {
        silence: Duration::ZERO,
        uncommitted_text: text,
        uncommitted_audio: ms(1_000),
        segment_audio: ms(1_000),
        text_unchanged_for: Duration::ZERO,
        has_stable_word_boundary: true,
    }
}

struct Row {
    name: &'static str,
    params: CommitParams,
    input: RecognitionInput<'static>,
    want: CommitDecision,
}

/// Part 1 of the recognition rows.
fn recognition_rows_1() -> Vec<Row> {
    use ClauseReason::ClausePause;
    use CommitDecision::{CommitClause, EndSegment, Wait};
    let p = CommitParams::default();
    let base = |text: &'static str| input(text);
    vec![
        Row {
            name: "speech goes on",
            params: p,
            input: base("你好世界今天"),
            want: Wait,
        },
        Row {
            name: "pause below the threshold",
            params: p,
            input: RecognitionInput {
                silence: ms(299),
                ..base("你好世界今天")
            },
            want: Wait,
        },
        Row {
            name: "clause pause with four Chinese characters",
            params: p,
            input: RecognitionInput {
                silence: ms(300),
                ..base("你好世界")
            },
            want: CommitClause(ClausePause),
        },
        Row {
            name: "clause pause with three characters is ignored",
            params: p,
            input: RecognitionInput {
                silence: ms(300),
                ..base("你好吗")
            },
            want: Wait,
        },
        Row {
            name: "clause pause with two English words",
            params: p,
            input: RecognitionInput {
                silence: ms(300),
                ..base("good morning")
            },
            want: CommitClause(ClausePause),
        },
        Row {
            name: "clause pause with one English word is ignored",
            params: p,
            input: RecognitionInput {
                silence: ms(450),
                ..base("hello")
            },
            want: Wait,
        },
        Row {
            name: "segment end commits the last clause even if short",
            params: p,
            input: RecognitionInput {
                silence: ms(600),
                ..base("好")
            },
            want: EndSegment {
                commit_clause: true,
                reason: EndReason::SegmentPause,
            },
        },
    ]
}

/// Part 2 of the recognition rows.
fn recognition_rows_2() -> Vec<Row> {
    use ClauseReason::LengthCut;
    use CommitDecision::{CommitClause, EndSegment, Wait};
    let p = CommitParams::default();
    let base = |text: &'static str| input(text);
    vec![
        Row {
            name: "segment end without text commits nothing",
            params: p,
            input: RecognitionInput {
                silence: ms(600),
                ..base("  ")
            },
            want: EndSegment {
                commit_clause: false,
                reason: EndReason::SegmentPause,
            },
        },
        Row {
            name: "long segment ends after the shorter pause",
            params: p,
            input: RecognitionInput {
                silence: ms(300),
                segment_audio: ms(8_000),
                ..base("one two three")
            },
            want: EndSegment {
                commit_clause: true,
                reason: EndReason::SegmentPause,
            },
        },
        Row {
            name: "length cut at a stable word boundary",
            params: p,
            input: RecognitionInput {
                uncommitted_audio: ms(8_000),
                ..base("a long clause")
            },
            want: CommitClause(LengthCut(CutPoint::LastStableWord)),
        },
        Row {
            name: "length cut without a stable boundary uses the VAD minimum",
            params: p,
            input: RecognitionInput {
                uncommitted_audio: ms(8_500),
                has_stable_word_boundary: false,
                ..base("a long clause")
            },
            want: CommitClause(LengthCut(CutPoint::LowestVadFrame)),
        },
        Row {
            name: "length cut needs text",
            params: p,
            input: RecognitionInput {
                uncommitted_audio: ms(9_000),
                ..base("")
            },
            want: Wait,
        },
        Row {
            name: "segment length limit ends the segment",
            params: p,
            input: RecognitionInput {
                segment_audio: ms(25_000),
                ..base("keep talking")
            },
            want: EndSegment {
                commit_clause: true,
                reason: EndReason::MaxSegmentLength,
            },
        },
        Row {
            name: "stable punctuation is off by default",
            params: p,
            input: RecognitionInput {
                text_unchanged_for: ms(900),
                ..base("今天天气很好。")
            },
            want: Wait,
        },
    ]
}

/// Part 3 of the recognition rows.
fn recognition_rows_3() -> Vec<Row> {
    use ClauseReason::{ClausePause, StablePunctuation};
    use CommitDecision::{CommitClause, Wait};
    let p = CommitParams::default();
    let stable = CommitParams {
        commit_on_stable_partial: true,
        ..p
    };
    let base = |text: &'static str| input(text);
    vec![
        Row {
            name: "stable punctuation commits when enabled and unchanged long enough",
            params: stable,
            input: RecognitionInput {
                text_unchanged_for: ms(400),
                ..base("今天天气很好。")
            },
            want: CommitClause(StablePunctuation),
        },
        Row {
            name: "stable punctuation waits while the prefix still changes",
            params: stable,
            input: RecognitionInput {
                text_unchanged_for: ms(399),
                ..base("今天天气很好。")
            },
            want: Wait,
        },
        Row {
            name: "no sentence-final mark, no stable-punctuation commit",
            params: stable,
            input: RecognitionInput {
                text_unchanged_for: ms(900),
                ..base("今天天气很好")
            },
            want: Wait,
        },
        Row {
            name: "pause wins over stable punctuation",
            params: stable,
            input: RecognitionInput {
                silence: ms(300),
                text_unchanged_for: ms(900),
                ..base("Is it ready?")
            },
            want: CommitClause(ClausePause),
        },
        Row {
            name: "mixed text below both thresholds is ignored (the counts are not added)",
            params: p,
            input: RecognitionInput {
                silence: ms(300),
                ..base("我要 iPhone")
            },
            want: Wait,
        },
    ]
}

#[test]
fn recognition_commit_rules() {
    let rows: Vec<Row> = [
        recognition_rows_1(),
        recognition_rows_2(),
        recognition_rows_3(),
    ]
    .into_iter()
    .flatten()
    .collect();
    for r in rows {
        assert_eq!(
            decide_recognition_commit(&r.params, &r.input),
            r.want,
            "{}",
            r.name
        );
    }
}

#[test]
fn clause_size_counts_characters_and_words() {
    let p = CommitParams::default();
    assert!(is_clause_sized(&p, "你好世界"));
    assert!(!is_clause_sized(&p, "你好吗"));
    assert!(is_clause_sized(&p, "two words"));
    assert!(!is_clause_sized(&p, "word"));
    assert!(!is_clause_sized(&p, "... !!"));
}

#[test]
fn thresholds_come_from_the_parameters() {
    let p = CommitParams {
        clause_pause: ms(250),
        min_clause_chars: 2,
        ..CommitParams::default()
    };
    let i = RecognitionInput {
        silence: ms(250),
        ..input("你好")
    };
    assert_eq!(
        decide_recognition_commit(&p, &i),
        CommitDecision::CommitClause(ClauseReason::ClausePause)
    );
}

#[test]
fn tts_commit_rules() {
    let split = CommitParams {
        split_target_clauses: true,
        ..CommitParams::default()
    };
    let whole = CommitParams::default();
    let t = |finished, text, tokens| TranslationProgress {
        finished,
        text,
        tokens_after_first_sentence: tokens,
    };
    assert_eq!(
        decide_tts_commit(&whole, &t(false, "Hello. And then", 9)),
        TtsCommit::Wait
    );
    assert_eq!(
        decide_tts_commit(&whole, &t(true, "Hello. And then", 9)),
        TtsCommit::Whole
    );
    assert_eq!(
        decide_tts_commit(&whole, &t(true, "  ", 0)),
        TtsCommit::Wait,
        "nothing to say"
    );
    assert_eq!(
        decide_tts_commit(&split, &t(false, "Hello. And then", 3)),
        TtsCommit::Wait
    );
    assert_eq!(
        decide_tts_commit(&split, &t(false, "Hello. And then", 4)),
        TtsCommit::FirstSentence { end: 6 }
    );
    assert_eq!(
        decide_tts_commit(&split, &t(false, "no mark yet and more", 9)),
        TtsCommit::Wait
    );
    assert_eq!(
        decide_tts_commit(&split, &t(false, "你好。然后", 5)),
        TtsCommit::FirstSentence {
            end: "你好。".len()
        }
    );
    assert_eq!(
        decide_tts_commit(&split, &t(true, "Hello. And", 1)),
        TtsCommit::Whole
    );
}

#[test]
fn speculative_translation_is_used_only_for_identical_text() {
    assert_eq!(check_speculation("今天下雨", "今天下雨"), Speculation::Use);
    assert_eq!(
        check_speculation("今天下雨", "今天下雨了"),
        Speculation::CancelAndRerun
    );
}
