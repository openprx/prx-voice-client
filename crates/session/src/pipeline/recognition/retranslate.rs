//! Recognition stage, re-translation: submits the open hypothesis to the draft worker, polls its replies once per frame and commits a source prefix whose translation the book confirmed (incremental-retranslation 4.3 to 4.5).

use prxvc_media::script::is_written_in;
use prxvc_media::text_units::units;
use prxvc_ports::{MtFlag, TraceStage};

use super::super::draft::{DraftJob, DraftKey, DraftLink, DraftReply, DraftRole, DraftText};
use prxvc_media::Envelope;

use super::super::messages::{CommitReason, DraftTally, RecognitionEvent};
use super::{OpenSegment, Out, RecognitionStage};
use crate::commit_policy::{
    DraftReplyView, DraftTranslation, HypInput, MAX_SENTENCE_UNITS, join_carry, join_sentence,
};
use crate::pipeline::snapshot::Stats;

impl RecognitionStage {
    /// Connect the stage to the draft worker. Without a link no hypothesis is submitted and the
    /// re-translation rule never fires.
    pub(crate) fn with_drafts(mut self, link: Box<dyn DraftLink>) -> Self {
        self.drafts = Some(link);
        self
    }

    /// After a hypothesis: ask the draft worker about it unless one of the skip rules holds (no
    /// job in flight, no pause under way, the interval since the last job passed, and the
    /// uncommitted text is not already in the language the listener hears). A skipped hypothesis
    /// is not shown to the book at all, so that no candidate waits for a translation nobody asked for.
    pub(super) fn submit_drafts(&mut self) {
        let p = self.p.retranslate;
        if !p.enabled {
            return;
        }
        let (Some(link), Some(open)) = (self.drafts.as_mut(), self.open.as_mut()) else {
            return;
        };
        let Some(hyp) = open.hyp.as_ref() else {
            return;
        };
        if link.in_flight() || !self.segmenter.silence().is_zero() {
            return;
        }
        if open
            .last_draft
            .is_some_and(|last| self.now.saturating_since(last) < p.interval)
        {
            return;
        }
        let committed = open.committed_bytes.min(hyp.text.len());
        let region = hyp.text.get(committed..).unwrap_or("");
        if region.trim().is_empty() || is_written_in(region, self.p.target) {
            return;
        }
        let input = HypInput {
            source: self.p.source,
            round_text: &hyp.text,
            committed,
            carry: &open.carry,
            sentence: &open.sentence,
            revision: open.revision,
        };
        let Some(plan) = open.book.on_hypothesis(&p, &input) else {
            return;
        };
        let mut texts: Vec<DraftText> = Vec::with_capacity(5);
        let mut candidates = vec![
            (DraftRole::Sentence, plan.sentence),
            (DraftRole::Candidate, plan.candidate),
        ];
        candidates.extend(
            plan.shorter
                .into_iter()
                .map(|t| (DraftRole::Shorter, Some(t))),
        );
        candidates.push((DraftRole::Reference, Some(plan.reference)));
        for (role, text) in candidates {
            if let Some(text) = text
                && !texts.iter().any(|t| t.text == text)
            {
                texts.push(DraftText { role, text });
            }
        }
        link.submit(DraftJob {
            key: DraftKey {
                segment: open.id,
                round: open.round,
                committed_bytes: open.committed_bytes,
                revision: open.revision,
            },
            clause: open.clause(),
            texts,
            token: open.token.clone(),
        });
        open.last_draft = Some(self.now);
    }

    /// Once per frame: take the draft replies that are ready and act on them. A reply for a round
    /// that has moved on is discarded and counted.
    pub(super) fn poll_drafts(&mut self, out: &mut Out) {
        while let Some(reply) = self.drafts.as_mut().and_then(|link| link.poll()) {
            self.on_draft_reply(&reply, out);
        }
    }

    /// Report what the draft worker did for a segment that is ending (only with re-translation on
    /// and something to report).
    pub(super) fn push_draft_tally(&self, open: &mut OpenSegment, out: &mut Out) {
        let tally = std::mem::take(&mut open.tally);
        if self.p.retranslate.enabled && tally != DraftTally::default() {
            out.push(Envelope::new(
                open.id,
                open.revision,
                open.onset,
                RecognitionEvent::DraftTally(tally),
            ));
        }
    }

    fn on_draft_reply(&mut self, reply: &DraftReply, out: &mut Out) {
        let p = self.p.retranslate;
        let target = self.p.target;
        if let Some(open) = self.open.as_mut().filter(|o| o.id == reply.key.segment) {
            open.tally.drafts += u32::try_from(reply.engine_ms.len()).unwrap_or(u32::MAX);
            open.tally.hits += reply.cache_hits;
            open.tally.engine_ms.extend_from_slice(&reply.engine_ms);
        }
        let fresh = self.open.as_ref().is_some_and(|open| {
            reply.key.segment == open.id
                && reply.key.round == open.round
                && reply.key.committed_bytes == open.committed_bytes
        });
        if !fresh {
            Stats::add(&self.stats.drafts_stale, 1);
            if let Some(open) = self.open.as_mut().filter(|o| o.id == reply.key.segment) {
                open.tally.stale += 1;
            }
            return;
        }
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let results: Vec<(&str, Option<DraftTranslation<'_>>)> = reply
            .results
            .iter()
            .map(|(_, source, result)| {
                let translation = result.as_ref().ok().map(|c| DraftTranslation {
                    text: &c.text,
                    wrong_language: c.flags.contains(&MtFlag::WrongLanguage),
                });
                (source.as_str(), translation)
            })
            .collect();
        let view = DraftReplyView {
            revision: reply.key.revision,
            results: &results,
        };
        let cut = open.book.on_reply(&p, target, &view);
        let rejected = open.book.rejected_spoken;
        Stats::add(
            &self.stats.drafts_rejected_spoken,
            rejected - open.rejected_counted,
        );
        open.rejected_counted = rejected;
        let Some(cut) = cut else {
            return;
        };
        // The hypothesis may have moved on while the job was out (no job is submitted while one is
        // in flight, so the book saw none of it): the unit is committed only when it is, byte for
        // byte, what the draft was made from.
        let still_valid = open.hyp.as_ref().is_some_and(|hyp| {
            hyp.text
                .get(open.committed_bytes..cut.cut)
                .is_some_and(|piece| {
                    join_sentence(&open.sentence, &join_carry(&open.carry, piece.trim()))
                        == cut.source_full
                })
        });
        let clause = open.clause();
        if still_valid && self.commit_prefix(cut.cut, CommitReason::Retranslation, out) {
            if let Some(open) = self.open.as_mut() {
                open.retranslated_round = true;
            }
            Stats::add(&self.stats.retranslated_units, 1);
            self.tracer.mark(clause, TraceStage::RetranslationCut);
        } else if let Some(open) = self.open.as_mut() {
            // Nothing was committed: what the book now believes was spoken is not known.
            open.book.set_spoken(None);
        }
    }
}

impl OpenSegment {
    /// After a unit was emitted: the source sentence goes on with it, or starts over when the unit
    /// ends the sentence. A sentence that has grown past the limit starts over with this unit, as
    /// the translation stage's memory does with the same text.
    pub(super) fn note_commit(&mut self, continues: bool, text: &str) {
        if continues {
            let base = if units(&self.sentence).len() > MAX_SENTENCE_UNITS {
                ""
            } else {
                self.sentence.as_str()
            };
            self.sentence = join_sentence(base, text);
        } else {
            self.sentence.clear();
        }
        self.book.reset();
    }
}
