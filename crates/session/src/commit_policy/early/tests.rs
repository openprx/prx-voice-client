//! Deterministic tests of the early commit rule: fixed texts, and a property-style test over random growth of the stable prefix.

use super::*;
use prxvc_media::text_units::units;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn params(early: EarlyCommit) -> CommitParams {
    CommitParams {
        early,
        ..CommitParams::default()
    }
}

/// Byte offset just after the first `n` comparison units of `text` (0 for none).
fn after_units(text: &str, n: usize) -> usize {
    units(text).get(n.wrapping_sub(1)).map_or(0, |u| u.end)
}

/// The cut for `text` of which the first `stable_units` units are stable and `committed` bytes are committed.
fn cut_of(
    p: &CommitParams,
    lang: Lang,
    text: &str,
    committed: usize,
    stable_units: usize,
    waited: Duration,
) -> Option<EarlyCut> {
    decide_early_cut(
        p,
        &EarlyInput {
            lang,
            text,
            committed,
            stable: after_units(text, stable_units),
            waited,
        },
    )
}

fn piece(text: &str, committed: usize, cut: Option<EarlyCut>) -> Option<String> {
    cut.map(|c| text[committed..c.end].trim().to_owned())
}

#[test]
fn the_modes_have_stable_spellings() {
    for m in [
        EarlyCommit::SegmentEnd,
        EarlyCommit::Punctuation,
        EarlyCommit::Chunk,
    ] {
        assert_eq!(EarlyCommit::parse(m.as_str()), Some(m));
    }
    assert_eq!(EarlyCommit::parse("early"), None);
}

#[test]
fn segment_end_mode_never_cuts() {
    let p = params(EarlyCommit::SegmentEnd);
    let text = "We should meet at the office tomorrow morning, and then go";
    assert_eq!(cut_of(&p, Lang::En, text, 0, 9, ms(0)), None);
}

#[test]
fn nothing_stable_nothing_cut() {
    let p = params(EarlyCommit::Chunk);
    let text = "We should meet at the office tomorrow morning, and then go";
    assert_eq!(cut_of(&p, Lang::En, text, 0, 0, ms(5_000)), None);
    // Everything stable is already committed.
    let end = after_units(text, 4);
    assert_eq!(cut_of(&p, Lang::En, text, end, 4, ms(5_000)), None);
}

#[test]
fn a_clause_mark_in_the_stable_text_commits_up_to_it() {
    let p = params(EarlyCommit::Punctuation);
    let text = "If it rains tomorrow, we will move the meeting to";
    let cut = cut_of(&p, Lang::En, text, 0, 7, ms(0)).expect("cut");
    assert_eq!(cut.reason, EarlyReason::Punctuation);
    assert_eq!(&text[..cut.end], "If it rains tomorrow,");
}

#[test]
fn the_enumeration_comma_is_not_a_punctuation_boundary() {
    let p = params(EarlyCommit::Punctuation);
    let text = "大金字塔是埃及人在公元前三四、三世纪的杰作，后来又被修缮";
    // Stable up to just after the enumeration comma: nothing to cut at.
    let at = text.find('、').expect("mark") + '、'.len_utf8();
    let stable = units(&text[..at]).len();
    assert_eq!(cut_of(&p, Lang::Zh, text, 0, stable, ms(0)), None);
    // The full-width comma still is a boundary, and the cut after it keeps the list whole.
    let stable = units(&text[..text.find('，').expect("comma")]).len();
    let cut = cut_of(&p, Lang::Zh, text, 0, stable, ms(0)).expect("cut");
    assert_eq!(cut.reason, EarlyReason::Punctuation);
    assert_eq!(
        &text[..cut.end],
        "大金字塔是埃及人在公元前三四、三世纪的杰作，"
    );
    for text in [
        "我们先去了北京：然后去上海，最后回家了吧",
        "我们先去了北京；然后去上海，最后回家了吧",
    ] {
        let mark = text.find(['：', '；']).expect("mark");
        let stable = units(&text[..mark]).len();
        let cut = cut_of(&p, Lang::Zh, text, 0, stable, ms(0)).expect("cut");
        assert_eq!(cut.reason, EarlyReason::Punctuation);
    }
}

#[test]
fn the_last_mark_wins_and_a_sentence_mark_is_taken_with_what_precedes_it() {
    let p = params(EarlyCommit::Punctuation);
    let text = "We left early today, and then the rain started. After that we";
    let cut = cut_of(&p, Lang::En, text, 0, 11, ms(0)).expect("cut");
    assert_eq!(
        &text[..cut.end],
        "We left early today, and then the rain started."
    );
}

