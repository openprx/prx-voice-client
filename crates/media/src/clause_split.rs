//! Cuts a stream of translated text into clauses for synthesis (pipeline-design 3.2.3).
//!
//! A clause is emitted as soon as a sentence-final mark (`。！？；.!?;…`) or a
//! clause-level mark (`，、：,:`) arrives and the text before it is long
//! enough. Length is counted in units: Chinese characters plus words. The
//! minimums follow the recognition commit rule (`prxvc-session`
//! `commit_policy`: 4 Chinese characters or 2 English words) for sentence
//! marks and are larger for clause marks (6 and 4, the values the
//! `bench/e2e` prototype settled on [P]), so that commas do not produce
//! fragments too short to synthesise well.
//!
//! The splitter never cuts inside a number (`3.5`, `1,000`, `10:30`), after a
//! known abbreviation (`Dr.`, `e.g.`), after an initial (`J. Smith`), inside a
//! dotted abbreviation (`U.S.`, `a.m.`), inside a domain name, or before a
//! lowercase continuation (`approx. five`). A mark whose meaning depends on
//! the next character is held until that character arrives.
//!
//! Placement: this module is pure and depends on nothing but `Lang`, so it
//! lives in `prxvc-media` (L0): the pipeline in `prxvc-session` (L2) and the
//! translation adapters in `prxvc-adapters` (L3) both may use it, and
//! `prxvc-session::decide_tts_commit` is built on [`first_clause_end`], so there
//! is exactly one clause rule.

use crate::lang::Lang;

/// Marks that end a sentence.
const SENTENCE_MARKS: [char; 8] = ['。', '！', '？', '；', '!', '?', ';', '…'];
/// Marks that end a clause inside a sentence.
const CLAUSE_MARKS: [char; 5] = ['，', '、', '：', ',', ':'];
/// Characters that may close a quotation or bracket after a mark.
const CLOSERS: [char; 8] = ['"', '\'', '\u{201D}', '\u{2019}', ')', '）', '」', '』'];

/// Abbreviations (lowercase, without the final dot) after which a dot does not end a sentence.
const ABBREVIATIONS: [&str; 33] = [
    "dr", "mr", "mrs", "ms", "prof", "sr", "jr", "st", "vs", "etc", "inc", "ltd", "co", "corp",
    "mt", "ave", "dept", "approx", "est", "fig", "jan", "feb", "mar", "apr", "jun", "jul", "aug",
    "sep", "sept", "oct", "nov", "dec", "gen",
];

/// Minimum clause lengths, in units (Chinese characters plus words).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitParams {
    /// Fewest units before a sentence-final mark.
    pub min_before_sentence_mark: usize,
    /// Fewest units before a clause-level mark.
    pub min_before_clause_mark: usize,
}

impl SplitParams {
    /// The defaults for text in `lang`.
    pub fn for_lang(lang: Lang) -> Self {
        match lang {
            Lang::Zh => Self {
                min_before_sentence_mark: 4,
                min_before_clause_mark: 6,
            },
            Lang::En => Self {
                min_before_sentence_mark: 2,
                min_before_clause_mark: 4,
            },
        }
    }
}

/// Incremental splitter of one translation stream.
#[derive(Debug)]
pub struct ClauseSplitter {
    params: SplitParams,
    buf: String,
    /// A clause was just emitted and nothing but stray marks has arrived since.
    after_cut: bool,
}

/// What the scan of the buffer found.
enum Scan {
    /// Cut after this byte offset.
    Cut(usize),
    /// A mark whose meaning needs the next character.
    Wait,
    /// No boundary in the buffer.
    None,
}

impl ClauseSplitter {
    /// A splitter for text in `lang` with its default minimums.
    pub fn new(lang: Lang) -> Self {
        Self::with_params(SplitParams::for_lang(lang))
    }

    /// A splitter with explicit minimums.
    pub fn with_params(params: SplitParams) -> Self {
        Self {
            params,
            buf: String::new(),
            after_cut: false,
        }
    }

    /// Add text; returns the clauses completed by it, in order.
    pub fn push(&mut self, text: &str) -> Vec<String> {
        self.buf.push_str(text);
        self.drop_stray_start();
        self.cut_all(false)
    }

    /// The stream ended: returns the clauses that were still held, including
    /// the unfinished last one.
    pub fn finish(&mut self) -> Vec<String> {
        let mut out = self.cut_all(true);
        let rest = std::mem::take(&mut self.buf);
        let rest = rest.trim();
        if has_content(rest) {
            out.push(rest.to_owned());
        }
        out
    }

    fn cut_all(&mut self, done: bool) -> Vec<String> {
        let mut out = Vec::new();
        while let Scan::Cut(end) = self.scan(done) {
            let head: String = self.buf.drain(..end).collect();
            let head = head.trim();
            if !head.is_empty() {
                out.push(head.to_owned());
            }
            self.after_cut = true;
            self.drop_stray_start();
        }
        out
    }

