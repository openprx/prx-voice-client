//! Early commit: where the stable, not yet committed text of a segment can be cut while the speaker is still talking (pipeline-design 3.2.1, 5.2).
//!
//! Waiting for the end of the sentence costs the whole sentence in latency, and the recogniser's
//! partials rarely end a sentence before the speaker pauses. The recogniser declares a prefix of its
//! text stable (it never retracts it), so the pipeline may commit translatable units from that prefix
//! at once. This module is the pure rule that picks the cut: no clock, no I/O, no model calls.
//!
//! The cut is the LAST safe boundary of the stable uncommitted text, by priority:
//!
//! 1. a punctuation boundary, found by the clause rule of `prxvc_media::clause_split` (sentence-final
//!    and clause-level marks, with its protections for numbers, times and abbreviations, and its
//!    minimum lengths: 2 words or 4 characters before a sentence mark, 4 words or 6 characters
//!    before a clause mark);
//! 2. with no punctuation, once the stable uncommitted text has at least a minimum size
//!    (`chunk_words` or `chunk_chars`, in comparison units), a unit boundary that does not split what
//!    must stay together: a number with its magnitude and unit (`3.5 million`, `10:30`, `March
//!    3rd`), a name (`John Smith`, `王建国`), a phrase left hanging on a function word (`of the`,
//!    `在`), and that leaves either nothing or at least `min_remainder` units after it, so that a tiny
//!    tail is not left to be translated alone;
//! 3. after `max_wait` without a commit the minimum size is halved and the remainder rule is dropped,
//!    so that long speech with no punctuation and slow stabilising text still commits.
//!
//! The boundaries are on the recogniser's own text, so what is committed is a prefix of it: committed
//! text is never retracted, and the final text of the segment is aligned against it as before.

use std::time::Duration;

use prxvc_media::text_units::{Unit, is_cjk, punctuation_after, units};
use prxvc_media::{Lang, last_clause_end_within};

use super::CommitParams;

/// When translatable units are committed (`pipeline.early_commit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EarlyCommit {
    /// Only at the end of the segment (a pause), the clause pause and the length cut.
    SegmentEnd,
    /// Also from the stable prefix at sentence-final and clause-level marks.
    Punctuation,
    /// Also from the stable prefix once it has a minimum size, when no mark is available.
    Chunk,
}

impl EarlyCommit {
    /// The configuration spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SegmentEnd => "segment_end",
            Self::Punctuation => "punctuation",
            Self::Chunk => "chunk",
        }
    }

    /// The mode a configuration spelling names.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "segment_end" => Some(Self::SegmentEnd),
            "punctuation" => Some(Self::Punctuation),
            "chunk" => Some(Self::Chunk),
            _ => None,
        }
    }
}

/// What the recognition side knows when it asks for an early cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EarlyInput<'a> {
    /// The language of the text.
    pub lang: Lang,
    /// The recogniser's text of the current round, committed part included.
    pub text: &'a str,
    /// Bytes of `text` that are committed.
    pub committed: usize,
    /// Bytes of `text` that the recogniser declared stable, on a unit boundary.
    pub stable: usize,
    /// Audio since the last commit (the clock of the maximum wait).
    pub waited: Duration,
}

/// Why an early cut was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EarlyReason {
    /// A sentence-final or clause-level mark.
    Punctuation,
    /// The stable text reached the minimum chunk size.
    Chunk,
    /// The maximum wait passed and a smaller chunk was taken.
    MaxWait,
}

/// A cut: commit `text[committed..end]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EarlyCut {
    /// Byte offset in the text just after the unit (and the marks that trail it).
    pub end: usize,
    /// Why.
    pub reason: EarlyReason,
}