#[test]
fn a_mark_after_the_stable_end_is_not_used_unless_it_trails_the_last_stable_unit() {
    let p = params(EarlyCommit::Punctuation);
    let text = "We left early today, and then the rain started. After that we";
    // Only "We left early today" is stable: the comma directly after it belongs to it.
    let cut = cut_of(&p, Lang::En, text, 0, 4, ms(0)).expect("cut");
    assert_eq!(&text[..cut.end], "We left early today,");
    // The second mark is beyond the stable text.
    let cut = cut_of(&p, Lang::En, text, 0, 6, ms(0)).expect("cut");
    assert_eq!(&text[..cut.end], "We left early today,");
}

#[test]
fn a_date_is_not_cut_from_its_year_at_the_comma() {
    let p = params(EarlyCommit::Punctuation);
    let text = "the meeting is on November 20th, 2026 at 3:15 p.m. in the main office";
    let n = units(text).len() - 1;
    assert_eq!(cut_of(&p, Lang::En, text, 0, n, ms(0)), None);
    let text = "we met on Monday, and then on November 20th, 2026 at the office";
    let cut = cut_of(&p, Lang::En, text, 0, units(text).len() - 1, ms(0)).expect("cut");
    assert_eq!(&text[..cut.end], "we met on Monday,");
}

#[test]
fn punctuation_mode_does_not_cut_without_a_mark() {
    let p = params(EarlyCommit::Punctuation);
    let text = "we should meet at the office tomorrow morning and then go home";
    assert_eq!(cut_of(&p, Lang::En, text, 0, 11, ms(9_000)), None);
}

#[test]
fn marks_inside_numbers_times_and_abbreviations_are_not_boundaries() {
    let p = params(EarlyCommit::Punctuation);
    for text in [
        "the total is 3.5 million dollars and we",
        "see you at 10:30 tomorrow then we",
        "Dr. Smith said that the plan was fine",
    ] {
        let n = units(text).len() - 1;
        assert_eq!(cut_of(&p, Lang::En, text, 0, n, ms(0)), None, "{text}");
    }
}

#[test]
fn stable_text_with_no_mark_commits_at_the_chunk_minimum() {
    let p = params(EarlyCommit::Chunk);
    let text = "we should meet at the office tomorrow morning and then go home";
    // Five stable units are below the minimum of six.
    assert_eq!(cut_of(&p, Lang::En, text, 0, 5, ms(0)), None);
    // Seven are enough: the cut is after the last unit that is not a function word.
    let cut = cut_of(&p, Lang::En, text, 0, 7, ms(0)).expect("cut");
    assert_eq!(cut.reason, EarlyReason::Chunk);
    assert_eq!(&text[..cut.end], "we should meet at the office tomorrow");
}

#[test]
fn a_chunk_never_ends_on_a_function_word() {
    let p = params(EarlyCommit::Chunk);
    let text = "please send the report to the manager of the";
    let n = units(text).len() - 2;
    let cut = cut_of(&p, Lang::En, text, 0, n, ms(0)).expect("cut");
    let piece = text[..cut.end].trim();
    assert!(
        !["the", "of", "to"].contains(&piece.rsplit(' ').next().unwrap_or("")),
        "{piece}"
    );
}

#[test]
fn numbers_with_their_magnitude_and_unit_are_not_split() {
    let p = params(EarlyCommit::Chunk);
    let text = "the budget for the whole project is about 3.5 million dollars this year";
    for stable in 6..units(text).len() {
        if let Some(cut) = cut_of(&p, Lang::En, text, 0, stable, ms(0)) {
            let head = &text[..cut.end];
            assert!(
                !head.ends_with("3.5") && !head.ends_with("million") && !head.ends_with("about"),
                "{head}"
            );
        }
    }
}

#[test]
fn times_dates_and_names_are_not_split() {
    let p = params(EarlyCommit::Chunk);
    for (text, whole) in [
        (
            "the train leaves tomorrow morning at 10:30 from the main station",
            "10:30",
        ),
        (
            "we agreed to hold the review meeting on March 3rd at the office",
            "March 3rd",
        ),
        (
            "yesterday the manager spoke with John Smith about the contract",
            "John Smith",
        ),
    ] {
        let at = text.find(whole).expect("token");
        for stable in 6..units(text).len() {
            if let Some(cut) = cut_of(&p, Lang::En, text, 0, stable, ms(0)) {
                assert!(
                    cut.end <= at || cut.end >= at + whole.len(),
                    "cut {:?} is inside {whole:?}",
                    &text[..cut.end]
                );
            }
        }
    }
}

