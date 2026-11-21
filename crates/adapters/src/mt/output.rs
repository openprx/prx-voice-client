//! Structural clean-up of the model's output: "translation only" (PD-2.6.3).
//!
//! The filter sits between the token stream and the caller. It removes only
//! wrappers that carry no translated content: a leading thinking block, a
//! leading `Translation:` style label, and one pair of quotes around the whole
//! output. Everything else passes through unchanged; whether the remaining
//! text is a translation at all is decided by `assess`, which only reports.
//!
//! The filter is incremental. It never emits text it might have to take back:
//! while the start of the output could still turn out to be a wrapper it holds
//! the text, and trailing whitespace is held until more text follows.

use super::report::Stripped;

/// Opening and closing tags of thinking blocks.
const THINK_TAGS: [(&str, &str); 3] = [
    ("<think>", "</think>"),
    ("<thinking>", "</thinking>"),
    ("<|channel>thought", "<channel|>"),
];

/// Labels some models put before the translation.
const PREAMBLES: [&str; 14] = [
    "translation:",
    "translation：",
    "translated text:",
    "translated:",
    "here is the translation:",
    "here's the translation:",
    "the translation is:",
    "sure, here is the translation:",
    "sure! here's the translation:",
    "译文：",
    "译文:",
    "翻译：",
    "翻译结果：",
    "以下是翻译：",
];

/// Pairs of quotes that may wrap the whole output.
const QUOTE_PAIRS: [(char, char); 5] = [
    ('"', '"'),
    ('\u{201C}', '\u{201D}'),
    ('\u{2018}', '\u{2019}'),
    ('\u{300C}', '\u{300D}'),
    ('\u{300E}', '\u{300F}'),
];

enum Prefix {
    /// `pattern` is a prefix of the text; the number of bytes it covers.
    Full(usize),
    /// The text is a proper prefix of `pattern`: undecided.
    Partial,
    /// Neither.
    No,
}

/// Compare `text` with `pattern`, ignoring ASCII case.
fn prefix_match(text: &str, pattern: &str) -> Prefix {
    let mut t = text.char_indices();
    for pc in pattern.chars() {
        match t.next() {
            None => return Prefix::Partial,
            Some((_, tc)) if tc.eq_ignore_ascii_case(&pc) => {}
            Some(_) => return Prefix::No,
        }
    }
    Prefix::Full(t.next().map_or(text.len(), |(i, _)| i))
}

/// The best match among `patterns`: a full match wins over an undecided one.
fn best_prefix<'a>(text: &str, patterns: impl Iterator<Item = &'a str>) -> Prefix {
    let mut partial = false;
    for p in patterns {
        match prefix_match(text, p) {
            Prefix::Full(n) => return Prefix::Full(n),
            Prefix::Partial => partial = true,
            Prefix::No => {}
        }
    }
    if partial { Prefix::Partial } else { Prefix::No }
}

enum View {
    /// Nothing can be emitted yet.
    Pending,
    /// The cleaned text so far.
    Text(String),
}

/// The cleaned view of everything received so far. `done` says the stream has
/// ended, which settles every undecided case. A non-final view is always a
/// prefix of every later view.
fn view(raw: &str, done: bool, thinking_only: bool, stripped: &mut Vec<Stripped>) -> View {
    let mut s = raw.trim_start();
    // 1. Leading thinking blocks.
    loop {
        match best_prefix(s, THINK_TAGS.iter().map(|(open, _)| *open)) {
            Prefix::Full(len) => {
                let open_len = len;
                let closer = THINK_TAGS
                    .iter()
                    .find(|(open, _)| matches!(prefix_match(s, open), Prefix::Full(_)))
                    .map_or("", |(_, close)| *close);
                match s[open_len..].find(closer) {
                    Some(at) => {
                        s = s[open_len + at + closer.len()..].trim_start();
                        push_once(stripped, Stripped::Thinking);
                    }
                    // Still thinking, or cut off inside the block: no text.
                    None if done => return View::Text(String::new()),
                    None => return View::Pending,
                }
            }
            Prefix::Partial if !done => return View::Pending,
            _ => break,
        }
    }
    if thinking_only {
        return View::Text(s.trim_end().to_owned());
    }
    // 2. A leading label.
    match best_prefix(s, PREAMBLES.iter().copied()) {
        Prefix::Full(len) => {
            s = s[len..].trim_start();
            push_once(stripped, Stripped::Preamble);
        }
        Prefix::Partial if !done => return View::Pending,
        _ => {}
    }
    // 3. One pair of quotes around the whole output.
    if let Some(first) = s.chars().next()
        && let Some(&(open, close)) = QUOTE_PAIRS.iter().find(|(o, _)| *o == first)
    {
        let body = &s[first.len_utf8()..];
        let body_end = body.trim_end();
        if let Some(inner) = body_end.strip_suffix(close)
            && !inner.contains(open)
            && !inner.contains(close)
        {
            // A candidate wrapper; more text could still follow the closing quote.
            if !done {
                return View::Pending;
            }
            push_once(stripped, Stripped::QuoteWrapper);
            return View::Text(inner.trim().to_owned());
        }
        if !done && !has_text_after_a_closer(body, close) {
            return View::Pending;
        }
    }
    View::Text(s.trim_end().to_owned())
}

