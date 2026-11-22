//! The translation stage on its own: queues in, queues out, with the fake translator.

use prxvc_media::{ClauseId, Direction, Envelope, Lang, ManualClock, MonoTime, SegmentId};
use prxvc_ports::fakes::{FakeTraceSink, FakeTranslator, MtScript};
use prxvc_ports::{MtFlag, VoiceSelection};

use super::*;
use crate::pipeline::queue::Policy;

const TEXT: &str = "we should meet at the office";

struct Rig {
    stage: MtStage,
    mt: FakeTranslator,
    input: Arc<BoundedQueue<MtItem>>,
    tts_in: Arc<BoundedQueue<TtsItem>>,
    inbox: Arc<BoundedQueue<super::super::messages::Inbound>>,
}

fn clause(index: u16) -> ClauseId {
    ClauseId::new(
        SegmentId::new(Direction::Incoming, 1).expect("seq >= 1"),
        index,
    )
}

fn rig(mt: FakeTranslator, cache: Option<Arc<DraftCache>>) -> Rig {
    rig_with(mt, cache, |_| {})
}

fn rig_with(
    mt: FakeTranslator,
    cache: Option<Arc<DraftCache>>,
    customize: impl FnOnce(&mut PipelineParams),
) -> Rig {
    let mut p = PipelineParams::new(
        Direction::Incoming,
        Lang::En,
        Lang::Zh,
        VoiceSelection::Preset { name: "p".into() },
    );
    customize(&mut p);
    let input = Arc::new(BoundedQueue::new("mt_in", 8, Policy::DropOldest));
    let tts_in = Arc::new(BoundedQueue::new("tts_in", 64, Policy::Block));
    let inbox = Arc::new(BoundedQueue::new("inbox", 256, Policy::Block));
    let stage = MtStage {
        mt: Arc::new(mt.clone()),
        p,
        outbox: Outbox::new(Arc::clone(&inbox)),
        input: Arc::clone(&input),
        tts_in: Arc::clone(&tts_in),
        call: CancellationToken::new(),
        tracer: Tracer::new(
            Arc::new(FakeTraceSink::new(false)),
            Arc::new(ManualClock::new()),
        ),
        stats: Arc::new(Stats::default()),
        cache,
    };
    Rig {
        stage,
        mt,
        input,
        tts_in,
        inbox,
    }
}

impl Rig {
    /// Translate the given clauses in order and return what the stage reported.
    fn run(self, texts: &[&str]) -> Done {
        let jobs: Vec<(u16, &str, bool)> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| (u16::try_from(i).expect("few clauses"), *t, false))
            .collect();
        self.run_jobs(&jobs)
    }

    /// Translate the given units, each with its clause index and whether it continues its sentence.
    fn run_jobs(self, jobs: &[(u16, &str, bool)]) -> Done {
        for (index, text, continues) in jobs {
            let job = ClauseJob {
                clause: clause(*index),
                text: (*text).to_owned(),
                token: CancellationToken::new(),
                continues: *continues,
            };
            let envelope = Envelope::new(job.clause.segment(), 1, MonoTime::ZERO, job);
            self.input.push(MtItem::Clause(envelope), None);
        }
        self.input.close();
        self.stage.run();
        Done {
            mt: self.mt,
            events: std::iter::from_fn(|| self.inbox.try_pop()).collect(),
            pieces: std::iter::from_fn(|| self.tts_in.try_pop()).collect(),
        }
    }
}

struct Done {
    mt: FakeTranslator,
    events: Vec<super::super::messages::Inbound>,
    pieces: Vec<TtsItem>,
}

impl Done {
    fn stage_events(&self) -> Vec<&StageEvent> {
        self.events
            .iter()
            .filter_map(|e| match e {
                super::super::messages::Inbound::Stage(e) => Some(&e.payload),
                _ => None,
            })
            .collect()
    }