    /// Marks and closing quotes that arrive after a cut belong to the clause
    /// just emitted and are silent: drop them, however late they arrive.
    fn drop_stray_start(&mut self) {
        if !self.after_cut {
            return;
        }
        let keep = trim_stray_start(&self.buf).len();
        let skip = self.buf.len() - keep;
        self.buf.drain(..skip);
        if !self.buf.is_empty() {
            self.after_cut = false;
        }
    }

    fn scan(&self, done: bool) -> Scan {
        let chars: Vec<(usize, char)> = self.buf.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            let (at, c) = chars[i];
            let is_dot = c == '.';
            let is_sentence = SENTENCE_MARKS.contains(&c) || is_dot;
            let is_clause = CLAUSE_MARKS.contains(&c);
            if !is_sentence && !is_clause {
                i += 1;
                continue;
            }
            // The run of identical dots is one mark ("...").
            let mut last = i;
            if is_dot {
                while last + 1 < chars.len() && chars[last + 1].1 == '.' {
                    last += 1;
                }
            }
            let next = chars.get(last + 1).map(|&(_, n)| n);
            let prev = if i > 0 { Some(chars[i - 1].1) } else { None };
            // A mark between digits is part of a number.
            let digit_before = prev.is_some_and(|p| p.is_ascii_digit());
            let needs_next = is_dot || (digit_before && matches!(c, ',' | ':'));
            if needs_next && next.is_none() && !done {
                return Scan::Wait;
            }
            if needs_next && is_inside_token(c, digit_before, next) {
                i = last + 1;
                continue;
            }
            if is_dot && !self.dot_ends_sentence(&chars, i, last, done) {
                if let Scan::Wait = self.dot_lookahead(&chars, last, done) {
                    return Scan::Wait;
                }
                i = last + 1;
                continue;
            }
            // Take the closing quotes and brackets that follow the mark.
            let mut end_idx = last + 1;
            while end_idx < chars.len() && CLOSERS.contains(&chars[end_idx].1) {
                end_idx += 1;
            }
            let end = chars.get(end_idx).map_or(self.buf.len(), |&(b, _)| b);
            let need = if is_clause {
                self.params.min_before_clause_mark
            } else {
                self.params.min_before_sentence_mark
            };
            if count_units(&self.buf[..at]) >= need {
                return Scan::Cut(end);
            }
            i = end_idx.max(last + 1);
        }
        Scan::None
    }

    /// Whether the dot run `chars[first..=last]` ends a sentence, judged by the
    /// word before it and the text after it. Returns `false` for a dot that
    /// belongs to an abbreviation, an initial or a continuation; the caller
    /// treats an undecided lookahead (end of buffer after spaces) through
    /// `dot_lookahead`.
    fn dot_ends_sentence(
        &self,
        chars: &[(usize, char)],
        first: usize,
        last: usize,
        done: bool,
    ) -> bool {
        if first == last && is_abbreviation_before(chars, first) {
            return false;
        }
        // A lowercase letter after the spaces continues the sentence.
        match next_non_space(chars, last + 1) {
            Some(n) => !n.is_lowercase(),
            None => {
                // Only spaces (or nothing) follow: end of text decides.
                done || chars.len() == last + 1
            }
        }
    }

    /// For a dot that was rejected only because nothing follows its spaces
    /// yet: wait for the next character when the stream is still open.
    fn dot_lookahead(&self, chars: &[(usize, char)], last: usize, done: bool) -> Scan {
        if !done && next_non_space(chars, last + 1).is_none() {
            Scan::Wait
        } else {
            Scan::None
        }
    }
}

fn next_non_space(chars: &[(usize, char)], from: usize) -> Option<char> {
    chars
        .get(from..)?
        .iter()
        .map(|&(_, c)| c)
        .find(|c| !c.is_whitespace())
}

/// Whether the mark `c` sits inside a token: a number (`3.5`, `1,000`,
/// `10:30`) or a dotted name (`example.com`, `U.S`).
fn is_inside_token(c: char, digit_before: bool, next: Option<char>) -> bool {
    let Some(n) = next else {
        return false;
    };
    match c {
        '.' => n.is_alphanumeric(),
        ',' | ':' => digit_before && n.is_ascii_digit(),
        _ => false,
    }
}

