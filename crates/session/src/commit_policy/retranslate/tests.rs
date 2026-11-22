use std::collections::HashMap;

use super::*;

/// Chinese words of the monotone fake translator: a text is translated word by word, so the
/// translation of a prefix is always the prefix of the translation.
const WORDS: &[(&str, &str)] = &[
    ("we", "我们"),
    ("should", "应该"),
    ("meet", "见面"),
    ("at", "在"),
    ("the", "这个"),
    ("office", "办公室"),
    ("tomorrow", "明天"),
    ("morning", "早上"),
    ("and", "然后"),
    ("then", "接着"),
    ("go", "回去"),
    ("home", "家里"),
];

fn monotone_zh(text: &str) -> String {
    units(text)
        .iter()
        .map(|u| {
            WORDS
                .iter()
                .find(|(w, _)| *w == u.key)
                .map_or("某", |(_, z)| *z)
        })
        .collect()
}

/// Drives a book like the recognition stage and the draft worker would: every text of a plan is
/// translated at once and the reply is given back before the next hypothesis.
struct Rig {
    book: DraftBook,
    p: RetranslateParams,
    source: Lang,
    target: Lang,
    sentence: String,
    revision: u32,
    /// Exact translations that replace the engine's answer for a source text.
    overrides: HashMap<String, String>,
    translate: fn(&str) -> String,
    /// Source texts the engine was asked for, in order.
    asked: Vec<String>,
}

impl Rig {
    fn english_to_chinese() -> Self {
        Self {
            book: DraftBook::default(),
            p: RetranslateParams {
                enabled: true,
                ..RetranslateParams::disabled()
            },
            source: Lang::En,
            target: Lang::Zh,
            sentence: String::new(),
            revision: 0,
            overrides: HashMap::new(),
            translate: monotone_zh,
            asked: Vec::new(),
        }
    }

    fn translated(&self, source: &str) -> String {
        self.overrides
            .get(source)
            .cloned()
            .unwrap_or_else(|| (self.translate)(source))
    }

