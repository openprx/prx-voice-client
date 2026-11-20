//! Cheap checks that tell a translation from something else (PD-2.6.3).
//!
//! These functions only detect. They never change the text; the pipeline
//! records the flags as `mt_suspect` and may fall back visibly. All checks are
//! heuristics on the finished output, deliberately simple:
//!
//! * `Empty`: no non-space character.
//! * `ThinkResidue`: a thinking tag is in the text.
//! * `Truncated`: the server reported `finish_reason: length`.
//! * `TooLong`: the estimated token count of the output exceeds
//!   `3 x (source tokens) + 16`, the same bound as the request's `max_tokens`.
//! * `WrongLanguage`: with at least 4 letters, the share of Chinese characters
//!   among Chinese and Latin letters is below 0.3 for a Chinese target, or
//!   above 0.2 for an English target.
//! * `Refusal`: the output opens like an apology or an AI self-reference
//!   ("I'm sorry", "I cannot", "as an AI", "抱歉", "我无法") and the source does
//!   not itself contain the matching words (a speaker may really say sorry).
//! * `AnswerLike`: the output opens like a chat reply ("Sure", "Of course",
//!   "Here is", "好的", "当然", "以下是") without the source containing the
//!   matching word; or a source that ends in a question mark is translated
//!   into a text that ends like a statement; or a later line starts with a
//!   note or comment label.

use prxvc_media::Lang;

use super::report::Suspect;

/// Everything `assess` looks at.
#[derive(Clone, Copy, Debug)]
pub struct AssessInput<'a> {
    /// The source clause.
    pub source: &'a str,
    /// Language of the source.
    pub source_lang: Lang,
    /// Language the output should be in.
    pub target_lang: Lang,
    /// The cleaned output.
    pub output: &'a str,
    /// The server's `finish_reason`, when it sent one.
    pub finish_reason: Option<&'a str>,
}

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
        &["好的", "当然", "没问题", "行", "可以", "确定", "sure"],
    ),
    ("certainly", &["当然", "肯定", "一定", "certainly"]),
    ("of course", &["当然", "自然", "of course"]),
    (
        "absolutely",
        &["当然", "完全", "绝对", "没错", "absolutely"],
    ),
    ("okay", &["好", "行", "可以", "嗯", "ok"]),
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

/// Rough token count: one per Chinese character, one per digit and
/// punctuation mark, one per three letters. Overestimates a little, which is
/// what a cap wants.
pub fn estimate_tokens(text: &str) -> u32 {
    let (mut cjk, mut letters, mut other) = (0u32, 0u32, 0u32);
    for c in text.chars() {
        if is_cjk(c) {
            cjk += 1;
        } else if c.is_alphabetic() {
            letters += 1;
        } else if !c.is_whitespace() {
            other += 1;
        }
    }
    cjk + letters.div_ceil(3) + other
}

/// The most tokens a translation of `source` may plausibly have:
/// `3 x source tokens + 16` (PD-2.6.3).
pub fn max_output_tokens(source: &str) -> u32 {
    estimate_tokens(source).saturating_mul(3).saturating_add(16)
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
}

/// The flags that apply to `input`, in a fixed order; empty when the output
/// looks like a translation.
pub fn assess(input: &AssessInput<'_>) -> Vec<Suspect> {
    let output = input.output.trim();
    if output.is_empty() {
        return vec![Suspect::Empty];
    }
    let mut out = Vec::new();
    let lower = output.to_lowercase();
    if lower.contains("<think") || lower.contains("</think") || lower.contains("<channel|>") {
        out.push(Suspect::ThinkResidue);
    }
    if input.finish_reason == Some("length") {
        out.push(Suspect::Truncated);
    }
    if estimate_tokens(output) > max_output_tokens(input.source) {
        out.push(Suspect::TooLong);
    }
    if wrong_language(output, input.target_lang) {
        out.push(Suspect::WrongLanguage);
    }
    let source_lower = input.source.to_lowercase();
    if opens_with_unexplained(&lower, &source_lower, &REFUSAL_RULES) {
        out.push(Suspect::Refusal);
    }
    if opens_with_unexplained(&lower, &source_lower, &ANSWER_RULES)
        || statement_for_question(input.source, output)
        || has_commentary_line(&lower)
    {
        out.push(Suspect::AnswerLike);
    }
    out
}

