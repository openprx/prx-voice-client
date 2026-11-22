//! Re-translation policy: the pure rules that commit a source prefix while the speaker goes on, once
//! its translation has been confirmed as the start of the translations of longer, later hypotheses
//! (incremental-retranslation 4.3 to 4.6). No clock, no I/O: the stages feed it and act on its answers.

use std::time::Duration;

use prxvc_media::script::is_wrong_language_for;
use prxvc_media::text_units::{Unit, is_cjk, punctuation_after, units};
use prxvc_media::{Lang, has_speakable_content, trim_stray_start};

use super::early::{DANGLING_EN, DANGLING_ZH, splits_something};
use crate::filler::is_pure_filler;

/// Thresholds of re-translation, shaped like the `[pipeline]` keys `retranslation` and
/// `retranslate_*` (the composition root fills this; the crate does not read the configuration).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetranslateParams {
    /// The cut rule runs and the draft worker is spawned.
    pub enabled: bool,
    /// Clause translations continue the sentence (engine without context), section 4.6.
    pub continuation: bool,
    /// Confirming reference translations a candidate needs (at least 1).
    pub agreement: u32,
    /// Units a reference hypothesis must extend past the candidate's cut to count (at least 1).
    pub lookahead: usize,
    /// Consecutive hypotheses whose unit keys must agree on the candidate's source prefix (at least 2).
    pub source_agreement: usize,
    /// Units at the end of the agreed prefix that are never part of a candidate.
    pub source_guard: usize,
    /// Fewest Chinese characters in a candidate's new source part.
    pub min_chars: usize,
    /// Fewest words (other scripts) in a candidate's new source part.
    pub min_words: usize,
    /// Least time between two draft submissions of one direction.
    pub interval: Duration,
}

impl RetranslateParams {
    /// Re-translation off, with the configuration defaults for every number. Every existing caller
    /// and test starts from this value.
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            continuation: false,
            agreement: 2,
            lookahead: 2,
            source_agreement: 2,
            source_guard: 1,
            min_chars: 4,
            min_words: 3,
            interval: Duration::from_millis(300),
        }
    }
}

/// Units after which a sentence stands alone again (the recognition and the translation stage apply
/// the same test to the same text).
pub(crate) const MAX_SENTENCE_UNITS: usize = 60;
/// Live candidates one book holds; the oldest is dropped beyond it.
pub(crate) const MAX_CANDIDATES: usize = 6;
/// Reference replies a book waits for at most.
const MAX_PENDING_REFERENCES: usize = 4;
/// Fewest English words in the part a cut speaks.
const MIN_TARGET_WORDS: usize = 2;
/// Fewest Chinese characters in the part a cut speaks.
const MIN_TARGET_CHARS: usize = 3;

/// Join the text of two units of one sentence: no space between two CJK characters, else one space.
/// The recognition and the translation stage both use this, so their cache keys match.
pub(crate) fn join_sentence(a: &str, b: &str) -> String {
    let cjk_side = |c: char| is_cjk(c) || matches!(u32::from(c), 0x3000..=0x303F | 0xFF00..=0xFFEF);
    if a.is_empty() {
        b.to_owned()
    } else if b.is_empty() {
        a.to_owned()
    } else if a.chars().next_back().is_some_and(cjk_side) && b.chars().next().is_some_and(cjk_side)
    {
        format!("{a}{b}")
    } else {
        format!("{a} {b}")
    }
}

/// The text of a unit with the text of a clause that was too short to commit (`carry`) in front.
pub(crate) fn join_carry(carry: &str, text: &str) -> String {
    if carry.is_empty() {
        text.to_owned()
    } else if text.is_empty() {
        carry.to_owned()
    } else {
        format!("{carry} {text}")
    }
}

/// If the units of `spoken` are the first units of `full` (case, punctuation and spacing ignored),
/// the byte offset in `full` just after the last matched unit and the marks that follow it.
/// `Some(0)` for empty `spoken`; `None` when `full` does not start with `spoken`.
pub(crate) fn prefix_end(spoken: &str, full: &str) -> Option<usize> {
    let s = units(spoken);
    if s.is_empty() {
        return Some(0);
    }
    let f = units(full);
    if s.len() > f.len() || s.iter().zip(&f).any(|(a, b)| a.key != b.key) {
        return None;
    }
    let end = f[s.len() - 1].end;
    Some(end + punctuation_after(full, end).len())
}

