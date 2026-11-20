//! Commit policy: the pure rules that decide when text becomes a clause or goes to synthesis (pipeline-design 3.2).

use std::time::Duration;

use prxvc_media::{Lang, first_clause_end};

/// Thresholds of the commit rules, shaped like the `[pipeline]` configuration keys.
///
/// The crate does not depend on `prxvc-config`: the composition root fills
/// this from the configuration (`clause_pause_ms`, `segment_end_pause_ms`,
/// `long_segment_after_s`, `long_segment_pause_ms`, `max_clause_s`,
/// `max_segment_s`, `commit_on_stable_partial`, `stable_partial_ms`,
/// `split_target_clauses`, `speculative_mt`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitParams {
    /// Silence that commits a clause inside a segment.
    pub clause_pause: Duration,
    /// Silence that ends a segment.
    pub segment_end_pause: Duration,
    /// Segment audio length after which the shorter pause applies.
    pub long_segment_after: Duration,
    /// Segment-end silence once the segment is long.
    pub long_segment_pause: Duration,
    /// Uncommitted audio length that forces a cut.
    pub max_clause: Duration,
    /// Segment length that forces the segment to end.
    pub max_segment: Duration,
    /// Fewest Chinese characters that make a clause on a pause.
    pub min_clause_chars: usize,
    /// Fewest English words that make a clause on a pause.
    pub min_clause_words: usize,
    /// Commit on a stable sentence-final mark (milestone M3).
    pub commit_on_stable_partial: bool,
    /// How long the prefix must stay unchanged for that.
    pub stable_partial: Duration,
    /// Send the first target sentence to synthesis early (milestone M3).
    pub split_target_clauses: bool,
    /// Tokens that must follow a sentence-final mark before an early split.
    pub split_min_tokens_after: u32,
}

impl Default for CommitParams {
    /// The configuration defaults of pipeline-design 7.1.
    fn default() -> Self {
        Self {
            clause_pause: Duration::from_millis(300),
            segment_end_pause: Duration::from_millis(600),
            long_segment_after: Duration::from_secs(8),
            long_segment_pause: Duration::from_millis(300),
            max_clause: Duration::from_secs(8),
            max_segment: Duration::from_secs(25),
            min_clause_chars: 4,
            min_clause_words: 2,
            commit_on_stable_partial: false,
            stable_partial: Duration::from_millis(400),
            split_target_clauses: false,
            split_min_tokens_after: 4,
        }
    }
}

/// What the recognition side knows at one instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecognitionInput<'a> {
    /// Silence since speech last stopped (zero while speech goes on).
    pub silence: Duration,
    /// The text recognised since the last committed boundary.
    pub uncommitted_text: &'a str,
    /// Audio since the last committed boundary.
    pub uncommitted_audio: Duration,
    /// Audio since the segment began.
    pub segment_audio: Duration,
    /// How long the partial's text has stayed unchanged.
    pub text_unchanged_for: Duration,
    /// Whether the partial has a stable word boundary to cut at.
    pub has_stable_word_boundary: bool,
}

/// Where a length cut falls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutPoint {
    /// At the last stable word boundary of the partial.
    LastStableWord,
    /// At the lowest-VAD-probability frame of the last second.
    LowestVadFrame,
}

/// Why a clause was committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClauseReason {
    /// A pause inside the segment, with enough text.
    ClausePause,
    /// The uncommitted audio reached its maximum.
    LengthCut(CutPoint),
    /// A sentence-final mark stayed unchanged (M3).
    StablePunctuation,
}

/// Why a segment ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// The segment-end silence elapsed.
    SegmentPause,
    /// The segment reached its maximum length.
    MaxSegmentLength,
}

/// The decision of the recognition commit rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitDecision {
    /// Keep listening.
    Wait,
    /// Commit the uncommitted text as a clause; the segment goes on.
    CommitClause(ClauseReason),
    /// End the segment, committing the uncommitted text as its last clause if any.
    EndSegment {
        /// Whether there is uncommitted text to commit first.
        commit_clause: bool,
        /// Why the segment ends.
        reason: EndReason,
    },
}