fn wrong_language(output: &str, target: Lang) -> bool {
    let (mut cjk, mut latin) = (0usize, 0usize);
    for c in output.chars() {
        if is_cjk(c) {
            cjk += 1;
        } else if c.is_alphabetic() {
            latin += 1;
        }
    }
    let letters = cjk + latin;
    if letters < 4 {
        return false;
    }
    #[allow(clippy::cast_precision_loss, reason = "character counts of one clause")]
    let cjk_share = cjk as f32 / letters as f32;
    match target {
        Lang::Zh => cjk_share < 0.3,
        Lang::En => cjk_share > 0.2,
    }
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

fn statement_for_question(source: &str, output: &str) -> bool {
    let ends = |s: &str, marks: &[char]| s.trim_end().ends_with(marks);
    ends(source, &['?', '？']) && ends(output, &['.', '。', '!', '！'])
}

fn has_commentary_line(output_lower: &str) -> bool {
    output_lower
        .lines()
        .skip(1)
        .map(str::trim_start)
        .any(|line| NOTE_LABELS.iter().any(|label| line.starts_with(label)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str, from: Lang, to: Lang, output: &str) -> Vec<Suspect> {
        assess(&AssessInput {
            source,
            source_lang: from,
            target_lang: to,
            output,
            finish_reason: Some("stop"),
        })
    }

    fn zh_en(source: &str, output: &str) -> Vec<Suspect> {
        check(source, Lang::Zh, Lang::En, output)
    }

    fn en_zh(source: &str, output: &str) -> Vec<Suspect> {
        check(source, Lang::En, Lang::Zh, output)
    }

    #[test]
    fn good_translations_raise_no_flag() {
        assert!(
            zh_en(
                "我们三点半在机场见面。",
                "We will meet at the airport at 3:30."
            )
            .is_empty()
        );
        assert!(en_zh("I did not say that.", "我没有那么说。").is_empty());
        assert!(zh_en("你几点到？", "What time are you arriving?").is_empty());
        assert!(
            en_zh(
                "Call Dr. Smith at 555-0134.",
                "给史密斯医生打电话，555-0134。"
            )
            .is_empty()
        );
        // Names and acronyms inside a Chinese sentence are fine.
        assert!(en_zh("The NASA report is ready.", "NASA 的报告已经准备好了。").is_empty());
    }

    #[test]
    fn empty_output_is_flagged() {
        assert_eq!(zh_en("你好", ""), [Suspect::Empty]);
        assert_eq!(zh_en("你好", "  \n"), [Suspect::Empty]);
    }

    #[test]
    fn the_wrong_language_is_flagged() {
        // The source echoed back instead of translated.
        assert_eq!(
            zh_en("我们三点见面。", "我们三点见面。"),
            [Suspect::WrongLanguage]
        );
        assert_eq!(
            en_zh("See you at three.", "See you at three."),
            [Suspect::WrongLanguage]
        );
        // Mixed output that is mostly the wrong script.
        assert!(!zh_en("好", "好 That is fine with me").contains(&Suspect::WrongLanguage));
        assert!(zh_en("好吧", "好吧，我们走吧。Let us go").contains(&Suspect::WrongLanguage));
    }

    #[test]
    fn an_output_much_longer_than_the_input_is_flagged() {
        let long = "word ".repeat(80);
        assert!(zh_en("好的。", &long).contains(&Suspect::TooLong));
        let ok = "Okay, that works for me.";
        assert!(!zh_en("好的，没问题。", ok).contains(&Suspect::TooLong));
    }

    #[test]
    fn a_length_stop_is_flagged_as_truncated() {
        let flags = assess(&AssessInput {
            source: "你好",
            source_lang: Lang::Zh,
            target_lang: Lang::En,
            output: "Hello",
            finish_reason: Some("length"),
        });
        assert_eq!(flags, [Suspect::Truncated]);
    }

    #[test]
    fn a_thinking_tag_left_in_the_text_is_flagged() {
        assert!(zh_en("你好", "Hello </think> there").contains(&Suspect::ThinkResidue));
    }

    #[test]
    fn refusals_are_flagged_unless_the_speaker_said_it() {
        assert!(
            zh_en(
                "你能帮我订机票吗？",
                "I'm sorry, but I can't help with that?"
            )
            .contains(&Suspect::Refusal)
        );
        assert!(en_zh("Book me a flight.", "抱歉，我无法预订机票。").contains(&Suspect::Refusal));
        assert!(en_zh("Book me a flight.", "作为一个AI，我不能订票。").contains(&Suspect::Refusal));
        // The speaker really apologises.
        assert!(!zh_en("对不起，我迟到了。", "I'm sorry, I'm late.").contains(&Suspect::Refusal));
        assert!(!en_zh("Sorry, I am late.", "抱歉，我迟到了。").contains(&Suspect::Refusal));
        assert!(!zh_en("我不能去。", "I can't go.").contains(&Suspect::Refusal));
    }

    #[test]
    fn answers_and_chat_openers_are_flagged_unless_the_source_has_them() {
        assert!(zh_en("你叫什么名字？", "Sure! My name is Gemma.").contains(&Suspect::AnswerLike));
        assert!(en_zh("What is your name?", "好的，我叫小明。").contains(&Suspect::AnswerLike));
        assert!(
            en_zh("Translate this please.", "以下是翻译：请翻译这个。")
                .contains(&Suspect::AnswerLike)
        );
        assert!(!zh_en("好的，没问题。", "Sure, no problem.").contains(&Suspect::AnswerLike));
        assert!(!en_zh("Of course.", "当然。").contains(&Suspect::AnswerLike));
        assert!(!zh_en("当然可以。", "Surely, you can.").contains(&Suspect::AnswerLike));
    }

    #[test]
    fn a_question_turned_into_a_statement_is_flagged() {
        assert!(zh_en("你几点到？", "I will arrive at six.").contains(&Suspect::AnswerLike));
        assert!(en_zh("When do you arrive?", "我六点到。").contains(&Suspect::AnswerLike));
        assert!(!en_zh("When do you arrive?", "你几点到？").contains(&Suspect::AnswerLike));
        // No mark on the source: nothing to compare.
        assert!(!zh_en("你几点到", "I will arrive at six.").contains(&Suspect::AnswerLike));
    }

    #[test]
    fn a_commentary_line_after_the_translation_is_flagged() {
        assert!(zh_en("你好", "Hello.\n\nNote: this is informal.").contains(&Suspect::AnswerLike));
        assert!(en_zh("Hello", "你好。\n（注：非正式）").contains(&Suspect::AnswerLike));
    }

    #[test]
    fn the_token_estimate_counts_digits_and_characters() {
        assert_eq!(estimate_tokens("你好"), 2);
        assert_eq!(estimate_tokens("3.5"), 3);
        assert_eq!(estimate_tokens("hello"), 2);
        assert_eq!(max_output_tokens("你好"), 22);
    }
}
