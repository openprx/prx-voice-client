//! Detects a translation model that loops (PD-2.6.3).
//!
//! In an end-to-end prototype the default translation model once looped on a fragment and emitted
//! the same token until the token cap. The guard watches the text of the stream as it arrives and
//! says as soon as the tail of the output is one short unit repeated many times in a row, or a
//! longer stretch that cycles with a short period, so that the request can be aborted early and the
//! pipeline can apply its policy instead of synthesising the loop.
//!
//! The guard looks at characters only and is language aware in one way: how much repetition is
//! legitimate depends on what repeats.
//!
//! | The repeating unit | Fires after | Does not fire on |
//! |---|---|---|
//! | one letter or Chinese character | [`LETTER_RUN_CHARS`] in a row | "哈哈哈", "ahhhh" |
//! | digits (with `,` `.` and spaces) | [`DIGIT_RUN_CHARS`] | "000000", "1,000,000,000" |
//! | punctuation or spaces only | [`MARK_RUN_CHARS`] | "......", "。。。。。。", an ellipsis |
//! | 2 to [`SHORT_UNIT_MAX`] characters | [`SHORT_UNIT_REPEATS`] repeats and [`SHORT_UNIT_MIN_CHARS`] characters | "no no no", "很好很好", "No, no, no, no, no." |
//! | up to [`MAX_PERIOD`] characters | [`CYCLE_REPEATS`] repeats and [`CYCLE_MIN_CHARS`] characters | "Thank you. Thank you. Thank you." |
//!
//! The thresholds are deliberately far above what speech repeats and far below what a loop
//! produces before the token cap. The guard keeps a bounded window of the latest characters and
//! never stores the whole output.

use std::collections::VecDeque;

/// A letter or Chinese character repeated this many times in a row is a loop.
pub const LETTER_RUN_CHARS: usize = 16;
/// Digits (with the separators of a number) repeating for this many characters are a loop.
pub const DIGIT_RUN_CHARS: usize = 40;
/// Punctuation or spaces repeating for this many characters are a loop.
pub const MARK_RUN_CHARS: usize = 40;
/// The longest unit that counts as "short": 2 to this many characters.
pub const SHORT_UNIT_MAX: usize = 8;
/// A short unit repeated this many times in a row is a loop.
pub const SHORT_UNIT_REPEATS: usize = 8;
/// ... provided the repeats cover at least this many characters.
pub const SHORT_UNIT_MIN_CHARS: usize = 16;
/// The longest period the guard looks for.
pub const MAX_PERIOD: usize = 64;
/// A unit longer than [`SHORT_UNIT_MAX`] repeated this many times in a row is a loop ...
pub const CYCLE_REPEATS: usize = 4;
/// ... provided the repeats cover at least this many characters.
pub const CYCLE_MIN_CHARS: usize = 64;
/// How many of the latest characters the guard keeps: enough for the longest threshold.
const WINDOW: usize = (CYCLE_REPEATS + 1) * MAX_PERIOD;

/// What the guard found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repetition {
    /// A short unit repeated in a row.
    Run {
        /// Characters in the unit.
        unit_chars: usize,
        /// Times it repeated.
        repeats: usize,
    },
    /// A longer stretch that cycles.
    Cycle {
        /// Characters in the period.
        period_chars: usize,
        /// Times it repeated.
        repeats: usize,
    },
}

impl Repetition {
    /// The reason text for the pipeline: what was found, never the text itself (call content is
    /// not logged).
    pub fn reason(&self) -> String {
        match self {
            Self::Run {
                unit_chars,
                repeats,
            } => format!(
                "repetition: a unit of {unit_chars} character(s) repeated {repeats} times in a row"
            ),
            Self::Cycle {
                period_chars,
                repeats,
            } => format!(
                "repetition: the output cycles with a period of {period_chars} characters ({repeats} repeats)"
            ),
        }
    }
}