/// Decide whether stable uncommitted text commits now.
///
/// Returns `None` in `SegmentEnd` mode, when nothing stable is uncommitted, or when no safe boundary
/// exists yet. One cut is returned at a time; the caller commits it and asks again.
pub fn decide_early_cut(p: &CommitParams, i: &EarlyInput<'_>) -> Option<EarlyCut> {
    if p.early == EarlyCommit::SegmentEnd || i.stable <= i.committed || i.stable > i.text.len() {
        return None;
    }
    let region = &i.text[i.committed..];
    let limit = i.stable - i.committed;
    // A mark directly after the last stable unit belongs to it.
    let marks = punctuation_after(region, limit).len();
    let mut upto = limit + marks;
    while let Some(end) = last_clause_end_within(i.lang, region, upto) {
        if !splits_number_series(region, end) {
            return Some(EarlyCut {
                end: i.committed + end,
                reason: EarlyReason::Punctuation,
            });
        }
        // A date and its year (`November 20th, 2026`) or a list of figures stay together: look for
        // an earlier mark.
        upto = region[..end].char_indices().next_back().map_or(0, |(at, _)| at);
    }
    if p.early != EarlyCommit::Chunk {
        return None;
    }
    chunk_cut(p, i, region, limit)
}

/// Whether the mark that ends `region[..end]` stands between two figures (`20th, 2026`, `3, 4`).
fn splits_number_series(region: &str, end: usize) -> bool {
    let before = region[..end]
        .trim_end_matches(|c: char| !c.is_alphanumeric())
        .rsplit(|c: char| !c.is_alphanumeric() && c != '.')
        .next()
        .unwrap_or("");
    let after = region[end..].trim_start();
    before.chars().any(|c| c.is_ascii_digit())
        && region[..end].ends_with([',', '\u{FF0C}'])
        && after.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn chunk_cut(p: &CommitParams, i: &EarlyInput<'_>, region: &str, limit: usize) -> Option<EarlyCut> {
    let all = units(region);
    let stable_units = all.iter().take_while(|u| u.end <= limit).count();
    let chunk = chunk_size(p, i.lang);
    let late = i.waited >= p.early_max_wait;
    let min = if late { (chunk / 2).max(2) } else { chunk };
    if stable_units < min {
        return None;
    }
    for k in (min..=stable_units).rev() {
        let remainder = all.len() - k;
        if !late && remainder > 0 && remainder < p.min_remainder {
            continue;
        }
        if splits_something(region, &all, k) {
            continue;
        }
        // Chinese words are not marked: take a boundary that is likely to be one (after a final
        // particle, before a conjunction or pronoun), or any safe one once the text has grown past
        // the minimum by `ZH_SLACK` characters.
        if i.lang == Lang::Zh && k < min + ZH_SLACK && !is_soft_zh(&all, k) {
            continue;
        }
        let end = all[k - 1].end;
        let end = end + punctuation_after(region, end).len();
        return Some(EarlyCut {
            end: i.committed + end,
            reason: if k >= chunk {
                EarlyReason::Chunk
            } else {
                EarlyReason::MaxWait
            },
        });
    }
    None
}

/// The minimum chunk of stable text for text in `lang`, in comparison units.
pub fn chunk_size(p: &CommitParams, lang: Lang) -> usize {
    match lang {
        Lang::Zh => p.chunk_chars,
        Lang::En => p.chunk_words,
    }
    .max(1)
}

/// Extra stable characters a Chinese chunk may wait for a boundary that is likely to be a word boundary.
const ZH_SLACK: usize = 4;

/// Whether a cut after the first `k` units is likely to fall between Chinese words.
fn is_soft_zh(all: &[Unit], k: usize) -> bool {
    let single = |idx: usize| -> Option<char> {
        let mut it = all.get(idx)?.key.chars();
        match (it.next(), it.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    };
    single(k - 1).is_some_and(|c| FINAL_PARTICLES.contains(&c))
        || single(k).is_some_and(|c| CLAUSE_STARTERS.contains(&c))
        || all
            .get(k)
            .is_some_and(|u| u.key.len() > 3 && !is_cjk_key(&u.key))
}

fn is_cjk_key(key: &str) -> bool {
    key.chars().next().is_some_and(is_cjk)
}

/// The text of unit `idx` as written, without the marks before it.
fn written<'a>(region: &'a str, all: &[Unit], idx: usize) -> &'a str {
    let from = if idx == 0 { 0 } else { all[idx - 1].end };
    region[from..all[idx].end].trim_start_matches(|c: char| !c.is_alphanumeric())
}

