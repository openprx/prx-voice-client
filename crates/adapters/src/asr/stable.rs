//! The stable-prefix rule for a recogniser that re-decodes a growing buffer.
//!
//! Qwen3-ASR is not incremental: every partial is a fresh decode of all the audio so far, and
//! about three quarters of such updates rewrite earlier text (bench/asr). So the adapter decides
//! what is stable from agreement: the longest prefix, in comparison units ([`super::units`]: one
//! per Chinese character, one per word of other text, punctuation and spacing ignored), on which
//! the last `N` consecutive hypotheses agree is declared stable.
//!
//! Agreement alone is not enough: a reading that survives a few passes can still be wrong until the
//! model has heard what comes after it (measured: "among is seventy" for "among the seventy-eight"
//! agreed for three passes and a later, better reading contradicted it). So the last `guard` units
//! of the newest hypothesis are never declared stable while the text is still growing; once the last
//! `N` hypotheses are identical (the text stopped growing, for example in a pause) everything is.
//!
//! A declared prefix is never retracted by a partial. If a later partial does not start with the
//! declared units (the model changed its mind, or the text shrank below it), the declared prefix is
//! kept and the partial is repaired to start with it: the declared text, then whatever the new
//! hypothesis says after the same number of units. Each repair is counted. The final hypothesis is
//! the exception: it is the model's best reading and is returned unrepaired, with the
//! contradiction counted.

use std::collections::VecDeque;

use super::units::{Unit, punctuation_after, units};

/// A hypothesis after the rule was applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stabilized {
    /// The text, starting with the declared prefix.
    pub(crate) text: String,
    /// Length in bytes of the declared stable prefix of `text`.
    pub(crate) stable_len: usize,
    /// Whether the model's text contradicted the declared prefix (a partial is then repaired to keep
    /// it; a final is returned unchanged).
    pub(crate) repaired: bool,
}

/// The declared prefix.
#[derive(Clone, Debug, Default)]
struct Declared {
    /// Units declared stable.
    keys: Vec<String>,
    /// The text of those units, with the punctuation that followed them when they were declared.
    text: String,
}

/// Tracks the hypotheses of one segment.
#[derive(Clone, Debug)]
pub(crate) struct StablePrefix {
    agreement: usize,
    guard: usize,
    window: VecDeque<(String, Vec<Unit>)>,
    declared: Declared,
}

impl StablePrefix {
    /// A tracker that declares text stable once `agreement` consecutive hypotheses (at least two)
    /// agree on it, except for the last `guard` units while the text still grows.
    pub(crate) fn new(agreement: usize, guard: usize) -> Self {
        Self {
            agreement: agreement.max(2),
            guard,
            window: VecDeque::new(),
            declared: Declared::default(),
        }
    }

    /// Start a new segment.
    pub(crate) fn reset(&mut self) {
        self.window.clear();
        self.declared = Declared::default();
    }

    /// Units declared stable so far.
    #[cfg(test)]
    pub(crate) fn declared_units(&self) -> usize {
        self.declared.keys.len()
    }

    /// Whether `new` starts with the declared units; if not, the repaired text.
    fn repair(&self, text: &str, new: &[Unit]) -> Option<String> {
        let k = self.declared.keys.len();
        if k == 0 {
            return None;
        }
        let agrees = new.len() >= k && new[..k].iter().map(|u| &u.key).eq(&self.declared.keys);
        if agrees {
            return None;
        }
        let tail = if new.len() > k {
            &text[new[k - 1].end..]
        } else {
            ""
        };
        Some(format!("{}{}", self.declared.text, tail))
    }