#[test]
fn chinese_chunks_do_not_split_a_name_a_number_or_a_phrase_on_a_function_word() {
    let p = params(EarlyCommit::Chunk);
    for (text, whole) in [
        ("明天上午我们要和王建国一起去机场接客人", "王建国"),
        ("这个项目的总预算大概是三点五百万美元左右", "三点五百万美元"),
        ("我们明天下午三点在公司楼下见面然后出发", "下午三点"),
        (
            "会议定在二零二六年十一月二十日下午三点十五分开始",
            "二零二六年十一月二十日",
        ),
        (
            "我们的航班起飞时间是二零二六年十二月二日早上六点四十五分",
            "早上",
        ),
        ("我下周三下午三点不能参加会议请改到周四上午十点钟", "参加"),
        ("请把一百零八号房间的钥匙交给李明不要给别人", "房间的"),
        ("苹果公司的首席执行官是蒂姆库克先生他们说", "首席执行官"),
    ] {
        let at = text.find(whole).expect("token");
        for stable in 8..units(text).len() {
            if let Some(cut) = cut_of(&p, Lang::Zh, text, 0, stable, ms(0)) {
                assert!(
                    cut.end <= at || cut.end >= at + whole.len(),
                    "cut {:?} is inside {whole:?}",
                    &text[..cut.end]
                );
                let last = text[..cut.end].chars().next_back().expect("char");
                assert!(
                    !"的在和跟与把被对给让从向为是有要会能就才都也还又不没将比而或".contains(last)
                );
            }
        }
    }
}

#[test]
fn chinese_chunk_minimum_is_in_characters() {
    let p = params(EarlyCommit::Chunk);
    let text = "我们明天上午去机场接客人然后回公司开会";
    assert_eq!(cut_of(&p, Lang::Zh, text, 0, 7, ms(0)), None);
    let cut = cut_of(&p, Lang::Zh, text, 0, 15, ms(0)).expect("cut");
    assert!(text[..cut.end].chars().count() >= 8);
}

#[test]
fn a_tiny_remainder_is_not_left_behind_but_nothing_left_is_fine() {
    let p = params(EarlyCommit::Chunk);
    let text = "we should meet at the office tomorrow morning ok";
    let all = units(text).len();
    // Stable up to "morning": the only text after the cut would be "ok" (one unit): the cut moves back.
    let cut = cut_of(&p, Lang::En, text, 0, all - 1, ms(0)).expect("cut");
    assert!(
        text[cut.end..].trim().is_empty() || units(&text[cut.end..]).len() >= p.min_remainder,
        "{:?}",
        &text[cut.end..]
    );
    // Everything stable and nothing after: cut at the end.
    let cut = cut_of(&p, Lang::En, text, 0, all, ms(0)).expect("cut");
    assert_eq!(text[cut.end..].trim(), "");
}

#[test]
fn after_the_maximum_wait_a_smaller_chunk_commits_and_a_tiny_tail_is_accepted() {
    let p = params(EarlyCommit::Chunk);
    let text = "we should meet at the office ok";
    assert_eq!(cut_of(&p, Lang::En, text, 0, 4, ms(2_499)), None);
    let cut = cut_of(&p, Lang::En, text, 0, 4, ms(2_500)).expect("cut");
    assert_eq!(cut.reason, EarlyReason::MaxWait);
    assert_eq!(&text[..cut.end], "we should meet");
}

#[test]
fn commits_continue_from_the_committed_offset() {
    let p = params(EarlyCommit::Punctuation);
    let text = "If it rains tomorrow, we will move the meeting to friday, and then we call you.";
    let first = cut_of(&p, Lang::En, text, 0, 8, ms(0)).expect("first");
    assert_eq!(&text[..first.end], "If it rains tomorrow,");
    let n = units(text).len();
    let second = cut_of(&p, Lang::En, text, first.end, n, ms(0)).expect("second");
    // The last mark ends the text and needs the next character to be judged: the cut is the comma.
    assert_eq!(
        piece(text, first.end, Some(second)).as_deref(),
        Some("we will move the meeting to friday,")
    );
}

/// A small deterministic generator (no dependency).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from((self.0 >> 33) % n as u64).unwrap_or(0)
    }
}

const EN_PARTS: [&str; 10] = [
    "we should meet at the office",
    "the total is about 3.5 million dollars",
    "see you at 10:30 tomorrow",
    "on March 3rd we have the review",
    "I talked to John Smith about it",
    "and then we went home",
    "because the weather was bad",
    "please send the report to the manager",
    "it was not a good idea",
    "the new phone costs 1,200 dollars",
];
const EN_PROTECTED: [&str; 5] = ["3.5 million", "10:30", "March 3rd", "John Smith", "1,200"];