/// Whether a cut after the first `k` units would split a token that must stay whole.
pub(super) fn splits_something(region: &str, all: &[Unit], k: usize) -> bool {
    let prev = &all[k - 1].key;
    if prev.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    if prev.chars().next().is_some_and(is_cjk) {
        return splits_chinese(all, k);
    }
    if NUMBER_WORDS.contains(&prev.as_str())
        || MONTHS.contains(&prev.as_str())
        || DANGLING_EN.contains(&prev.as_str())
    {
        return true;
    }
    // Two capitalised words in a row are a name; so is a capitalised last word whose successor has
    // not arrived (the later words of the name may follow).
    let capital = |idx: usize| {
        let w = written(region, all, idx);
        let key = &all[idx].key;
        w.chars().next().is_some_and(char::is_uppercase) && key != "i" && key != "a"
    };
    capital(k - 1) && (all.get(k).is_none() || capital(k))
}

fn splits_chinese(all: &[Unit], k: usize) -> bool {
    let ch = |idx: usize| -> Option<char> {
        let key = &all.get(idx)?.key;
        let mut it = key.chars();
        match (it.next(), it.next()) {
            (Some(c), None) if is_cjk(c) => Some(c),
            _ => None,
        }
    };
    let Some(prev) = ch(k - 1) else {
        return false;
    };
    if NUMERALS.contains(&prev) || DANGLING_ZH.contains(&prev) {
        return true;
    }
    // A particle belongs to the word before it (`房间|的`).
    if ch(k).is_some_and(|n| ATTACHED_PARTICLES.contains(&n)) {
        return true;
    }
    // Two characters of one common word stay together (`早|上`, `参|加`).
    if let (Some(a), Some(b)) = (ch(k - 1), ch(k)) {
        let mut pair = String::new();
        pair.push(a);
        pair.push(b);
        if COMPOUNDS_ZH.contains(&pair.as_str()) {
            return true;
        }
    }
    // A date goes on after its year and month (`二零二六年|十一月`).
    if matches!(prev, '年' | '月') && ch(k).is_some_and(|n| NUMERALS.contains(&n)) {
        return true;
    }
    // A run of measure words after a number (`百万美元`) stays together.
    let measure_run = MEASURES.contains(&prev)
        && all
            .get(k)
            .is_none_or(|_| ch(k).is_some_and(|n| MEASURES.contains(&n)))
        && k >= 2
        && ch(k - 2).is_some_and(|b| NUMERALS.contains(&b) || MEASURES.contains(&b));
    if measure_run {
        return true;
    }
    // A name: a surname and the one or two characters after it.
    let name_cut = |surname_at: usize| {
        ch(surname_at).is_some_and(|c| SURNAMES.contains(&c))
            && (surname_at + 1..k).all(|j| ch(j).is_some())
    };
    name_cut(k - 1) || (k >= 2 && name_cut(k - 2))
}

const NUMBER_WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "million",
    "billion",
    "trillion",
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "half",
    "quarter",
    "dozen",
    "couple",
    "few",
    "several",
];