    /// Apply the rule to the next hypothesis of the segment.
    pub(crate) fn push(&mut self, text: &str) -> Stabilized {
        let mut text = text.to_owned();
        let mut new = units(&text);
        let repaired = match self.repair(&text, &new) {
            Some(fixed) => {
                text = fixed;
                new = units(&text);
                true
            }
            None => false,
        };
        self.window.push_back((text.clone(), new.clone()));
        while self.window.len() > self.agreement {
            self.window.pop_front();
        }
        let declared_k = self.declared.keys.len();
        let mut stable = declared_k;
        if self.window.len() >= self.agreement {
            let first = &self.window[0].1;
            let common = (0..first.len())
                .take_while(|&i| {
                    self.window
                        .iter()
                        .all(|(_, u)| u.get(i).is_some_and(|x| x.key == first[i].key))
                })
                .count();
            let identical = self.window.iter().all(|(_, u)| u.len() == first.len());
            let allowed = if identical {
                common
            } else {
                common.min(new.len().saturating_sub(self.guard))
            };
            stable = stable.max(allowed);
        }
        let stable_len = self.stable_len(&text, &new, stable);
        if stable > declared_k {
            self.declared = Declared {
                keys: new[..stable].iter().map(|u| u.key.clone()).collect(),
                text: text[..stable_len].to_owned(),
            };
        }
        Stabilized {
            text,
            stable_len,
            repaired,
        }
    }

    /// Byte length of the first `stable` units of `text`, taking the punctuation that follows the
    /// last of them along when every hypothesis of the window has the same mark there.
    fn stable_len(&self, text: &str, new: &[Unit], stable: usize) -> usize {
        if stable == 0 {
            return 0;
        }
        let end = new[stable - 1].end;
        let mark = punctuation_after(text, end);
        let same = self.window.len() >= self.agreement
            && self.window.iter().all(|(t, u)| {
                u.get(stable - 1)
                    .is_some_and(|x| punctuation_after(t, x.end) == mark)
            });
        if same { end + mark.len() } else { end }
    }