/// The spoken text of a unit that does not end its source sentence: a trailing sentence mark (or a
/// run of them) becomes a comma in the target language, so the voice does not fall as it would at
/// the end of a sentence.
pub(crate) fn continuation_text(lang: Lang, text: &str) -> String {
    let body = text.trim_end();
    let kept = body.trim_end_matches(SENTENCE_FINAL);
    if kept.len() == body.len() || kept.trim().is_empty() {
        return text.to_owned();
    }
    let comma = match lang {
        Lang::En => ',',
        Lang::Zh => '\u{FF0C}',
    };
    format!("{kept}{comma}{}", &text[body.len()..])
}

/// Marks that end a sentence, as far as a unit that continues a sentence is concerned.
const SENTENCE_FINAL: &[char] = &['.', '?', '!', '\u{3002}', '\u{FF1F}', '\u{FF01}'];

/// Whether `text` ends with a sentence-final mark (closing quotes and brackets after it do not
/// matter): the unit then ends its source sentence.
pub(crate) fn ends_sentence(text: &str) -> bool {
    const CLOSERS: &[char] = &[
        '"', '\'', '\u{201D}', '\u{2019}', ')', '\u{FF09}', '\u{300D}', '\u{300F}',
    ];
    text.trim_end_matches(|c: char| c.is_whitespace() || CLOSERS.contains(&c))
        .ends_with(SENTENCE_FINAL)
}

/// How many leading unit keys all of `recent` have in common (zero for no hypotheses).
pub(crate) fn agreed_prefix_units(recent: &[Vec<String>]) -> usize {
    let Some((first, rest)) = recent.split_first() else {
        return 0;
    };
    first
        .iter()
        .enumerate()
        .take_while(|(i, key)| rest.iter().all(|r| r.get(*i) == Some(key)))
        .count()
}

/// What the book needs from one hypothesis.
pub(crate) struct HypInput<'a> {
    pub source: Lang,
    /// The recogniser's text of the round.
    pub round_text: &'a str,
    /// Bytes of `round_text` that are committed.
    pub committed: usize,
    /// Text of a clause that was too short to commit, joining the next one.
    pub carry: &'a str,
    /// The committed source text of the current source sentence.
    pub sentence: &'a str,
    pub revision: u32,
}

/// Texts to translate for one hypothesis.
pub(crate) struct DraftPlan {
    /// The sentence so far, when its translation is not known yet.
    pub sentence: Option<String>,
    /// The new candidate's text from the start of the sentence.
    pub candidate: Option<String>,
    /// The whole uncommitted hypothesis from the start of the sentence.
    pub reference: String,
}

/// A successful draft translation, as the book needs it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DraftTranslation<'a> {
    pub text: &'a str,
    /// The engine flagged the output as being in the wrong language.
    pub wrong_language: bool,
}

/// A reply of the draft worker: for each source text it was asked for, its translation or `None`
/// when that draft failed.
pub(crate) struct DraftReplyView<'a> {
    /// The hypothesis revision the job was made for.
    pub revision: u32,
    pub results: &'a [(&'a str, Option<DraftTranslation<'a>>)],
}

/// The cut the rule decided on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RetranslateCut {
    /// Byte offset in the round text.
    pub cut: usize,
    /// The unit's source text from the start of the sentence (the cache key of its translation).
    pub source_full: String,
    /// Its verified translation from the start of the sentence.
    pub target_full: String,
}

struct Candidate {
    cut: usize,
    /// The round text up to the cut, to see when the recogniser rewrote it.
    prefix: String,
    source_full: String,
    target: Option<(String, bool)>,
    /// Revision of the hypothesis that created it.
    created: u32,
    agreement: u32,
    /// The last revision whose reference was counted.
    counted: Option<u32>,
}

struct PendingReference {
    revision: u32,
    source: String,
    /// For each candidate (by cut): units the reference extends past it.
    extends: Vec<(usize, usize)>,
}

/// The state of one open segment's re-translation: candidates, source agreement, the translation of
/// the sentence already spoken.
#[derive(Default)]
pub(crate) struct DraftBook {
    candidates: Vec<Candidate>,
    recent: Vec<Vec<String>>,
    pending: Vec<PendingReference>,
    /// The source sentence `spoken` is the translation of; `None` until the next hypothesis adopts it.
    spoken_source: Option<String>,
    spoken: Option<String>,
    last_reference: String,
    /// Candidates dropped because the translation no longer started with what was spoken.
    pub(crate) rejected_spoken: u64,
}

impl DraftBook {
    /// Forget candidates and source agreement (round reset, or after a commit). The translation of
    /// the spoken sentence stays: the sentence goes on across rounds.
    pub(crate) fn reset(&mut self) {
        self.candidates.clear();
        self.recent.clear();
        self.pending.clear();
        self.last_reference.clear();
    }