const ZH_PARTS: [&str; 8] = [
    "我们明天下午三点在公司见面",
    "王建国说他不同意这个方案",
    "总共是三点五百万美元",
    "如果天气不好我们就改期",
    "请把报告发给我",
    "他们去年在北京开了一家新公司",
    "这件事我们不能马上决定",
    "我昨天和李明一起吃了晚饭",
];
const ZH_PROTECTED: [&str; 3] = ["王建国", "三点五百万美元", "李明"];

fn random_text(rng: &mut Lcg, lang: Lang) -> String {
    let (parts, seps): (&[&str], &[&str]) = match lang {
        Lang::En => (&EN_PARTS, &[" ", ", ", ". ", " "]),
        Lang::Zh => (&ZH_PARTS, &["", "，", "。", ""]),
    };
    let n = 2 + rng.next(3);
    let mut text = String::new();
    for k in 0..n {
        if k > 0 {
            text.push_str(seps[rng.next(seps.len())]);
        }
        text.push_str(parts[rng.next(parts.len())]);
    }
    text
}

/// Grow the stable prefix one unit at a time, committing whenever the rule says so, and check what the
/// pipeline relies on: cuts move forward on char boundaries inside the stable text, no piece is blank,
/// no cut is inside a protected token, and the pieces plus the remainder are the text in comparison units.
fn simulate(p: &CommitParams, lang: Lang, text: &str, guard: usize, protected: &[&str]) {
    let all = units(text);
    let mut committed = 0;
    let mut pieces: Vec<&str> = Vec::new();
    for known in 1..=all.len() {
        let shown = &text[..all[known - 1].end];
        let stable_units = known.saturating_sub(guard);
        let stable = if stable_units == 0 {
            0
        } else {
            all[stable_units - 1].end
        };
        // The audio waited grows with the text: two units per second.
        let waited = Duration::from_millis(known as u64 * 500);
        while let Some(cut) = decide_early_cut(
            p,
            &EarlyInput {
                lang,
                text: shown,
                committed,
                stable,
                waited: waited.saturating_sub(ms(committed as u64)),
            },
        ) {
            assert!(cut.end > committed && cut.end <= shown.len(), "{cut:?}");
            assert!(shown.is_char_boundary(cut.end));
            assert!(
                !shown[committed..cut.end].trim().is_empty(),
                "blank piece in {text:?}"
            );
            let marks = prxvc_media::text_units::punctuation_after(shown, stable).len();
            assert!(
                cut.end <= stable + marks,
                "cut {} beyond the stable text {stable} in {text:?}",
                cut.end
            );
            for token in protected {
                for (at, _) in text.match_indices(token) {
                    assert!(
                        cut.end <= at || cut.end >= at + token.len(),
                        "cut {:?} is inside {token:?} in {text:?}",
                        &text[..cut.end]
                    );
                }
            }
            pieces.push(&text[committed..cut.end]);
            committed = cut.end;
        }
    }
    pieces.push(&text[committed..]);
    let joined: Vec<String> = pieces
        .iter()
        .flat_map(|s| units(s))
        .map(|u| u.key)
        .collect();
    let whole: Vec<String> = all.into_iter().map(|u| u.key).collect();
    assert_eq!(joined, whole, "pieces {pieces:?} of {text:?}");
}

#[test]
fn random_growth_commits_a_partition_of_the_text_and_never_splits_a_protected_token() {
    let mut rng = Lcg(7);
    for early in [EarlyCommit::Punctuation, EarlyCommit::Chunk] {
        let p = params(early);
        for _ in 0..300 {
            let text = random_text(&mut rng, Lang::En);
            simulate(&p, Lang::En, &text, 6, &EN_PROTECTED);
            let text = random_text(&mut rng, Lang::Zh);
            simulate(&p, Lang::Zh, &text, 6, &ZH_PROTECTED);
        }
    }
}

#[test]
fn random_growth_with_a_small_guard_is_also_a_partition() {
    let mut rng = Lcg(99);
    let p = params(EarlyCommit::Chunk);
    for guard in [0, 1, 3] {
        for _ in 0..200 {
            let text = random_text(&mut rng, Lang::En);
            simulate(&p, Lang::En, &text, guard, &EN_PROTECTED);
            let text = random_text(&mut rng, Lang::Zh);
            simulate(&p, Lang::Zh, &text, guard, &ZH_PROTECTED);
        }
    }
}