/// Whether `body` has a `close` character that is followed by more text.
fn has_text_after_a_closer(body: &str, close: char) -> bool {
    body.char_indices()
        .any(|(i, c)| c == close && !body[i + c.len_utf8()..].trim().is_empty())
}

fn push_once(list: &mut Vec<Stripped>, item: Stripped) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// The incremental output filter of one request.
#[derive(Debug, Default)]
pub struct OutputFilter {
    /// Strip leading thinking blocks only (a dialogue reply may legitimately start with a label or a quote).
    thinking_only: bool,
    raw: String,
    emitted: String,
    stripped: Vec<Stripped>,
}

/// What `OutputFilter::finish` returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterEnd {
    /// The last piece of cleaned text, possibly empty.
    pub tail: String,
    /// The whole cleaned output (everything emitted, including `tail`).
    pub text: String,
    /// The wrappers that were removed.
    pub stripped: Vec<Stripped>,
}

impl OutputFilter {
    /// A filter at the start of an output.
    pub fn new() -> Self {
        Self::default()
    }

    /// A filter that removes leading thinking blocks and nothing else.
    pub fn thinking_only() -> Self {
        Self {
            thinking_only: true,
            ..Self::default()
        }
    }

    /// Feed the next raw delta; the result is the cleaned text to pass on
    /// (possibly empty).
    pub fn push(&mut self, delta: &str) -> String {
        self.raw.push_str(delta);
        let mut stripped = Vec::new();
        let out = match view(&self.raw, false, self.thinking_only, &mut stripped) {
            View::Pending => String::new(),
            View::Text(t) => self.advance(&t),
        };
        self.stripped = stripped;
        out
    }

    /// The stream ended: settle every undecided case and return the rest.
    pub fn finish(&mut self) -> FilterEnd {
        let mut stripped = Vec::new();
        let tail = match view(&self.raw, true, self.thinking_only, &mut stripped) {
            View::Pending => String::new(),
            View::Text(t) => self.advance(&t),
        };
        self.stripped = stripped;
        FilterEnd {
            tail,
            text: self.emitted.clone(),
            stripped: self.stripped.clone(),
        }
    }

