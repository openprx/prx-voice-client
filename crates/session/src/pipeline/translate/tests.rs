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
    let p = PipelineParams::new(
        Direction::Incoming,
        Lang::En,
        Lang::Zh,
        VoiceSelection::Preset { name: "p".into() },
    );
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
        for (i, text) in texts.iter().enumerate() {
            let job = ClauseJob {
                clause: clause(u16::try_from(i).expect("few clauses")),
                text: (*text).to_owned(),
                token: CancellationToken::new(),
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