/// Whether the word that ends just before the dot at `chars[dot]` is an
/// abbreviation, an initial or a dotted abbreviation.
fn is_abbreviation_before(chars: &[(usize, char)], dot: usize) -> bool {
    let mut start = dot;
    while start > 0 {
        let c = chars[start - 1].1;
        if c.is_alphabetic() || c == '.' {
            start -= 1;
        } else {
            break;
        }
    }
    let token: String = chars[start..dot].iter().map(|&(_, c)| c).collect();
    if token.is_empty() {
        return false;
    }
    if token.contains('.') {
        return true; // U.S, e.g, a.m
    }
    let mut it = token.chars();
    if let (Some(only), None) = (it.next(), it.next()) {
        // A single capital is an initial; "I" and "A" are words.
        return only.is_uppercase() && only != 'I' && only != 'A';
    }
    ABBREVIATIONS.contains(&token.to_lowercase().as_str())
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
}

/// Chinese characters plus words.
fn count_units(text: &str) -> usize {
    let mut units = 0;
    let mut in_word = false;
    for c in text.chars() {
        if is_cjk(c) {
            units += 1;
            in_word = false;
        } else if c.is_alphanumeric() {
            if !in_word {
                units += 1;
            }
            in_word = true;
        } else {
            in_word = false;
        }
    }
    units
}

/// The byte offset just after the first clause of `text`, when `text` already
/// holds a complete one (the stream is still open: a mark that needs the next
/// character to be judged does not count yet).
///
/// This is the rule of [`ClauseSplitter`] applied to a string, for callers
/// that keep their own buffer.
pub fn first_clause_end(lang: Lang, text: &str) -> Option<usize> {
    let splitter = ClauseSplitter {
        params: SplitParams::for_lang(lang),
        buf: text.to_owned(),
        after_cut: false,
    };
    match splitter.scan(false) {
        Scan::Cut(end) => Some(end),
        Scan::Wait | Scan::None => None,
    }
}

/// `text` without the whitespace, closing quotes and marks that trail a clause
/// just cut (they belong to that clause and are silent).
pub fn trim_stray_start(text: &str) -> &str {
    text.trim_start_matches(|c: char| {
        c.is_whitespace() || CLOSERS.contains(&c) || SENTENCE_MARKS.contains(&c) || c == '.'
    })
}