    /// The translation of the sentence as spoken, after a commit; `None` when it is not known.
    pub(crate) fn set_spoken(&mut self, sentence_target: Option<String>) {
        self.spoken = sentence_target;
        self.spoken_source = None;
    }

    /// Take one hypothesis; returns the texts to translate for it, if any.
    pub(crate) fn on_hypothesis(
        &mut self,
        p: &RetranslateParams,
        i: &HypInput<'_>,
    ) -> Option<DraftPlan> {
        let sentence = if units(i.sentence).len() > MAX_SENTENCE_UNITS {
            ""
        } else {
            i.sentence
        };
        self.adopt_sentence(sentence);
        let committed = i.committed.min(i.round_text.len());
        let region = i.round_text.get(committed..).unwrap_or("");
        let all = units(region);
        self.candidates
            .retain(|c| i.round_text.starts_with(&c.prefix));
        self.push_recent(p, &all);
        let candidate = self.add_candidate(p, i, sentence, region, &all);
        let reference_text = join_sentence(sentence, &join_carry(i.carry, region.trim()));
        if self.candidates.is_empty() || region.trim().is_empty() {
            return None;
        }
        let sentence_unknown = !sentence.is_empty() && self.spoken.is_none();
        if candidate.is_none() && !sentence_unknown && reference_text == self.last_reference {
            return None;
        }
        let extends: Vec<(usize, usize)> = self
            .candidates
            .iter()
            .map(|c| {
                let upto = units(&i.round_text[committed..c.cut]).len();
                (c.cut, all.len().saturating_sub(upto))
            })
            .collect();
        self.pending.push(PendingReference {
            revision: i.revision,
            source: reference_text.clone(),
            extends,
        });
        if self.pending.len() > MAX_PENDING_REFERENCES {
            self.pending.remove(0);
        }
        self.last_reference.clone_from(&reference_text);
        Some(DraftPlan {
            sentence: sentence_unknown.then(|| sentence.to_owned()),
            candidate,
            reference: reference_text,
        })
    }

    /// Take the reply to a job; returns the cut that is now decided, if any.
    pub(crate) fn on_reply(
        &mut self,
        p: &RetranslateParams,
        target: Lang,
        reply: &DraftReplyView<'_>,
    ) -> Option<RetranslateCut> {
        for (source, result) in reply.results {
            self.record(source, result.as_ref());
        }
        for (source, result) in reply.results {
            if let Some(t) = result {
                self.count_reference(p.lookahead.max(1), reply.revision, source, t.text);
            }
        }
        let cut = self.decide(p, target)?;
        self.spoken = Some(cut.target_full.clone());
        self.spoken_source = Some(cut.source_full.clone());
        self.candidates.clear();
        self.recent.clear();
        self.pending.clear();
        self.last_reference.clear();
        Some(cut)
    }

    /// The spoken translation is valid for `sentence` only: a sentence that changed outside the
    /// book (a clause committed by another rule) makes it stale.
    fn adopt_sentence(&mut self, sentence: &str) {
        if sentence.is_empty() {
            self.spoken = Some(String::new());
            self.spoken_source = Some(String::new());
            return;
        }
        match (&self.spoken_source, &self.spoken) {
            (None, Some(_)) => self.spoken_source = Some(sentence.to_owned()),
            (Some(s), _) if s == sentence => {}
            _ => {
                self.spoken = None;
                self.spoken_source = Some(sentence.to_owned());
            }
        }
    }

    fn push_recent(&mut self, p: &RetranslateParams, all: &[Unit]) {
        self.recent
            .push(all.iter().map(|u| u.key.clone()).collect());
        let keep = p.source_agreement.max(2);
        if self.recent.len() > keep {
            self.recent.remove(0);
        }
    }

    /// A new candidate for the agreed prefix of the recent hypotheses, if there is one.
    fn add_candidate(
        &mut self,
        p: &RetranslateParams,
        i: &HypInput<'_>,
        sentence: &str,
        region: &str,
        all: &[Unit],
    ) -> Option<String> {
        if self.recent.len() < p.source_agreement.max(2) {
            return None;
        }
        let mut n = agreed_prefix_units(&self.recent).saturating_sub(p.source_guard);
        while n > 0 && splits_something(region, all, n) {
            n -= 1;
        }
        if n == 0 {
            return None;
        }
        let end = all[n - 1].end;
        let upto = end + punctuation_after(region, end).len();
        let cut = i.committed.min(i.round_text.len()) + upto;
        if self.candidates.iter().any(|c| c.cut >= cut) {
            return None;
        }
        let piece = region[..upto].trim();
        let (chars, words) = count_units(&all[..n]);
        if (chars < p.min_chars && words < p.min_words) || is_pure_filler(i.source, piece) {
            return None;
        }
        let source_full = join_sentence(sentence, &join_carry(i.carry, piece));
        if self.candidates.len() >= MAX_CANDIDATES {
            self.candidates.remove(0);
        }
        self.candidates.push(Candidate {
            cut,
            prefix: i.round_text[..cut].to_owned(),
            source_full: source_full.clone(),
            target: None,
            created: i.revision,
            agreement: 0,
            counted: None,
        });
        Some(source_full)
    }