/// How much of a unit's repetition is needed to call it a loop: `(repeats, covered characters)`.
fn needed(unit: &[char]) -> (usize, usize) {
    let is_number_char = |c: &char| c.is_ascii_digit() || matches!(c, ',' | '.' | ' ');
    if unit.iter().all(|c| !c.is_alphanumeric()) {
        (2, MARK_RUN_CHARS)
    } else if unit.iter().all(is_number_char) && unit.iter().any(char::is_ascii_digit) {
        (2, DIGIT_RUN_CHARS)
    } else if unit.len() == 1 {
        (2, LETTER_RUN_CHARS)
    } else if unit.len() <= SHORT_UNIT_MAX {
        (SHORT_UNIT_REPEATS, SHORT_UNIT_MIN_CHARS)
    } else {
        (CYCLE_REPEATS, CYCLE_MIN_CHARS)
    }
}

/// The incremental detector of one request.
#[derive(Clone, Debug, Default)]
pub struct RepetitionGuard {
    tail: VecDeque<char>,
    found: Option<Repetition>,
}

impl RepetitionGuard {
    /// A guard with no history.
    pub fn new() -> Self {
        Self::default()
    }

    /// The first repetition found, if any. Once found it stays found.
    pub fn found(&self) -> Option<Repetition> {
        self.found
    }

    /// Take the next piece of the output (any length, cut anywhere). Returns the repetition the
    /// moment the tail of the output becomes one.
    pub fn push(&mut self, text: &str) -> Option<Repetition> {
        if self.found.is_some() {
            return self.found;
        }
        for c in text.chars() {
            self.tail.push_back(c);
            if self.tail.len() > WINDOW {
                self.tail.pop_front();
            }
        }
        self.found = self.check();
        self.found
    }