    fn text_of(&self, c: ClauseId) -> String {
        self.stage_events()
            .iter()
            .filter_map(|e| match e {
                StageEvent::MtText { clause, delta } if *clause == c => Some(delta.as_str()),
                _ => None,
            })
            .collect()
    }

    fn outcomes(&self) -> Vec<(u16, ContinuationOutcome)> {
        self.stage_events()
            .iter()
            .filter_map(|e| match e {
                StageEvent::MtContinuation { clause, outcome } => Some((clause.index(), *outcome)),
                _ => None,
            })
            .collect()
    }

    fn failed(&self) -> bool {
        self.stage_events()
            .iter()
            .any(|e| matches!(e, StageEvent::MtFailed { .. }))
    }

    /// The text of the clause's pieces and whether its end was sent, in order.
    fn spoken_of(&self, c: ClauseId) -> Vec<String> {
        self.pieces
            .iter()
            .filter_map(|p| match p {
                TtsItem::Piece(e) if e.payload.clause == c => Some(e.payload.text.clone()),
                _ => None,
            })
            .collect()
    }

    fn ends_of(&self, c: ClauseId) -> usize {
        self.pieces
            .iter()
            .filter(|p| matches!(p, TtsItem::End(e) if e.payload.clause == c))
            .count()
    }

    fn spoken(&self) -> Vec<String> {
        self.pieces
            .iter()
            .filter_map(|p| match p {
                TtsItem::Piece(e) => Some(e.payload.text.clone()),
                _ => None,
            })
            .filter(|t| !t.is_empty())
            .collect()
    }
}

fn mt() -> FakeTranslator {
    FakeTranslator::new(ManualClock::new())
        .with_script(TEXT, MtScript::tokens(["我们应该在办公室见面。"]))
}

#[test]
fn a_cache_hit_gives_one_text_and_done_with_the_flags_of_the_entry_and_no_engine_call() {
    let cache = Arc::new(DraftCache::default());
    cache.put(
        TEXT,
        Cached {
            text: "我们应该在办公室碰头。".to_owned(),
            flags: vec![MtFlag::TooLong],
        },
    );
    let done = rig(mt(), Some(cache)).run(&[TEXT]);
    assert!(done.mt.requests().is_empty(), "the engine was not asked");
    assert_eq!(done.text_of(clause(0)), "我们应该在办公室碰头。");
    let events = done.stage_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, StageEvent::MtDone { .. })),
        "{events:?}"
    );
    assert!(
        events.iter().any(
            |e| matches!(e, StageEvent::MtFlagged { flags, .. } if flags == &[MtFlag::TooLong])
        ),
        "{events:?}"
    );
    assert_eq!(done.spoken(), ["我们应该在办公室碰头。"]);
}

#[test]
fn a_miss_asks_the_engine_and_the_result_is_held_for_the_next_request() {
    let cache = Arc::new(DraftCache::default());
    let done = rig(mt(), Some(Arc::clone(&cache))).run(&[TEXT, TEXT]);
    assert_eq!(done.mt.requests().len(), 1, "the second clause is a hit");
    assert_eq!(done.text_of(clause(0)), "我们应该在办公室见面。");
    assert_eq!(done.text_of(clause(1)), "我们应该在办公室见面。");
    assert_eq!(
        cache.get(TEXT).map(|c| c.text),
        Some("我们应该在办公室见面。".to_owned())
    );
}

#[test]
fn without_a_cache_every_clause_asks_the_engine() {
    let done = rig(mt(), None).run(&[TEXT, TEXT]);
    assert_eq!(done.mt.requests().len(), 2);
}