    /// Store a draft result where it belongs.
    fn record(&mut self, source: &str, result: Option<&DraftTranslation<'_>>) {
        if self.spoken_source.as_deref() == Some(source)
            && !source.is_empty()
            && let Some(t) = result
        {
            self.spoken = Some(t.text.to_owned());
        }
        if let Some(at) = self.candidates.iter().position(|c| c.source_full == source) {
            match result {
                Some(t) => {
                    self.candidates[at].target = Some((t.text.to_owned(), t.wrong_language));
                }
                None => {
                    self.candidates.remove(at);
                }
            }
        }
    }

    /// Count a reference translation once per revision for each candidate it reaches far enough past.
    fn count_reference(&mut self, lookahead: usize, revision: u32, source: &str, text: &str) {
        let Some(pending) = self
            .pending
            .iter()
            .find(|r| r.revision == revision && r.source == source)
        else {
            return;
        };
        for c in &mut self.candidates {
            let Some((_, extends)) = pending.extends.iter().find(|(cut, _)| *cut == c.cut) else {
                continue;
            };
            let Some((t_c, _)) = &c.target else {
                continue;
            };
            if revision < c.created
                || c.counted.is_some_and(|r| revision <= r)
                || *extends < lookahead
            {
                continue;
            }
            c.counted = Some(revision);
            c.agreement = if prefix_end(t_c, text).is_some() {
                c.agreement + 1
            } else {
                0
            };
        }
    }

    /// The longest candidate that has enough agreement and whose spoken part is still valid and
    /// whose new part can be spoken. A candidate whose translation no longer starts with what was
    /// spoken is dropped.
    fn decide(&mut self, p: &RetranslateParams, target: Lang) -> Option<RetranslateCut> {
        let wanted = p.agreement.max(1);
        let spoken = self.spoken.clone();
        let mut best: Option<usize> = None;
        let mut dropped = Vec::new();
        for (at, c) in self.candidates.iter().enumerate() {
            let Some((t_c, wrong)) = &c.target else {
                continue;
            };
            if c.agreement < wanted {
                continue;
            }
            let Some(t_s) = &spoken else {
                continue;
            };
            let Some(from) = prefix_end(t_s, t_c) else {
                dropped.push(at);
                continue;
            };
            if !speakable_new_part(target, t_c, from, *wrong) {
                continue;
            }
            if best.is_none_or(|b| self.candidates[b].cut < c.cut) {
                best = Some(at);
            }
        }
        self.rejected_spoken += dropped.len() as u64;
        let chosen = best.map(|at| {
            let c = &self.candidates[at];
            RetranslateCut {
                cut: c.cut,
                source_full: c.source_full.clone(),
                target_full: c.target.as_ref().map(|t| t.0.clone()).unwrap_or_default(),
            }
        });
        if chosen.is_none() {
            for at in dropped.into_iter().rev() {
                self.candidates.remove(at);
            }
        }
        chosen
    }
}

/// Fewest Chinese characters and words (other scripts) of `units`.
fn count_units(all: &[Unit]) -> (usize, usize) {
    let chars = all
        .iter()
        .filter(|u| u.key.chars().next().is_some_and(is_cjk))
        .count();
    (chars, all.len() - chars)
}

/// Whether `full[from..]` is a part a voice can speak on its own: enough units, something to say,
/// not in the wrong language, not ending on a word a phrase cannot end on.
fn speakable_new_part(target: Lang, full: &str, from: usize, wrong_flag: bool) -> bool {
    let new = trim_stray_start(&full[from..]);
    if wrong_flag || is_wrong_language_for(full, target) || !has_speakable_content(new) {
        return false;
    }
    let all = units(new);
    let (chars, words) = count_units(&all);
    let Some(last) = all.last() else {
        return false;
    };
    match target {
        Lang::En => words >= MIN_TARGET_WORDS && !DANGLING_EN.contains(&last.key.as_str()),
        Lang::Zh => {
            chars >= MIN_TARGET_CHARS
                && !last
                    .key
                    .chars()
                    .next()
                    .is_some_and(|c| DANGLING_ZH.contains(&c))
        }
    }
}

#[cfg(test)]
mod tests;