/// Decide whether recognised text commits now (PD-3.2.1).
///
/// All rules are evaluated at one instant; when several hold, the strongest
/// wins: segment end (by length, then by pause), clause length cut, clause
/// pause, stable punctuation. A clause pause with too little text is
/// ignored and the text joins the next clause; a segment end always commits
/// whatever text exists, so no recognised text is lost.
pub fn decide_recognition_commit(p: &CommitParams, i: &RecognitionInput<'_>) -> CommitDecision {
    let has_text = !i.uncommitted_text.trim().is_empty();
    if i.segment_audio >= p.max_segment {
        return end(has_text, EndReason::MaxSegmentLength);
    }
    let end_pause = if i.segment_audio >= p.long_segment_after {
        p.long_segment_pause
    } else {
        p.segment_end_pause
    };
    if i.silence >= end_pause {
        return end(has_text, EndReason::SegmentPause);
    }
    if has_text && i.uncommitted_audio >= p.max_clause {
        let cut = if i.has_stable_word_boundary {
            CutPoint::LastStableWord
        } else {
            CutPoint::LowestVadFrame
        };
        return CommitDecision::CommitClause(ClauseReason::LengthCut(cut));
    }
    if i.silence >= p.clause_pause && is_clause_sized(p, i.uncommitted_text) {
        return CommitDecision::CommitClause(ClauseReason::ClausePause);
    }
    if p.commit_on_stable_partial
        && ends_sentence(i.uncommitted_text)
        && i.text_unchanged_for >= p.stable_partial
    {
        return CommitDecision::CommitClause(ClauseReason::StablePunctuation);
    }
    CommitDecision::Wait
}

fn end(commit_clause: bool, reason: EndReason) -> CommitDecision {
    CommitDecision::EndSegment {
        commit_clause,
        reason,
    }
}

/// Whether `text` has at least the configured Chinese characters or English words.
pub fn is_clause_sized(p: &CommitParams, text: &str) -> bool {
    let (chars, words) = count_units(text);
    chars >= p.min_clause_chars || words >= p.min_clause_words
}

/// Chinese characters and English words in `text`.
fn count_units(text: &str) -> (usize, usize) {
    let mut chars = 0;
    let mut words = 0;
    let mut in_word = false;
    for c in text.chars() {
        if is_cjk(c) {
            chars += 1;
            in_word = false;
        } else if c.is_alphanumeric() {
            if !in_word {
                words += 1;
            }
            in_word = true;
        } else {
            in_word = false;
        }
    }
    (chars, words)
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
}

const SENTENCE_FINAL: [char; 7] = ['。', '！', '？', '.', '!', '?', '…'];

fn is_final_mark(c: char) -> bool {
    SENTENCE_FINAL.contains(&c)
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .chars()
        .next_back()
        .is_some_and(is_final_mark)
}

/// The translation stream of one clause at one instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TranslationProgress<'a> {
    /// The language of `text` (the clause rule has language-specific minimum lengths).
    pub lang: Lang,
    /// Whether the translation stream is complete.
    pub finished: bool,
    /// The translated text so far.
    pub text: &'a str,
    /// Tokens produced after the first sentence-final mark (counted by the translator).
    pub tokens_after_first_sentence: u32,
}

/// The decision of the synthesis commit rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TtsCommit {
    /// Not yet: wait for more of the translation.
    Wait,
    /// Synthesise the complete translated clause.
    Whole,
    /// Synthesise the first clause now: the text up to `end`, a byte offset
    /// after its closing mark. The clause is the one `prxvc_media::ClauseSplitter`
    /// finds (a sentence or a clause-level piece; never inside `3.5`, `Dr.`
    /// or `U.S.`).
    FirstSentence {
        /// Byte offset just after the mark that ends the first clause.
        end: usize,
    },
}

/// Decide whether translated text goes to synthesis now (PD-3.2.3, PD-3.2.4).
///
/// Uncommitted source text is never synthesised: this function only sees
/// translations of committed clauses. Synthesis takes one complete clause,
/// except with `split_target_clauses` where the first clause goes as soon as
/// the clause rule of `prxvc_media::clause_split` finds its end and enough
/// tokens have followed. That rule is the only one: there is no separate
/// punctuation test here.
pub fn decide_tts_commit(p: &CommitParams, t: &TranslationProgress<'_>) -> TtsCommit {
    if t.finished {
        return if t.text.trim().is_empty() {
            TtsCommit::Wait
        } else {
            TtsCommit::Whole
        };
    }
    if !p.split_target_clauses || t.tokens_after_first_sentence < p.split_min_tokens_after {
        return TtsCommit::Wait;
    }
    match first_clause_end(t.lang, t.text) {
        Some(end) => TtsCommit::FirstSentence { end },
        None => TtsCommit::Wait,
    }
}

/// What to do with a speculative translation once the clause text is committed (PD-3.2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speculation {
    /// The committed text equals the speculated text: use the translation.
    Use,
    /// The text differs: cancel the speculation and translate the committed text.
    CancelAndRerun,
}

/// Compare the speculated source text with the committed clause text.
pub fn check_speculation(speculated: &str, committed: &str) -> Speculation {
    if speculated == committed {
        Speculation::Use
    } else {
        Speculation::CancelAndRerun
    }
}

#[cfg(test)]
mod tests;
