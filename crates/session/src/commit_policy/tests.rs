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
    ]
}

/// Part 3 of the recognition rows.
fn recognition_rows_3() -> Vec<Row> {
    use ClauseReason::ClausePause;
    use CommitDecision::{CommitClause, Wait};
    let p = CommitParams::default();
    let base = |text: &'static str| input(text);
    vec![
        Row {
            name: "a sentence-final mark alone commits nothing here (the early rule does)",
            params: p,
            input: base("今天天气很好。"),
            want: Wait,
        },
        Row {
            name: "pause commits a finished sentence",
            params: p,
            input: RecognitionInput {
                silence: ms(300),
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
    let en = |finished, text, tokens| TranslationProgress {
        lang: Lang::En,
        finished,
        text,
        tokens_after_first_sentence: tokens,
    };
    let zh = |finished, text, tokens| TranslationProgress {
        lang: Lang::Zh,
        finished,
        text,
        tokens_after_first_sentence: tokens,
    };
    let text = "Hello there. And then";
    assert_eq!(
        decide_tts_commit(&whole, &en(false, text, 9)),
        TtsCommit::Wait
    );
    assert_eq!(
        decide_tts_commit(&whole, &en(true, text, 9)),
        TtsCommit::Whole
    );
    assert_eq!(
        decide_tts_commit(&whole, &en(true, "  ", 0)),
        TtsCommit::Wait,
        "nothing to say"
    );
    assert_eq!(
        decide_tts_commit(&split, &en(false, text, 3)),
        TtsCommit::Wait
    );
    assert_eq!(
        decide_tts_commit(&split, &en(false, text, 4)),
        TtsCommit::FirstSentence {
            end: "Hello there.".len()
        }
    );
    assert_eq!(
        decide_tts_commit(&split, &en(false, "no mark yet and more", 9)),
        TtsCommit::Wait
    );
    assert_eq!(
        decide_tts_commit(&split, &zh(false, "今天天气很好。然后", 5)),
        TtsCommit::FirstSentence {
            end: "今天天气很好。".len()
        }
    );
    assert_eq!(
        decide_tts_commit(&split, &en(true, "Hello there. And", 1)),
        TtsCommit::Whole
    );
}

#[test]
fn tts_commit_never_cuts_inside_numbers_or_abbreviations() {
    let split = CommitParams {
        split_target_clauses: true,
        split_min_tokens_after: 0,
        ..CommitParams::default()
    };
    let en = |text| TranslationProgress {
        lang: Lang::En,
        finished: false,
        text,
        tokens_after_first_sentence: 9,
    };
    for text in [
        "The total is 3.5 dollars today",
        "It costs 1,000 dollars today",
        "We meet at 10:30 tomorrow morning",
        "Dr. Smith will call you back",
        "The U.S. market opened higher",
    ] {
        assert_eq!(
            decide_tts_commit(&split, &en(text)),
            TtsCommit::Wait,
            "{text}"
        );
    }
    assert_eq!(
        decide_tts_commit(&split, &en("The total is 3.5 dollars. Then")),
        TtsCommit::FirstSentence {
            end: "The total is 3.5 dollars.".len()
        }
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