#[test]
fn the_retry_with_the_strict_instruction_bypasses_the_cache() {
    // The held translation is in the wrong language: it is caught before anything is spoken and
    // the engine is asked again, with the strict instruction, whatever the cache holds.
    let cache = Arc::new(DraftCache::default());
    cache.put(
        TEXT,
        Cached {
            text: "We should meet at the office tomorrow.".to_owned(),
            flags: Vec::new(),
        },
    );
    let done = rig(mt(), Some(Arc::clone(&cache))).run(&[TEXT]);
    let requests = done.mt.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].strict_language);
    assert_eq!(done.spoken(), ["我们应该在办公室见面。"]);
    assert_eq!(
        cache.get(TEXT).map(|c| c.text),
        Some("我们应该在办公室见面。".to_owned()),
        "the good answer replaces the bad one"
    );
}

#[test]
fn a_translation_that_failed_is_not_held() {
    let cache = Arc::new(DraftCache::default());
    let failing = FakeTranslator::new(ManualClock::new()).with_script(
        TEXT,
        MtScript {
            suspect: Some("empty".to_owned()),
            ..MtScript::tokens(["我们应该在办公室见面。"])
        },
    );
    rig(failing, Some(Arc::clone(&cache))).run(&[TEXT]);
    assert_eq!(cache.len(), 0);
}

/// Re-translation with continuation on: units translated from the start of their sentence.
fn continuing(mt: FakeTranslator) -> Rig {
    rig_with(mt, Some(Arc::new(DraftCache::default())), |p| {
        p.retranslate.enabled = true;
        p.retranslate.continuation = true;
    })
}

fn sentence_mt() -> FakeTranslator {
    FakeTranslator::new(ManualClock::new())
        .with_script("we should meet", MtScript::tokens(["我们应该见面。"]))
        .with_script(
            "we should meet at the office",
            MtScript::tokens(["我们应该见面，在办公室。"]),
        )
        .with_script(
            "we should meet at the office tomorrow",
            MtScript::tokens(["我们应该见面，在办公室，明天。"]),
        )
}

#[test]
fn the_next_unit_is_translated_from_the_start_of_the_sentence_and_speaks_only_the_suffix() {
    let done = continuing(sentence_mt())
        .run_jobs(&[(0, "we should meet", true), (1, "at the office", false)]);
    let requests = done.mt.requests();
    let texts: Vec<&str> = requests.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, ["we should meet", "we should meet at the office"]);
    // The first unit continues its sentence: its closing mark is spoken as a comma.
    assert_eq!(done.spoken_of(clause(0)), ["我们应该见面，"]);
    // The second speaks what follows the first, with its own closing mark.
    assert_eq!(done.spoken_of(clause(1)), ["在办公室。"]);
    // Captions and the clause's text hold the suffix only.
    assert_eq!(done.text_of(clause(1)), "在办公室。");
    assert_eq!(
        done.outcomes(),
        [
            (0, ContinuationOutcome::Standalone),
            (1, ContinuationOutcome::Continued)
        ]
    );
    assert!(!done.failed());
    assert_eq!(done.ends_of(clause(1)), 1);
}

#[test]
fn a_chain_of_units_speaks_the_whole_translation_once() {
    let done = continuing(sentence_mt()).run_jobs(&[
        (0, "we should meet", true),
        (1, "at the office", true),
        (2, "tomorrow", false),
    ]);
    let spoken: String = (0..3).flat_map(|i| done.spoken_of(clause(i))).collect();
    assert_eq!(spoken, "我们应该见面，在办公室，明天。");
    let requests = done.mt.requests();
    assert_eq!(
        requests.last().map(|r| r.text.as_str()),
        Some("we should meet at the office tomorrow")
    );
}

