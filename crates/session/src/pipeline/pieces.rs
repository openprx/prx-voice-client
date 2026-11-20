//! Cuts the token stream of one translation into the pieces that go to synthesis, by the shared clause rule.

use prxvc_media::{Lang, first_clause_end, trim_stray_start};

use crate::commit_policy::{CommitParams, TranslationProgress, TtsCommit, decide_tts_commit};

/// Collects translated text and says when a piece may be synthesised.
///
/// Without `split_target_clauses` the whole translation is one piece, handed
/// out when the stream ends. With it, the first clause goes as soon as
/// `decide_tts_commit` allows (the clause rule of `prxvc_media::clause_split`
/// plus the configured number of tokens after its mark), then the same
/// decision is taken for the rest.
pub(crate) struct PieceSplitter {
    params: CommitParams,
    lang: Lang,
    pending: String,
    tokens_after_mark: u32,
}

impl PieceSplitter {
    pub(crate) fn new(params: CommitParams, lang: Lang) -> Self {
        Self {
            params,
            lang,
            pending: String::new(),
            tokens_after_mark: 0,
        }
    }

    /// Add a token delta; returns the pieces that may go to synthesis now.
    pub(crate) fn push(&mut self, delta: &str) -> Vec<String> {
        if first_clause_end(self.lang, &self.pending).is_some() {
            self.tokens_after_mark += 1;
        }
        self.pending.push_str(delta);
        let mut pieces = Vec::new();
        while let TtsCommit::FirstSentence { end } =
            decide_tts_commit(&self.params, &self.progress(false))
        {
            let piece = self.pending[..end].trim().to_owned();
            self.pending = trim_stray_start(&self.pending[end..]).to_owned();
            self.tokens_after_mark = 0;
            if !piece.is_empty() {
                pieces.push(piece);
            }
        }
        pieces
    }

    /// The stream ended: the rest of the text is the last piece, if there is any.
    pub(crate) fn finish(&mut self) -> Option<String> {
        match decide_tts_commit(&self.params, &self.progress(true)) {
            TtsCommit::Whole => {
                let rest = std::mem::take(&mut self.pending);
                Some(rest.trim().to_owned())
            }
            TtsCommit::Wait | TtsCommit::FirstSentence { .. } => None,
        }
    }

    fn progress(&self, finished: bool) -> TranslationProgress<'_> {
        TranslationProgress {
            lang: self.lang,
            finished,
            text: &self.pending,
            tokens_after_first_sentence: self.tokens_after_mark,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split_params() -> CommitParams {
        CommitParams {
            split_target_clauses: true,
            split_min_tokens_after: 2,
            ..CommitParams::default()
        }
    }

    fn run(params: CommitParams, lang: Lang, tokens: &[&str]) -> Vec<String> {
        let mut s = PieceSplitter::new(params, lang);
        let mut pieces: Vec<String> = tokens.iter().flat_map(|t| s.push(t)).collect();
        pieces.extend(s.finish());
        pieces
    }

    #[test]
    fn without_splitting_the_whole_translation_is_one_piece() {
        let pieces = run(
            CommitParams::default(),
            Lang::En,
            &["Hello ", "there. ", "How ", "are ", "you?"],
        );
        assert_eq!(pieces, vec!["Hello there. How are you?"]);
    }

    #[test]
    fn splitting_cuts_the_first_clause_after_enough_tokens_and_sends_the_rest_at_the_end() {
        let pieces = run(
            split_params(),
            Lang::En,
            &["Hello ", "there", ". ", "How ", "are ", "you ", "today?"],
        );
        assert_eq!(pieces, vec!["Hello there.", "How are you today?"]);
    }

    #[test]
    fn a_decimal_point_and_an_abbreviation_never_split() {
        let pieces = run(
            split_params(),
            Lang::En,
            &[
                "The ", "price ", "is ", "3", ".", "5 ", "dollars ", "from ", "Dr", ". ", "Smith ",
                "today.",
            ],
        );
        assert_eq!(
            pieces,
            vec!["The price is 3.5 dollars from Dr. Smith today."],
            "no mark here ends a clause until the stream does"
        );
    }

    #[test]
    fn a_real_sentence_end_after_a_number_splits() {
        let pieces = run(
            split_params(),
            Lang::En,
            &[
                "It ", "costs ", "3.5 ", "dollars", ". ", "Pay ", "now ", "please ", "sir.",
            ],
        );
        assert_eq!(pieces, vec!["It costs 3.5 dollars.", "Pay now please sir."]);
    }

    #[test]
    fn chinese_text_splits_on_its_own_marks() {
        let pieces = run(
            split_params(),
            Lang::Zh,
            &[
                "今天", "天气", "很好", "。", "我们", "去", "公园", "吧", "。",
            ],
        );
        assert_eq!(pieces, vec!["今天天气很好。", "我们去公园吧。"]);
    }

    #[test]
    fn empty_or_mark_only_text_makes_no_piece() {
        assert!(run(CommitParams::default(), Lang::En, &["  "]).is_empty());
        assert!(run(split_params(), Lang::En, &[]).is_empty());
    }

    #[test]
    fn a_mark_that_needs_the_next_character_waits_for_it() {
        let mut s = PieceSplitter::new(split_params(), Lang::En);
        assert!(
            s.push("Pay now please sir.").is_empty(),
            "the dot may be a decimal point"
        );
        assert!(s.push(" ").is_empty());
        assert!(
            s.push("Then").is_empty(),
            "not enough tokens after the mark yet"
        );
        let rest = s.push(" go");
        assert_eq!(rest, vec!["Pay now please sir."]);
    }
}