    /// One hypothesis: the plan, the translations of its texts and the reply.
    fn hyp(&mut self, text: &str) -> (Option<DraftPlan>, Option<RetranslateCut>) {
        self.revision += 1;
        let p = self.p;
        let sentence = self.sentence.clone();
        let input = HypInput {
            source: self.source,
            target: self.target,
            round_text: text,
            committed: 0,
            carry: "",
            sentence: &sentence,
            revision: self.revision,
        };
        let plan = self.book.on_hypothesis(&p, &input);
        let Some(plan) = plan else {
            return (None, None);
        };
        let mut sources: Vec<String> = Vec::new();
        for s in [
            plan.sentence.clone(),
            plan.candidate.clone(),
            Some(plan.reference.clone()),
        ]
        .into_iter()
        .flatten()
        {
            if !sources.contains(&s) {
                sources.push(s);
            }
        }
        let targets: Vec<String> = sources.iter().map(|s| self.translated(s)).collect();
        self.asked.extend(sources.iter().cloned());
        let results: Vec<(&str, Option<DraftTranslation<'_>>)> = sources
            .iter()
            .zip(&targets)
            .map(|(s, t)| {
                (
                    s.as_str(),
                    Some(DraftTranslation {
                        text: t.as_str(),
                        wrong_language: false,
                    }),
                )
            })
            .collect();
        let view = DraftReplyView {
            revision: self.revision,
            results: &results,
        };
        let cut = self.book.on_reply(&p, self.target, &view);
        (Some(plan), cut)
    }

    fn cut_sources(&mut self, texts: &[&str]) -> Vec<Option<String>> {
        texts
            .iter()
            .map(|t| self.hyp(t).1.map(|c| c.source_full))
            .collect()
    }
}

const GROWING: [&str; 7] = [
    "we should meet",
    "we should meet at",
    "we should meet at the",
    "we should meet at the office",
    "we should meet at the office tomorrow",
    "we should meet at the office tomorrow morning",
    "we should meet at the office tomorrow morning and",
];

fn candidate(cut: usize, source: &str, target: &str, agreement: u32) -> Candidate {
    Candidate {
        cut,
        prefix: String::new(),
        source_full: source.to_owned(),
        target: Some((target.to_owned(), false)),
        created: 1,
        agreement,
        counted: None,
    }
}

fn book_with(candidates: Vec<Candidate>, spoken: &str) -> DraftBook {
    DraftBook {
        candidates,
        spoken: Some(spoken.to_owned()),
        ..DraftBook::default()
    }
}

fn decide(book: &mut DraftBook, target: Lang) -> Option<RetranslateCut> {
    let p = RetranslateParams::disabled();
    book.decide(&p, target)
}

#[test]
fn agreement_counts_once_per_revision_and_the_cut_comes_with_the_second_reference() {
    let mut rig = Rig::english_to_chinese();
    let cuts = rig.cut_sources(&GROWING[..4]);
    // The first candidate appears with the third hypothesis (two agreeing readings, a guard of one
    // word, three words); the fourth is its second confirming reference.
    assert_eq!(cuts, [None, None, None, Some("we should meet".to_owned())]);
}

#[test]
fn a_disagreeing_reference_resets_the_count() {
    let mut rig = Rig::english_to_chinese();
    // The engine rewrites the start of its translation for the fourth reading.
    rig.overrides.insert(
        "we should meet at the office".to_owned(),
        "办公室里见面".to_owned(),
    );
    let cuts = rig.cut_sources(&GROWING);
    // Revision 4 resets the first candidate; revisions 5 and 6 count again, so the cut comes two
    // revisions later than without the rewrite.
    assert_eq!(
        cuts,
        [
            None,
            None,
            None,
            None,
            None,
            Some("we should meet".to_owned()),
            None,
        ]
    );
}

#[test]
fn a_reference_that_does_not_extend_far_enough_is_ignored() {
    let mut rig = Rig::english_to_chinese();
    rig.p.lookahead = 3;
    let cuts = rig.cut_sources(&GROWING[..5]);
    // The reference of revision 3 reaches two words past the cut: it does not count. Revisions 4
    // and 5 do, so the cut is one revision later than with a lookahead of two.
    assert_eq!(
        cuts,
        [None, None, None, None, Some("we should meet".to_owned())]
    );
}

#[test]
fn the_longest_qualifying_candidate_wins() {
    let mut book = book_with(
        vec![
            candidate(14, "we should meet", "我们应该见面", 2),
            candidate(
                28,
                "we should meet at the office",
                "我们应该见面在这个办公室",
                2,
            ),
            candidate(
                37,
                "we should meet at the office tomorrow",
                "我们应该见面在这个办公室明天",
                1,
            ),
        ],
        "",
    );
    let cut = decide(&mut book, Lang::Zh).expect("a cut");
    assert_eq!(cut.cut, 28);
    assert_eq!(cut.source_full, "we should meet at the office");
}

#[test]
fn a_candidate_whose_translation_no_longer_starts_with_the_spoken_part_is_dropped() {
    let mut book = book_with(
        vec![candidate(
            28,
            "we should meet at the office",
            "他们必须在办公室见面",
            2,
        )],
        "我们应该见面",
    );
    assert_eq!(decide(&mut book, Lang::Zh), None);
    assert!(book.candidates.is_empty());
    assert_eq!(book.rejected_spoken, 1);
}

#[test]
fn only_the_new_part_after_the_spoken_part_has_to_be_speakable() {
    // Spoken "我们应该见面"; the candidate adds "在这个办公室".
    let mut book = book_with(
        vec![candidate(
            28,
            "we should meet at the office",
            "我们应该见面在这个办公室",
            2,
        )],
        "我们应该见面",
    );
    let cut = decide(&mut book, Lang::Zh).expect("a cut");
    assert_eq!(cut.target_full, "我们应该见面在这个办公室");
}

#[test]
fn a_new_part_that_ends_on_a_dangling_target_word_is_refused() {
    let mut en = book_with(
        vec![candidate(10, "wir treffen uns", "We should meet at the", 2)],
        "",
    );
    assert_eq!(decide(&mut en, Lang::En), None);
    let mut zh = book_with(
        vec![candidate(10, "meet at the office", "我们在公司的", 2)],
        "",
    );
    assert_eq!(decide(&mut zh, Lang::Zh), None);
    // The same candidates, ending on a content word, pass.
    let mut en = book_with(vec![candidate(10, "x", "We should meet there", 2)], "");
    assert!(decide(&mut en, Lang::En).is_some());
    let mut zh = book_with(vec![candidate(10, "x", "我们在公司见面", 2)], "");
    assert!(decide(&mut zh, Lang::Zh).is_some());
}

#[test]
fn a_new_part_with_too_few_target_units_is_refused() {
    let mut en = book_with(vec![candidate(10, "x", "Hello", 2)], "");
    assert_eq!(decide(&mut en, Lang::En), None);
    let mut zh = book_with(vec![candidate(10, "x", "你好", 2)], "");
    assert_eq!(decide(&mut zh, Lang::Zh), None);
    let mut en = book_with(vec![candidate(10, "x", "Hello there", 2)], "");
    assert!(decide(&mut en, Lang::En).is_some());
    let mut zh = book_with(vec![candidate(10, "x", "你好朋友", 2)], "");
    assert!(decide(&mut zh, Lang::Zh).is_some());
    // One extra word beyond the spoken part is not enough either.
    let mut en = book_with(vec![candidate(10, "x", "We meet there", 2)], "We meet");
    assert_eq!(decide(&mut en, Lang::En), None);
}

#[test]
fn a_draft_in_the_wrong_language_never_makes_a_cut() {
    let mut book = book_with(
        vec![candidate(10, "x", "我们应该明天在办公室见面谈谈", 2)],
        "",
    );
    assert_eq!(decide(&mut book, Lang::En), None);
    // The engine's own flag does the same for a text the script check cannot judge.
    let mut flagged = candidate(10, "x", "We should meet there", 2);
    flagged.target = Some(("We should meet there".to_owned(), true));
    let mut book = book_with(vec![flagged], "");
    assert_eq!(decide(&mut book, Lang::En), None);
}

#[test]
fn a_candidate_without_enough_agreement_or_a_known_spoken_part_waits() {
    let mut book = book_with(vec![candidate(10, "x", "We should meet there", 1)], "");
    assert_eq!(decide(&mut book, Lang::En), None);
    assert_eq!(book.candidates.len(), 1);
    let mut book = DraftBook {
        candidates: vec![candidate(10, "x", "We should meet there", 2)],
        ..DraftBook::default()
    };
    assert_eq!(decide(&mut book, Lang::En), None);
    assert_eq!(book.candidates.len(), 1);
}

#[test]
fn a_candidate_that_is_only_filler_is_never_created() {
    let mut rig = Rig::english_to_chinese();
    for text in [
        "um uh um uh",
        "um uh um uh um",
        "um uh um uh um uh",
        "um uh um uh um uh um",
    ] {
        let (plan, cut) = rig.hyp(text);
        assert!(plan.is_none(), "{text}");
        assert!(cut.is_none());
    }
    assert!(rig.book.candidates.is_empty());
}

#[test]
fn a_candidate_is_invalidated_when_the_round_text_before_its_cut_changes() {
    let mut rig = Rig::english_to_chinese();
    rig.cut_sources(&GROWING[..3]);
    assert_eq!(rig.book.candidates.len(), 1);
    // The recogniser rewrites "should" inside the candidate.
    let (plan, _) = rig.hyp("we would meet at the office");
    assert!(
        rig.book
            .candidates
            .iter()
            .all(|c| c.source_full != "we should meet"),
        "{:?}",
        rig.asked
    );
    // Nothing agreed yet for the new wording, so nothing is asked.
    assert!(plan.is_none());
}

#[test]
fn a_word_order_change_in_the_translation_gives_no_cut() {
    // Chinese puts the time before the verb, English after it: the translation of the start of the
    // sentence keeps changing until the verb arrives.
    let mut rig = Rig {
        source: Lang::Zh,
        target: Lang::En,
        p: RetranslateParams {
            enabled: true,
            min_chars: 2,
            ..RetranslateParams::disabled()
        },
        ..Rig::english_to_chinese()
    };
    for (zh, en) in [
        ("我明天", "Tomorrow I"),
        ("我明天去", "I am going tomorrow"),
        ("我明天去北", "I am going north tomorrow"),
        ("我明天去北京", "I am going to Beijing tomorrow"),
        (
            "我明天去北京出差",
            "I am going to Beijing on a trip tomorrow",
        ),
    ] {
        rig.overrides.insert(zh.to_owned(), en.to_owned());
    }
    let cuts = rig.cut_sources(&["我明天去", "我明天去北", "我明天去北京", "我明天去北京出差"]);
    assert_eq!(cuts, [None, None, None, None], "{:?}", rig.asked);
    assert!(rig.asked.iter().any(|s| s == "我明天"), "{:?}", rig.asked);
}

#[test]
fn the_cut_leaves_the_sentence_translated_for_the_next_unit() {
    let mut rig = Rig::english_to_chinese();
    let cuts = rig.cut_sources(&GROWING[..4]);
    assert!(cuts[3].is_some());
    // The next unit is built on the sentence "we should meet": its translation is known.
    rig.sentence = "we should meet".to_owned();
    rig.book.reset();
    let (plan, _) = rig.hyp("at the office tomorrow");
    assert!(plan.is_none(), "one reading only");
    let (plan, _) = rig.hyp("at the office tomorrow morning");
    let plan = plan.expect("a plan");
    assert_eq!(plan.sentence, None, "the spoken translation is known");
}

#[test]
fn a_sentence_the_book_did_not_translate_is_asked_for() {
    let mut rig = Rig::english_to_chinese();
    rig.sentence = "we should meet".to_owned();
    rig.hyp("at the office");
    rig.hyp("at the office tomorrow");
    let (plan, _) = rig.hyp("at the office tomorrow morning");
    let plan = plan.expect("a plan");
    assert_eq!(plan.sentence.as_deref(), Some("we should meet"));
}

#[test]
fn join_sentence_puts_a_space_except_between_chinese_characters() {
    for (a, b, want) in [
        ("", "hello", "hello"),
        ("hello", "", "hello"),
        ("we meet", "at nine", "we meet at nine"),
        ("我们明天", "去北京", "我们明天去北京"),
        ("如果明天下雨，", "我们改期", "如果明天下雨，我们改期"),
        ("我们", "meet at nine", "我们 meet at nine"),
        ("we meet", "在北京", "we meet 在北京"),
    ] {
        assert_eq!(join_sentence(a, b), want, "{a:?} + {b:?}");
    }
}

#[test]
fn join_carry_joins_with_one_space() {
    assert_eq!(join_carry("", "a b"), "a b");
    assert_eq!(join_carry("the", ""), "the");
    assert_eq!(join_carry("the", "office"), "the office");
}

#[test]
fn prefix_end_ignores_case_punctuation_and_spacing() {
    let full = "We should meet, at the office.";
    for (spoken, want) in [
        ("", Some(0)),
        ("we should meet", Some("We should meet,".len())),
        ("WE SHOULD MEET!", Some("We should meet,".len())),
        ("we  should-meet at", Some("We should meet, at".len())),
        ("we should meat", None),
        ("we should meet at the office again", None),
        ("we should meet at the office", Some(full.len())),
    ] {
        assert_eq!(prefix_end(spoken, full), want, "{spoken:?}");
    }
    assert_eq!(
        prefix_end("我们应该", "我们应该，见面"),
        Some("我们应该，".len())
    );
    assert_eq!(
        prefix_end("我们应该。", "我们应该见面"),
        Some("我们应该".len())
    );
    assert_eq!(prefix_end("我们应该见面", "我们应该"), None);
}

#[test]
fn continuation_text_turns_a_sentence_mark_into_a_comma() {
    for (lang, text, want) in [
        (Lang::En, "We should meet.", "We should meet,"),
        (Lang::En, "Really?", "Really,"),
        (Lang::En, "Stop!", "Stop,"),
        (Lang::En, "Well...", "Well,"),
        (Lang::En, "Right?!  ", "Right,  "),
        (Lang::En, "We should meet,", "We should meet,"),
        (Lang::En, "We should meet", "We should meet"),
        (Lang::En, "3.5", "3.5"),
        (Lang::En, "...", "..."),
        (Lang::Zh, "我们应该见面。", "我们应该见面，"),
        (Lang::Zh, "真的吗？", "真的吗，"),
        (Lang::Zh, "Stop.", "Stop，"),
        (Lang::Zh, "我们应该见面，", "我们应该见面，"),
    ] {
        assert_eq!(continuation_text(lang, text), want, "{text:?}");
    }
}

#[test]
fn the_agreed_prefix_is_what_every_reading_has_in_common() {
    let k = |s: &str| -> Vec<String> {
        s.split(' ')
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(agreed_prefix_units(&[]), 0);
    assert_eq!(agreed_prefix_units(&[k("a b c")]), 3);
    assert_eq!(agreed_prefix_units(&[k("a b c"), k("a b c d")]), 3);
    assert_eq!(agreed_prefix_units(&[k("a b c"), k("a x c d")]), 1);
    assert_eq!(
        agreed_prefix_units(&[k("a b c d"), k("a b c"), k("a b x c")]),
        2
    );
    assert_eq!(agreed_prefix_units(&[k(""), k("a")]), 0);
}

#[test]
fn a_cut_never_splits_a_number_or_ends_on_a_dangling_word() {
    // Agreed prefix "the budget is 3" would end inside a figure: the candidate is cut earlier or
    // not at all.
    let mut rig = Rig::english_to_chinese();
    rig.hyp("the budget is 3.5 million");
    rig.hyp("the budget is 3.5 million dollars");
    rig.hyp("the budget is 3.5 million dollars for");
    assert!(
        rig.book
            .candidates
            .iter()
            .all(|c| !c.source_full.ends_with("is") && !c.source_full.ends_with("3.5")),
        "{:?}",
        rig.asked
    );
}

#[test]
fn the_default_parameters_are_the_documented_ones() {
    let p = RetranslateParams::disabled();
    assert!(!p.enabled && !p.continuation);
    assert_eq!(
        (p.agreement, p.lookahead, p.source_agreement, p.source_guard),
        (2, 2, 2, 1)
    );
    assert_eq!((p.min_chars, p.min_words), (4, 3));
    assert_eq!(p.interval, Duration::from_millis(300));
}