    fn check(&mut self) -> Option<Repetition> {
        let s = self.tail.make_contiguous();
        let n = s.len();
        for period in 1..=MAX_PERIOD.min(n / 2) {
            // Trailing positions that equal the character one period earlier.
            let run = (period..n)
                .rev()
                .take_while(|&i| s[i] == s[i - period])
                .count();
            let covered = run + period;
            let repeats = covered / period;
            let (min_repeats, min_chars) = needed(&s[n - period..]);
            if repeats >= min_repeats && covered >= min_chars {
                return Some(if period <= SHORT_UNIT_MAX {
                    Repetition::Run {
                        unit_chars: period,
                        repeats,
                    }
                } else {
                    Repetition::Cycle {
                        period_chars: period,
                        repeats,
                    }
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed `text` in pieces of `step` characters; the repetition found, if any.
    fn feed(text: &str, step: usize) -> Option<Repetition> {
        let chars: Vec<char> = text.chars().collect();
        let mut guard = RepetitionGuard::new();
        for piece in chars.chunks(step) {
            let piece: String = piece.iter().collect();
            if let Some(found) = guard.push(&piece) {
                return Some(found);
            }
        }
        None
    }

    fn fires(text: &str) -> bool {
        feed(text, 1).is_some()
    }

    #[test]
    fn a_single_token_repeated_until_the_cap_is_caught() {
        // The observed failure: a fragment, then the same token over and over.
        let looped = format!("好的，我们先看一下这个点{}", "点".repeat(60));
        let found = feed(&looped, 1).expect("a loop");
        assert!(matches!(
            found,
            Repetition::Run {
                unit_chars: 1,
                repeats: 16
            }
        ));
        assert!(
            fires(&"点点点…".repeat(30)),
            "with the ellipsis in the unit"
        );
        assert!(fires(&format!("Well {}", "uh ".repeat(20))));
    }

    #[test]
    fn the_catch_is_early_and_independent_of_how_the_text_is_cut_into_deltas() {
        let looped = format!("我想说的是{}", "点".repeat(500));
        let at = |step: usize| {
            let chars: Vec<char> = looped.chars().collect();
            let mut guard = RepetitionGuard::new();
            let mut seen = 0;
            for piece in chars.chunks(step) {
                seen += piece.len();
                if guard.push(&piece.iter().collect::<String>()).is_some() {
                    return Some(seen);
                }
            }
            None
        };
        let one = at(1).expect("caught");
        assert!(one <= 5 + LETTER_RUN_CHARS, "caught after {one} characters");
        for step in [2, 3, 7, 50] {
            let seen = at(step).expect("caught");
            assert!(seen >= one && seen < one + step + 1, "{step}: {seen}");
        }
    }

    #[test]
    fn a_short_phrase_that_loops_is_caught() {
        assert!(fires(&"今天天气很好，".repeat(8)));
        assert!(fires(&"ab ".repeat(8)));
        assert!(fires(&"the same words again ".repeat(8)));
        let found = feed(&"That is what I meant. ".repeat(6), 1).expect("a longer cycle");
        assert!(
            matches!(
                found,
                Repetition::Cycle {
                    period_chars: 22,
                    ..
                }
            ),
            "{found:?}"
        );
    }

    #[test]
    fn the_tail_cycling_after_a_normal_start_is_caught() {
        let text = format!(
            "We will meet at three o'clock tomorrow. {}",
            "Please confirm that you can come. ".repeat(6)
        );
        assert!(fires(&text));
    }

    #[test]
    fn legitimate_repetition_does_not_fire() {
        for text in [
            "哈哈哈",
            "哈哈哈哈哈哈哈哈",
            "啊啊啊啊啊啊",
            "ahhhhhhh",
            "no no no",
            "No, no, no, no, no.",
            "I said no no no no no no no, I really did.",
            "很好很好",
            "很好很好很好",
            "好的好的好的好的",
            "000000",
            "0000000000000000",
            "The code is 1000000000000.",
            "1,000,000,000",
            "3.3.3.3.3.3.3.3",
            "......",
            "。。。。。。。。。。",
            "Well…………",
            "Wait... what?...",
            "--------",
            "Thank you. Thank you. Thank you. Thank you.",
            "Yes, yes, yes! Yes! Yes!",
            "谢谢，谢谢，谢谢，谢谢。",
            "bye bye bye",
            "Knock knock. Who's there?",
        ] {
            assert!(!fires(text), "{text:?} is legitimate");
            assert!(feed(text, 3).is_none(), "{text:?} in larger pieces");
        }
    }

    #[test]
    fn ordinary_translations_do_not_fire() {
        for text in [
            "Let's go to the park this afternoon, and then we can have dinner together at the new restaurant.",
            "今天天气很好，我们下午去公园散步，晚上一起在新开的餐厅吃饭，你觉得怎么样？",
            "The meeting starts at nine, ends at ten, and the next one starts at ten, ends at eleven.",
            "他说他说他说的话我都听见了，但是我还是不明白为什么要这样做。",
            "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen",
        ] {
            assert!(!fires(text), "{text:?}");
        }
    }

    #[test]
    fn once_found_it_stays_found_and_the_window_is_bounded() {
        let mut guard = RepetitionGuard::new();
        assert!(guard.push(&"点".repeat(40)).is_some());
        assert!(guard.push("then something else entirely").is_some());
        assert_eq!(guard.found(), guard.push(""));
        let mut guard = RepetitionGuard::new();
        for i in 0..10_000 {
            assert!(guard.push(&format!("word{i} ")).is_none());
        }
        assert!(guard.tail.len() <= WINDOW);
    }

    #[test]
    fn the_reason_names_what_was_found_and_never_quotes_the_text() {
        let found = feed(&"点".repeat(30), 1).unwrap();
        let reason = found.reason();
        assert!(reason.starts_with("repetition:"), "{reason}");
        assert!(!reason.contains('点'));
        let found = feed(&"That is what I meant. ".repeat(6), 1).unwrap();
        assert!(
            found.reason().contains("period of 22"),
            "{}",
            found.reason()
        );
    }
}