    /// The final hypothesis of the segment: the whole text is stable. The final decode is the most
    /// accurate reading of the segment, so it is returned as the model gave it even when it
    /// contradicts a prefix declared stable earlier (repairing it would cost accuracy: measured
    /// 3.7 to 4.6 % error on Mandarin and 4.8 to 5.3 % on English against 3.7 and 4.8 for the plain
    /// decode); `repaired` then says that the declared prefix was contradicted.
    pub(crate) fn finish(&mut self, text: &str) -> Stabilized {
        let contradicts = self.repair(text, &units(text)).is_some();
        self.reset();
        Stabilized {
            stable_len: text.len(),
            text: text.to_owned(),
            repaired: contradicts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stable prefix text of each hypothesis of a scripted sequence.
    fn run(n: usize, hyps: &[&str]) -> Vec<(String, String, bool)> {
        let mut s = StablePrefix::new(n, 0);
        hyps.iter()
            .map(|h| {
                let r = s.push(h);
                (
                    r.text.clone(),
                    r.text[..r.stable_len].to_owned(),
                    r.repaired,
                )
            })
            .collect()
    }

    fn stable_of(rows: &[(String, String, bool)]) -> Vec<&str> {
        rows.iter().map(|r| r.1.as_str()).collect()
    }

    #[test]
    fn growing_chinese_text_becomes_stable_after_n_agreeing_hypotheses() {
        let rows = run(3, &["你", "你好", "你好世", "你好世界", "你好世界啊"]);
        // Window of three: the first full window is rows 0..=2, agreeing on "你".
        assert_eq!(stable_of(&rows), ["", "", "你", "你好", "你好世"]);
        assert!(rows.iter().all(|r| !r.2));
    }

    #[test]
    fn growing_english_text_is_stable_by_whole_words() {
        let rows = run(
            3,
            &[
                "the",
                "the meeting",
                "the meeting has",
                "the meeting has been",
                "the meeting has been moved",
            ],
        );
        assert_eq!(
            stable_of(&rows),
            ["", "", "the", "the meeting", "the meeting has"]
        );
    }

    #[test]
    fn identical_hypotheses_make_everything_stable() {
        let rows = run(3, &["hello world", "hello world", "hello world"]);
        assert_eq!(stable_of(&rows), ["", "", "hello world"]);
    }

    #[test]
    fn rewriting_text_stays_unstable_until_it_settles() {
        // The tail flips between readings; only the agreed head is stable.
        let rows = run(
            3,
            &["我想去商店", "我想去上店", "我想去商点", "我想去商店买"],
        );
        assert_eq!(stable_of(&rows), ["", "", "我想去", "我想去"]);
    }

    #[test]
    fn oscillating_hypotheses_never_declare_the_oscillating_part() {
        let rows = run(3, &["a b", "a c", "a b", "a c", "a b", "a c"]);
        assert!(stable_of(&rows).iter().all(|s| s.is_empty() || *s == "a"));
        assert_eq!(rows[5].1, "a");
    }

    #[test]
    fn a_declared_prefix_is_never_retracted() {
        let mut s = StablePrefix::new(3, 0);
        for h in ["我想去商店", "我想去商店买", "我想去商店买东西"] {
            s.push(h);
        }
        assert_eq!(s.declared_units(), 5);
        // The model now rewrites the declared part.
        let r = s.push("我要去超市买东西");
        assert!(r.repaired);
        assert!(r.text.starts_with("我想去商店"), "{}", r.text);
        // The text after the declared units is the new reading's; the hypotheses of the window
        // now agree on one more character.
        assert_eq!(r.text, "我想去商店买东西");
        assert_eq!(&r.text[..r.stable_len], "我想去商店买");
        assert_eq!(s.declared_units(), 6);
    }

    #[test]
    fn a_shrinking_hypothesis_keeps_the_declared_prefix() {
        let mut s = StablePrefix::new(2, 0);
        s.push("hello big world");
        let r = s.push("hello big world");
        assert_eq!(&r.text[..r.stable_len], "hello big world");
        let r = s.push("hello big");
        assert!(r.repaired);
        assert_eq!(r.text, "hello big world");
        assert_eq!(r.stable_len, r.text.len());
    }

    #[test]
    fn empty_hypotheses_before_speech_are_harmless() {
        let rows = run(2, &["", "", "hi", "hi there", "hi there"]);
        assert_eq!(stable_of(&rows), ["", "", "", "hi", "hi there"]);
    }

    #[test]
    fn punctuation_and_spacing_differences_do_not_break_agreement() {
        let rows = run(
            3,
            &[
                "Hello, world",
                "hello world",
                "Hello world!",
                "hello world!",
            ],
        );
        assert!(rows.iter().all(|r| !r.2));
        assert_eq!(
            rows[2].1.to_lowercase().replace([',', '!', ' '], ""),
            "helloworld"
        );
        // The sentence mark is stable only when the whole window agrees on it.
        assert!(!rows[2].1.ends_with('!'));
        let more = run(3, &["你好。", "你好。", "你好。"]);
        assert_eq!(more[2].1, "你好。");
    }

    #[test]
    fn the_final_hypothesis_is_all_stable_and_resets_the_tracker() {
        let mut s = StablePrefix::new(2, 0);
        s.push("你好");
        s.push("你好世界");
        let f = s.finish("你好世界吗");
        assert_eq!(f.stable_len, f.text.len());
        assert!(!f.repaired);
        assert_eq!(s.declared_units(), 0);
    }

    #[test]
    fn a_final_that_contradicts_the_declared_prefix_is_kept_and_flagged() {
        let mut s = StablePrefix::new(2, 0);
        s.push("我想去商店");
        s.push("我想去商店买");
        let f = s.finish("我要去超市买");
        assert!(f.repaired);
        assert_eq!(f.text, "我要去超市买");
        assert_eq!(f.stable_len, f.text.len());
    }

    #[test]
    fn the_guard_keeps_the_last_units_of_a_growing_text_unstable() {
        let mut s = StablePrefix::new(3, 2);
        let rows: Vec<String> = [
            "first among is",
            "first among is seven",
            "first among is seventy",
            "first among the seventy eight",
            "first among the seventy eight recommendations",
        ]
        .iter()
        .map(|h| {
            let r = s.push(h);
            r.text[..r.stable_len].to_owned()
        })
        .collect();
        // "is" agreed three times but sat in the last three units; the guard held back two of them,
        // and by the time it was clear of the end the model had read "the" instead.
        assert_eq!(rows, ["", "", "first among", "first among", "first among"]);
        // A pause: the text stops growing and the whole of it becomes stable.
        for _ in 0..3 {
            s.push("first among the seventy eight recommendations");
        }
        let r = s.push("first among the seventy eight recommendations");
        assert_eq!(r.stable_len, r.text.len());
    }

    #[test]
    fn agreement_below_two_is_raised_to_two() {
        let rows = run(0, &["a", "a b", "a b"]);
        assert_eq!(stable_of(&rows), ["", "a", "a b"]);
    }
}