#[test]
fn a_translation_that_no_longer_starts_with_the_spoken_part_is_translated_on_its_own() {
    let mt = FakeTranslator::new(ManualClock::new())
        .with_script("we should meet", MtScript::tokens(["我们应该见面。"]))
        .with_script(
            "we should meet at the office",
            MtScript::tokens(["他们必须在办公室碰头。"]),
        )
        .with_script("at the office", MtScript::tokens(["在办公室。"]))
        .with_script(
            "at the office tomorrow",
            MtScript::tokens(["在办公室，明天。"]),
        );
    let done = continuing(mt).run_jobs(&[
        (0, "we should meet", true),
        (1, "at the office", true),
        (2, "tomorrow", false),
    ]);
    let requests = done.mt.requests();
    let texts: Vec<&str> = requests.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "we should meet",
            "we should meet at the office",
            "at the office",
            "at the office tomorrow",
        ],
        "the divergent unit is asked for again alone, and the sentence restarts from it"
    );
    // Nothing spoken is repeated or taken back; the new unit speaks its own translation.
    assert_eq!(done.spoken_of(clause(0)), ["我们应该见面，"]);
    assert_eq!(done.spoken_of(clause(1)), ["在办公室，"]);
    assert_eq!(done.spoken_of(clause(2)), ["明天。"]);
    assert_eq!(
        done.outcomes(),
        [
            (0, ContinuationOutcome::Standalone),
            (1, ContinuationOutcome::Diverged),
            (2, ContinuationOutcome::Continued),
        ]
    );
    assert!(!done.failed());
}

#[test]
fn a_gap_in_the_chain_drops_the_sentence_memory() {
    let mt = sentence_mt().with_script("at the office", MtScript::tokens(["在办公室。"]));
    let done = continuing(mt).run_jobs(&[(0, "we should meet", true), (2, "at the office", false)]);
    let requests = done.mt.requests();
    let texts: Vec<&str> = requests.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, ["we should meet", "at the office"]);
    assert_eq!(done.spoken_of(clause(2)), ["在办公室。"]);
    assert_eq!(
        done.outcomes(),
        [
            (0, ContinuationOutcome::Standalone),
            (2, ContinuationOutcome::Standalone)
        ]
    );
}

#[test]
fn an_empty_suffix_ends_the_unit_with_the_end_alone_and_is_not_a_failure() {
    let mt = sentence_mt().with_script(
        "we should meet at the office",
        MtScript::tokens(["我们应该见面。"]),
    );
    let done = continuing(mt).run_jobs(&[(0, "we should meet", true), (1, "at the office", false)]);
    assert!(!done.failed());
    assert_eq!(done.text_of(clause(1)), "");
    assert!(done.spoken_of(clause(1)).iter().all(String::is_empty));
    assert_eq!(done.ends_of(clause(1)), 1);
    assert!(
        done.stage_events()
            .iter()
            .any(|e| matches!(e, StageEvent::MtDone { clause: c } if *c == clause(1)))
    );
    assert_eq!(
        done.outcomes(),
        [
            (0, ContinuationOutcome::Standalone),
            (1, ContinuationOutcome::Empty)
        ]
    );
}

#[test]
fn a_closing_mark_becomes_a_comma_only_when_the_unit_continues_its_sentence() {
    let mt = || {
        FakeTranslator::new(ManualClock::new())
            .with_script("we should meet", MtScript::tokens(["我们应该见面。"]))
    };
    let continuing_unit = continuing(mt()).run_jobs(&[(0, "we should meet", true)]);
    assert_eq!(continuing_unit.spoken_of(clause(0)), ["我们应该见面，"]);
    let last_unit = continuing(mt()).run_jobs(&[(0, "we should meet", false)]);
    assert_eq!(last_unit.spoken_of(clause(0)), ["我们应该见面。"]);
}

#[test]
fn without_continuation_units_are_translated_alone_and_keep_their_marks() {
    let mt = sentence_mt().with_script("at the office", MtScript::tokens(["在办公室。"]));
    let done = rig_with(mt, None, |_| {})
        .run_jobs(&[(0, "we should meet", true), (1, "at the office", true)]);
    let requests = done.mt.requests();
    let texts: Vec<&str> = requests.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, ["we should meet", "at the office"]);
    assert!(done.outcomes().is_empty(), "no continuation event");
    assert_eq!(done.text_of(clause(0)), "我们应该见面。");
}