    /// The part of `cleaned` that has not been emitted yet. Views are
    /// prefix-stable by construction; a view that is not an extension of what
    /// was emitted emits nothing rather than repeat text.
    fn advance(&mut self, cleaned: &str) -> String {
        match cleaned.strip_prefix(self.emitted.as_str()) {
            Some(new) => {
                self.emitted.push_str(new);
                new.to_owned()
            }
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the filter over `deltas`; returns the concatenated emissions and the end.
    fn run(deltas: &[&str]) -> (String, FilterEnd) {
        let mut f = OutputFilter::new();
        let mut out = String::new();
        for d in deltas {
            out.push_str(&f.push(d));
        }
        let end = f.finish();
        out.push_str(&end.tail);
        (out, end)
    }

    /// The same text cut into single characters.
    fn run_chars(text: &str) -> (String, FilterEnd) {
        let parts: Vec<String> = text.chars().map(String::from).collect();
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        run(&refs)
    }

    #[test]
    fn a_thinking_only_filter_keeps_labels_and_quotes() {
        let mut f = OutputFilter::thinking_only();
        let mut out = f.push("<think>hm</think>");
        out.push_str(&f.push("\"Translation: yes,\" she said."));
        let end = f.finish();
        out.push_str(&end.tail);
        assert_eq!(out, "\"Translation: yes,\" she said.");
        assert_eq!(end.stripped, vec![Stripped::Thinking]);
    }

    #[test]
    fn plain_text_passes_through_and_is_emitted_before_the_end() {
        let mut f = OutputFilter::new();
        assert_eq!(f.push("Hello"), "Hello");
        assert_eq!(f.push(" there. "), " there.");
        let end = f.finish();
        assert_eq!(end.tail, "");
        assert_eq!(end.text, "Hello there.");
        assert!(end.stripped.is_empty());
    }

    #[test]
    fn a_label_is_removed_however_the_tokens_are_cut() {
        for text in [
            "Translation: See you at 3.",
            "translation：See you at 3.",
            "Here's the translation:\nSee you at 3.",
        ] {
            let (out, end) = run_chars(text);
            assert_eq!(out, "See you at 3.", "{text}");
            assert_eq!(end.stripped, [Stripped::Preamble]);
            assert_eq!(run(&[text]).0, "See you at 3.");
        }
        let (out, end) = run_chars("译文：三点见。");
        assert_eq!(out, "三点见。");
        assert_eq!(end.stripped, [Stripped::Preamble]);
    }

    #[test]
    fn text_that_starts_like_a_label_but_is_not_one_is_kept() {
        let (out, end) = run_chars("Translators are people.");
        assert_eq!(out, "Translators are people.");
        assert!(end.stripped.is_empty());
        let (out, _) = run_chars("Tra");
        assert_eq!(out, "Tra", "an undecided start is released at the end");
    }

    #[test]
    fn a_pair_of_quotes_around_everything_is_removed() {
        for (text, inner) in [
            ("\"We are late.\"", "We are late."),
            ("\u{201C}We are late.\u{201D}", "We are late."),
            ("「我们迟到了。」", "我们迟到了。"),
            ("  \"We are late.\"\n", "We are late."),
        ] {
            let (out, end) = run_chars(text);
            assert_eq!(out, inner);
            assert_eq!(end.stripped, [Stripped::QuoteWrapper]);
        }
    }

    #[test]
    fn quotes_that_are_part_of_the_translation_are_kept() {
        for text in [
            "\"Fine,\" he said.",
            "\"One\" and \"two\".",
            "He said \"fine\".",
            "\"Unclosed",
        ] {
            let (out, end) = run_chars(text);
            assert_eq!(out, text);
            assert!(end.stripped.is_empty(), "{text}");
        }
    }

    #[test]
    fn text_after_a_quoted_start_is_held_only_while_undecided() {
        let mut f = OutputFilter::new();
        assert_eq!(f.push("\"Fine,\""), "", "could still be a wrapper");
        assert_eq!(f.push(" he said."), "\"Fine,\" he said.");
    }

    #[test]
    fn thinking_blocks_are_dropped() {
        let (out, end) = run_chars("<think>\nthe user wants...\n</think>\n\nSee you.");
        assert_eq!(out, "See you.");
        assert_eq!(end.stripped, [Stripped::Thinking]);
        let (out, _) = run(&["<|channel>thought\nhmm<channel|>OK."]);
        assert_eq!(out, "OK.");
        let (out, end) = run(&["<think>never closed"]);
        assert_eq!(out, "");
        assert_eq!(end.text, "");
    }

    #[test]
    fn a_thinking_block_a_label_and_quotes_can_stack() {
        let (out, end) = run_chars("<think>x</think>Translation: \"Yes.\"");
        assert_eq!(out, "Yes.");
        assert_eq!(
            end.stripped,
            [
                Stripped::Thinking,
                Stripped::Preamble,
                Stripped::QuoteWrapper
            ]
        );
    }

    #[test]
    fn emissions_never_repeat_or_lose_text_for_any_split_point() {
        let text = "Translation: \"Call me at 10:30, please.\"";
        for cut in 0..text.len() {
            if !text.is_char_boundary(cut) {
                continue;
            }
            let (a, b) = text.split_at(cut);
            let (out, _) = run(&[a, b]);
            assert_eq!(out, "Call me at 10:30, please.", "cut at {cut}");
        }
    }
}