fn has_content(text: &str) -> bool {
    text.chars().any(|c| c.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All clauses of `text` fed in one piece.
    fn split_whole(lang: Lang, text: &str) -> Vec<String> {
        let mut s = ClauseSplitter::new(lang);
        let mut out = s.push(text);
        out.extend(s.finish());
        out
    }

    /// All clauses of `text` fed in the given pieces.
    fn split_pieces<'a>(lang: Lang, pieces: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut s = ClauseSplitter::new(lang);
        let mut out = Vec::new();
        for p in pieces {
            out.extend(s.push(p));
        }
        out.extend(s.finish());
        out
    }

    /// Whole-text result equals the result for every two-way cut and for one
    /// character at a time.
    fn assert_boundary_independent(lang: Lang, text: &str) -> Vec<String> {
        let whole = split_whole(lang, text);
        for cut in (0..=text.len()).filter(|c| text.is_char_boundary(*c)) {
            let (a, b) = text.split_at(cut);
            assert_eq!(split_pieces(lang, [a, b]), whole, "cut {cut} in {text:?}");
        }
        let chars: Vec<String> = text.chars().map(String::from).collect();
        assert_eq!(
            split_pieces(lang, chars.iter().map(String::as_str)),
            whole,
            "char by char in {text:?}"
        );
        whole
    }

    #[test]
    fn chinese_sentences_split_at_sentence_marks() {
        let out =
            assert_boundary_independent(Lang::Zh, "我们下午三点在机场见面。你带护照了吗？好的！");
        assert_eq!(
            out,
            ["我们下午三点在机场见面。", "你带护照了吗？", "好的！"]
        );
    }

    #[test]
    fn chinese_clause_marks_need_six_characters() {
        let out = assert_boundary_independent(Lang::Zh, "如果明天下雨，我们就改期，好吗？");
        // "如果明天下雨" has 6 characters; "我们就改期" has 5 and joins the rest.
        assert_eq!(out, ["如果明天下雨，", "我们就改期，好吗？"]);
    }

    #[test]
    fn english_sentences_split_when_two_words_are_present() {
        let out = assert_boundary_independent(
            Lang::En,
            "We meet at noon. Please bring the report. Okay.",
        );
        assert_eq!(
            out,
            ["We meet at noon.", "Please bring the report.", "Okay."]
        );
    }

    #[test]
    fn a_too_short_sentence_joins_the_next_one() {
        let out = assert_boundary_independent(Lang::En, "Yes. We meet at noon.");
        assert_eq!(out, ["Yes. We meet at noon."]);
    }

    #[test]
    fn english_clause_marks_need_four_words() {
        let out = assert_boundary_independent(Lang::En, "If it rains tomorrow, we will move it.");
        assert_eq!(out, ["If it rains tomorrow,", "we will move it."]);
    }

    #[test]
    fn decimal_numbers_thousands_and_times_are_not_split() {
        let out = assert_boundary_independent(
            Lang::En,
            "The price is 3.5 dollars. It was 1,000 last year. We meet at 10:30 sharp.",
        );
        assert_eq!(
            out,
            [
                "The price is 3.5 dollars.",
                "It was 1,000 last year.",
                "We meet at 10:30 sharp."
            ]
        );
        let out = assert_boundary_independent(Lang::Zh, "总共是3.5公斤，需要十二点三十分到达。");
        // The decimal point is not a cut; the comma after 7 units is.
        assert_eq!(out, ["总共是3.5公斤，", "需要十二点三十分到达。"]);
    }

    #[test]
    fn a_number_at_the_end_of_a_chunk_waits_for_the_next_character() {
        let mut s = ClauseSplitter::new(Lang::En);
        assert!(s.push("The price is 3.").is_empty(), "3. may be 3.5");
        assert!(s.push("5 dollars").is_empty());
        assert_eq!(s.push(". Next"), ["The price is 3.5 dollars."]);
        assert_eq!(s.finish(), ["Next"]);
    }

    #[test]
    fn abbreviations_and_initials_do_not_end_a_sentence() {
        let text =
            "Please ask Dr. Smith about it. He lives in the U.S. now. Call J. Brown, e.g. today.";
        let out = assert_boundary_independent(Lang::En, text);
        assert_eq!(
            out,
            [
                "Please ask Dr. Smith about it.",
                "He lives in the U.S. now.",
                "Call J. Brown, e.g. today."
            ]
        );
    }

    #[test]
    fn dotted_names_and_ellipses_are_handled() {
        let out = assert_boundary_independent(
            Lang::En,
            "Write to info@example.com today. Well... we will see.",
        );
        assert_eq!(
            out,
            ["Write to info@example.com today.", "Well... we will see."]
        );
    }

    #[test]
    fn a_lowercase_continuation_after_a_dot_keeps_the_sentence_together() {
        let out = assert_boundary_independent(Lang::En, "It costs approx. five dollars. Fine.");
        assert_eq!(out, ["It costs approx. five dollars.", "Fine."]);
    }

    #[test]
    fn a_clause_is_emitted_before_the_stream_ends() {
        let mut s = ClauseSplitter::new(Lang::En);
        assert!(s.push("We meet").is_empty());
        assert!(s.push(" at noon").is_empty());
        assert!(
            s.push(".").is_empty(),
            "the dot waits for the next character"
        );
        assert_eq!(s.push(" And"), ["We meet at noon."]);
        let mut z = ClauseSplitter::new(Lang::Zh);
        assert_eq!(
            z.push("我们在中午见面。"),
            ["我们在中午见面。"],
            "full-width marks need no lookahead"
        );
    }

    #[test]
    fn trailing_quotes_stay_with_their_clause_and_stray_marks_are_dropped() {
        let out = assert_boundary_independent(Lang::En, "He said \"we are late.\" Then we left.");
        assert_eq!(out, ["He said \"we are late.\"", "Then we left."]);
        let out = assert_boundary_independent(Lang::Zh, "你真的是认真的吗？！我不相信。");
        assert_eq!(out, ["你真的是认真的吗？", "我不相信。"]);
    }

    #[test]
    fn the_unfinished_tail_is_flushed_by_finish_and_empty_input_yields_nothing() {
        let out = assert_boundary_independent(Lang::En, "Call me back when you are free");
        assert_eq!(out, ["Call me back when you are free"]);
        assert!(split_whole(Lang::En, "").is_empty());
        assert!(split_whole(Lang::En, " ... ").is_empty());
    }

    #[test]
    fn mixed_text_counts_words_and_characters_together() {
        let out = assert_boundary_independent(Lang::Zh, "请打开 Wi-Fi 设置。然后重启。");
        assert_eq!(out, ["请打开 Wi-Fi 设置。", "然后重启。"]);
    }

    #[test]
    fn first_clause_end_follows_the_splitter_rule() {
        assert_eq!(first_clause_end(Lang::En, "The total is 3.5 dollars"), None);
        assert_eq!(
            first_clause_end(Lang::En, "The total is 3.5 dollars. Then"),
            Some("The total is 3.5 dollars.".len())
        );
        assert_eq!(
            first_clause_end(Lang::En, "Hello."),
            None,
            "too short, and the dot waits"
        );
        assert_eq!(first_clause_end(Lang::En, "Dr. Smith said"), None);
        assert_eq!(
            first_clause_end(Lang::Zh, "今天天气很好。然后"),
            Some("今天天气很好。".len())
        );
        assert_eq!(first_clause_end(Lang::En, "The U.S. economy grew"), None);
    }

    #[test]
    fn stray_marks_after_a_cut_are_trimmed() {
        assert_eq!(trim_stray_start(" ?! \"next"), "next");
        assert_eq!(trim_stray_start("next"), "next");
    }
}
