//! What a reply that is not a translation looks like: an apology or refusal, a chat answer, a note after the text (PD-2.6.3).
//!
//! Pure checks on text, shared by the translation adapter (which flags a finished output) and the
//! pipeline (which looks at the start of a translation before any of it is spoken, to ask for it
//! once more with the strict instruction). Placement as for [`crate::script`]: the pipeline may
//! not depend on the adapters.
//!
//! An opener is legitimate when the source itself contains a matching word: a speaker may really
//! say "sorry", "of course" or "好的", and a translation of "知道了" or "明白了" is "Okay, got it".

/// `(output opener, words in the source that make the opener legitimate)`.
type Rule = (&'static str, &'static [&'static str]);

const REFUSAL_RULES: [Rule; 17] = [
    ("i'm sorry", &["抱歉", "对不起", "不好意思", "sorry"]),
    ("i am sorry", &["抱歉", "对不起", "不好意思", "sorry"]),
    ("sorry,", &["抱歉", "对不起", "不好意思", "sorry"]),
    ("i apologize", &["抱歉", "对不起", "不好意思", "apolog"]),
    (
        "i cannot",
        &["无法", "不能", "不可以", "不会", "cannot", "can't"],
    ),
    (
        "i can't",
        &["无法", "不能", "不可以", "不会", "cannot", "can't"],
    ),
    (
        "i can not",
        &["无法", "不能", "不可以", "不会", "cannot", "can't"],
    ),
    ("i'm unable", &["无法", "不能", "不可以", "unable"]),
    ("i am unable", &["无法", "不能", "不可以", "unable"]),
    (
        "as an ai",
        &["ai", "人工智能", "语言模型", "language model"],
    ),
    (
        "as a language model",
        &["ai", "人工智能", "语言模型", "language model"],
    ),
    ("抱歉", &["sorry", "apolog", "excuse"]),
    ("对不起", &["sorry", "apolog", "excuse"]),
    ("很抱歉", &["sorry", "apolog", "excuse"]),
    ("我无法", &["cannot", "can't", "unable", "can not"]),
    ("我不能", &["cannot", "can't", "unable", "can not"]),
    ("作为一个ai", &["ai", "language model", "artificial"]),
];

const ANSWER_RULES: [Rule; 15] = [
    (
        "sure",
        &["好的", "当然", "没问题", "行", "可以", "确定", "确实", "没错", "对", "sure"],
    ),
    ("certainly", &["当然", "肯定", "一定", "certainly"]),
    ("of course", &["当然", "自然", "of course"]),
    (
        "absolutely",
        &["当然", "完全", "绝对", "没错", "absolutely"],
    ),
    ("okay", &["好", "行", "可以", "嗯", "知道", "明白", "了解", "懂", "收到", "没事", "没关系", "对", "是", "ok"]),
    ("here is", &["这是", "这里", "给你", "以下", "here"]),
    ("here's", &["这是", "这里", "给你", "以下", "here"]),
    (
        "好的",
        &[
            "ok", "sure", "alright", "fine", "good", "right", "yes", "yeah",
        ],
    ),
    (
        "当然",
        &["sure", "of course", "certainly", "definitely", "absolutely"],
    ),
    ("没问题", &["no problem", "sure", "fine", "ok", "of course"]),
    ("以下是", &["here", "below", "following"]),
    ("翻译如下", &["translat"]),
    ("我来翻译", &["translat"]),
    ("这是翻译", &["translat"]),
    ("好的，我", &["ok", "sure", "alright", "i will", "i'll"]),
];

/// Labels of a model's own commentary on a later line of the output.
const NOTE_LABELS: [&str; 7] = [
    "note:",
    "(note",
    "[note",
    "注：",
    "（注",
    "注意：",
    "explanation:",
];


/// Whether `output` opens like an apology or an AI self-reference ("I'm sorry", "I cannot", "抱歉")
/// that the source does not itself contain.
pub fn opens_like_refusal(source: &str, output: &str) -> bool {
    opens_with_unexplained(&output.to_lowercase(), &source.to_lowercase(), &REFUSAL_RULES)
}

/// Whether `output` opens like a chat reply ("Sure", "Of course", "Here is", "好的", "以下是") that
/// the source does not itself contain.
pub fn opens_like_answer(source: &str, output: &str) -> bool {
    opens_with_unexplained(&output.to_lowercase(), &source.to_lowercase(), &ANSWER_RULES)
}

fn opens_with_unexplained(output_lower: &str, source_lower: &str, rules: &[Rule]) -> bool {
    let opening = output_lower.trim_start_matches(|c: char| !c.is_alphanumeric());
    rules.iter().any(|(opener, excuses)| {
        starts_with_word(opening, opener) && !excuses.iter().any(|w| source_lower.contains(w))
    })
}

/// Whether `text` starts with `opener` and the next character does not
/// continue a Latin word ("sure" matches "Sure, ..." but not "surely").
fn starts_with_word(text: &str, opener: &str) -> bool {
    match text.strip_prefix(opener) {
        None => false,
        Some(rest) => {
            let ends_in_letter = opener
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphabetic());
            !(ends_in_letter && rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
        }
    }
}

/// Whether a source that is one question (it ends in a question mark and has no other sentence
/// end) came back as a statement: the finished `output` ends like a statement and has no question
/// mark anywhere ("2+2等于几？只用一个数字回答。" is a translation of a question and a request).
pub fn answers_a_question(source: &str, output: &str) -> bool {
    let ends = |s: &str, marks: &[char]| s.trim_end().ends_with(marks);
    let body = source.trim_end().trim_end_matches(['?', '？']);
    let single = !body.contains(['?', '？', '.', '。', '!', '！']);
    single
        && ends(source, &['?', '？'])
        && ends(output, &['.', '。', '!', '！'])
        && !output.contains(['?', '？'])
}

/// Whether a later line of `output` starts with a note or comment label.
pub fn has_commentary_line(output: &str) -> bool {
    output
        .to_lowercase()
        .lines()
        .skip(1)
        .map(str::trim_start)
        .any(|line| NOTE_LABELS.iter().any(|label| line.starts_with(label)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chat_opener_is_an_answer_unless_the_source_has_it() {
        assert!(opens_like_answer("你叫什么名字？", "Sure! My name is Gemma."));
        assert!(opens_like_answer("What is your name?", "好的，我叫小明。"));
        assert!(!opens_like_answer("好的，没问题。", "Sure, no problem."));
        assert!(!opens_like_answer("Of course.", "当然。"));
        assert!(!opens_like_answer("当然可以。", "Surely, you can."));
    }

    #[test]
    fn a_translation_of_an_acknowledgement_is_not_an_answer() {
        for (source, output) in [
            ("知道了。", "Okay, got it."),
            ("我明白了。", "Okay, I understand."),
            ("收到。", "Okay, received."),
            ("没关系。", "Okay, it's fine."),
            ("对，没错。", "Sure, that's right."),
        ] {
            assert!(!opens_like_answer(source, output), "{source}");
        }
    }

    #[test]
    fn a_refusal_is_one_unless_the_speaker_said_it() {
        assert!(opens_like_refusal("你能帮我订机票吗？", "I'm sorry, but I can't help with that."));
        assert!(opens_like_refusal("Book me a flight.", "抱歉，我无法预订机票。"));
        assert!(!opens_like_refusal("对不起，我迟到了。", "I'm sorry, I'm late."));
        assert!(!opens_like_refusal("我不能去。", "I can't go."));
    }

    #[test]
    fn a_single_question_answered_as_a_statement_is_caught_and_a_mixed_one_is_not() {
        assert!(answers_a_question("你几点到？", "I will arrive at six."));
        assert!(answers_a_question("When do you arrive?", "我六点到。"));
        assert!(!answers_a_question("When do you arrive?", "你几点到？"));
        assert!(!answers_a_question("你几点到", "I will arrive at six."), "no mark on the source");
        assert!(!answers_a_question("好的。你几点到？", "Okay. I will arrive at six."), "two sentences");
        assert!(!answers_a_question(
            "Answer with one number only, what is two plus two?",
            "2+2等于几？只用一个数字回答。"
        ));
    }

    #[test]
    fn a_note_line_after_the_text_is_commentary() {
        assert!(has_commentary_line("Hello.\n\nNote: this is informal."));
        assert!(has_commentary_line("你好。\n（注：非正式）"));
        assert!(!has_commentary_line("Note: that was the first line"));
    }
}