/// A translator that words the start of the whole sentence differently the second time it is asked.
fn rewording_mt() -> FakeTranslator {
    FakeTranslator::new(ManualClock::new())
        .with_script(
            "这个项目的预算",
            MtScript::tokens(["The budget of this project"]),
        )
        .with_script(
            "这个项目的预算是两百五十万美元",
            MtScript::tokens(["The budget for this project is $2.5 million."]),
        )
}

/// A Chinese to English rig with the given alignment ratio.
fn continuing_with_ratio(mt: FakeTranslator, ratio: f32) -> Rig {
    rig_with(mt, Some(Arc::new(DraftCache::default())), |p| {
        p.source = Lang::Zh;
        p.target = Lang::En;
        p.retranslate.enabled = true;
        p.retranslate.continuation = true;
        p.retranslate.align_max_ratio = ratio;
    })
}

const REWORDED_JOBS: [(u16, &str, bool); 2] =
    [(0, "这个项目的预算", true), (1, "是两百五十万美元", false)];

#[test]
fn a_near_prefix_continues_the_sentence_and_is_counted_apart() {
    let done = continuing_with_ratio(rewording_mt(), 0.5).run_jobs(&REWORDED_JOBS);
    assert!(!done.failed());
    assert_eq!(done.text_of(clause(1)), "is $2.5 million.");
    assert_eq!(
        done.outcomes(),
        [
            (0, ContinuationOutcome::Standalone),
            (1, ContinuationOutcome::ContinuedApprox)
        ]
    );
}

#[test]
fn an_exact_prefix_still_counts_as_continued_whatever_the_ratio() {
    let mt = FakeTranslator::new(ManualClock::new())
        .with_script("我们见面", MtScript::tokens(["We meet"]))
        .with_script("我们见面在九点", MtScript::tokens(["We meet at nine."]));
    let done =
        continuing_with_ratio(mt, 0.5).run_jobs(&[(0, "我们见面", true), (1, "在九点", false)]);
    assert_eq!(done.text_of(clause(1)), "at nine.");
    assert_eq!(done.outcomes()[1], (1, ContinuationOutcome::Continued));
}

#[test]
fn a_zero_ratio_restores_the_exact_rule() {
    let mt =
        rewording_mt().with_script("是两百五十万美元", MtScript::tokens(["It is 2.5 million."]));
    let done = continuing_with_ratio(mt, 0.0).run_jobs(&REWORDED_JOBS);
    assert_eq!(done.outcomes()[1], (1, ContinuationOutcome::Diverged));
    assert_eq!(done.text_of(clause(1)), "It is 2.5 million.");
}

#[test]
fn a_really_different_translation_still_diverges() {
    let mt = FakeTranslator::new(ManualClock::new())
        .with_script("我们见面", MtScript::tokens(["We meet"]))
        .with_script(
            "我们见面在九点",
            MtScript::tokens(["Nothing alike was said then."]),
        )
        .with_script("在九点", MtScript::tokens(["at nine."]));
    let done =
        continuing_with_ratio(mt, 0.5).run_jobs(&[(0, "我们见面", true), (1, "在九点", false)]);
    assert_eq!(done.outcomes()[1], (1, ContinuationOutcome::Diverged));
}

#[test]
fn an_approximate_start_with_nothing_after_it_is_an_empty_remainder() {
    let mt = FakeTranslator::new(ManualClock::new())
        .with_script(
            "这个项目的预算",
            MtScript::tokens(["The budget of this project"]),
        )
        .with_script(
            "这个项目的预算是",
            MtScript::tokens(["The budget for this project."]),
        );
    let done =
        continuing_with_ratio(mt, 0.5).run_jobs(&[(0, "这个项目的预算", true), (1, "是", false)]);
    assert!(!done.failed());
    assert_eq!(done.text_of(clause(1)), "");
    assert_eq!(done.outcomes()[1], (1, ContinuationOutcome::Empty));
}