const MONTHS: &[&str] = &[
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// Words a phrase cannot end on: a cut after one leaves a fragment that cannot be translated alone.
pub(super) const DANGLING_EN: &[&str] = &[
    "the", "a", "an", "of", "to", "in", "on", "at", "for", "with", "by", "from", "about", "into",
    "over", "under", "and", "but", "or", "so", "if", "that", "which", "who", "whom", "whose",
    "because", "as", "than", "when", "while", "where", "my", "your", "his", "her", "our", "their",
    "this", "these", "those", "its", "is", "are", "was", "were", "be", "been", "will", "would",
    "can", "could", "should", "shall", "may", "might", "must", "have", "has", "had", "do", "does",
];

/// Characters that end a phrase and are never the start of a word.
const FINAL_PARTICLES: &[char] = &['了', '吧', '呢', '吗', '啊', '呀', '嘛', '过'];

/// Characters that start a conjunction, preposition or pronoun, so a cut before one is between words.
const CLAUSE_STARTERS: &[char] = &[
    '但', '所', '因', '如', '然', '而', '并', '可', '或', '还', '就', '虽', '和', '跟', '与', '把',
    '被', '在', '从', '对', '给', '让', '为', '我', '你', '他', '她', '它', '这', '那', '请',
];

const NUMERALS: &[char] = &[
    '零', '〇', '一', '二', '三', '四', '五', '六', '七', '八', '九', '十', '百', '千', '万', '亿',
    '两', '半', '点', '第',
];

const MEASURES: &[char] = &[
    '个', '年', '月', '日', '号', '点', '分', '秒', '时', '块', '元', '钱', '岁', '位', '次', '倍',
    '度', '件', '只', '条', '张', '本', '天', '周', '美', '欧', '英', '镑',
];

/// Frequent two-character words: a cut between their characters splits the word.
const COMPOUNDS_ZH: &[&str] = &[
    "早上", "上午", "下午", "中午", "晚上", "今天", "明天", "昨天", "后天", "前天", "去年", "今年",
    "明年", "这个", "那个", "什么", "因为", "所以", "但是", "如果", "可以", "已经", "现在", "时候",
    "时间", "会议", "项目", "公司", "预算", "钥匙", "航班", "起飞", "房间", "电话", "号码", "账户",
    "转账", "经理", "女士", "先生", "医生", "同学", "朋友", "孩子", "问题", "工作", "国家", "政府",
    "美国", "中国", "北京", "上海", "国际", "开始", "结束", "告诉", "知道", "觉得", "需要", "应该",
    "可能", "一起", "一定", "不是", "没有", "不能", "不会", "参加", "批准", "改期", "通知", "大家",
    "我们", "你们", "他们", "她们", "自己", "一些", "一下", "这样", "那样", "为了", "关于", "对于",
    "以及", "或者", "而且", "并且", "然后", "不过", "虽然", "还是", "就是", "只是", "星期", "周末",
    "分钟", "小时", "交给", "发给", "给我", "改到", "转到", "前往", "离开", "回到", "到达", "到来",
    "出发", "见面", "行官", "首席", "同意", "决定", "讨论", "安排", "确认", "取消", "收到", "联系",
];

/// Particles that attach to the word before them: a cut before one leaves it alone at the start of a unit.
const ATTACHED_PARTICLES: &[char] = &['的', '地', '得', '了', '着', '过', '吗', '呢', '吧', '啊', '嘛'];

/// Characters a Chinese phrase cannot end on.
pub(super) const DANGLING_ZH: &[char] = &[
    '的', '在', '和', '跟', '与', '把', '被', '对', '给', '让', '从', '向', '为', '是', '有', '要',
    '会', '能', '就', '才', '都', '也', '还', '又', '不', '没', '将', '比', '而', '或',
];

const SURNAMES: &[char] = &[
    '王', '李', '张', '刘', '陈', '杨', '黄', '赵', '吴', '周', '徐', '孙', '马', '朱', '胡', '郭',
    '何', '罗', '郑', '梁', '谢', '宋', '唐', '许', '韩', '冯', '邓', '曹', '彭', '曾', '萧', '董',
    '潘', '袁', '蔡', '蒋', '杜', '叶', '程', '魏', '苏', '吕', '卢', '姚', '沈', '钟', '姜', '崔',
    '谭', '陆', '范', '汪', '廖', '贾', '夏', '邹', '熊', '孟', '秦', '邱', '侯', '江', '尹', '薛',
    '段', '雷', '龙', '黎', '陶', '贺', '顾', '龚', '邵', '林', '高', '郝', '毛', '史', '田', '石',
    '金', '方', '白', '余', '丁', '任', '韦', '傅', '钱', '严', '洪', '武', '孔', '赖', '覃',
];

#[cfg(test)]
mod tests;
