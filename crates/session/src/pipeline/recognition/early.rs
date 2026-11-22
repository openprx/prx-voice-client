//! Recognition stage, early commits: cuts from the recogniser's stable prefix while the speaker goes on.

use std::time::Duration;

use super::{OpenSegment, Out, RecognitionStage};
use crate::commit_policy::{EarlyInput, decide_early_cut};

impl RecognitionStage {
    /// Commit text from the recogniser's stable prefix while the speaker goes on (`early_commit`).
    /// Not with a refine pass: it decodes the audio of a clause again and would not match a cut that
    /// the stable text chose.
    pub(super) fn commit_early(&mut self, waited: Duration, out: &mut Out) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let (Some(hyp), Some(stable)) = (open.hyp.as_ref(), stable_cut(open)) else {
            return;
        };
        let input = EarlyInput {
            lang: self.p.source,
            text: &hyp.text,
            committed: open.committed_bytes,
            stable,
            waited,
        };
        if let Some(cut) = decide_early_cut(&self.p.commit, &input) {
            self.commit_prefix(cut.end, true, out);
        }
    }
}

/// The largest index `<= at` that is a char boundary of `text`.
pub(super) fn floor_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// Where the stable, uncommitted text can be cut: just after its last word,
/// never inside an ASCII word. `None` when there is nothing to cut.
pub(super) fn stable_cut(open: &OpenSegment) -> Option<usize> {
    let hyp = open.hyp.as_ref()?;
    let text = &hyp.text;
    let mut cut = floor_boundary(text, hyp.stable_prefix_len);
    if cut <= open.committed_bytes {
        return None;
    }
    let ascii_word = |c: char| c.is_ascii_alphanumeric();
    if cut < text.len() {
        let before = text[..cut].chars().next_back();
        let after = text[cut..].chars().next();
        if before.is_some_and(ascii_word) && after.is_some_and(ascii_word) {
            cut = text[..cut].rfind(|c: char| !ascii_word(c)).map_or(0, |i| {
                i + text[i..].chars().next().map_or(1, char::len_utf8)
            });
        }
    }
    (cut > open.committed_bytes && !text[open.committed_bytes..cut].trim().is_empty())
        .then_some(cut)
}
