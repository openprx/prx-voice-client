//! The draft worker and its link, with the fake translator on a manual clock.

use std::thread;
use std::time::Duration;

use prxvc_media::{Direction, Lang, ManualClock};
use prxvc_ports::fakes::{FakeTraceSink, FakeTranslator, MtScript};
use prxvc_ports::{MtError, MtFlag, MtRequest, MtStream, ProviderDescriptor, VoiceSelection};

use super::*;

const A: &str = "we should meet";
const B: &str = "we should meet at the office";

/// A translator whose every call takes 80 ms of pipeline time.
struct Timed {
    inner: FakeTranslator,
    clock: ManualClock,
}

impl Translator for Timed {
    fn descriptor(&self) -> ProviderDescriptor {
        self.inner.descriptor()
    }

    fn translate(&self, req: MtRequest) -> Result<Box<dyn MtStream>, MtError> {
        self.clock.advance(Duration::from_millis(80));
        self.inner.translate(req)
    }
}

fn segment() -> SegmentId {
    SegmentId::first(Direction::Incoming)
}

fn key(revision: u32) -> DraftKey {
    DraftKey {
        segment: segment(),
        round: 0,
        committed_bytes: 0,
        revision,
    }
}

fn job(revision: u32, texts: &[(DraftRole, &str)], token: &CancellationToken) -> DraftJob {
    DraftJob {
        key: key(revision),
        clause: ClauseId::new(segment(), 0),
        texts: texts
            .iter()
            .map(|(role, text)| DraftText {
                role: *role,
                text: (*text).to_owned(),
            })
            .collect(),
        token: token.clone(),
    }
}

fn scripted() -> (FakeTranslator, ManualClock) {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script(A, MtScript::tokens(["Nos ", "vemos."]))
        .with_script(B, MtScript::tokens(["Nos vemos en la oficina."]));
    (mt, clock)
}

fn worker(mt: FakeTranslator, clock: &ManualClock) -> (DraftWorker, Arc<Stats>, Arc<DraftCache>) {
    let stats = Arc::new(Stats::default());
    let cache = Arc::new(DraftCache::default());
    let mut p = PipelineParams::new(
        Direction::Incoming,
        Lang::En,
        Lang::Zh,
        VoiceSelection::Preset { name: "p".into() },
    );
    p.glossary = vec![("office".to_owned(), "oficina".to_owned())];
    let w = DraftWorker {
        mt: Arc::new(Timed {
            inner: mt,
            clock: clock.clone(),
        }),
        p,
        cache: Arc::clone(&cache),
        tracer: Tracer::new(Arc::new(FakeTraceSink::new(false)), Arc::new(clock.clone())),
        stats: Arc::clone(&stats),
        warned: None,
    };
    (w, stats, cache)
}

fn texts_of(reply: &DraftReply) -> Vec<(DraftRole, String, Option<String>)> {
    reply
        .results
        .iter()
        .map(|(role, source, r)| {
            (
                *role,
                source.clone(),
                r.as_ref().ok().map(|c| c.text.clone()),
            )
        })
        .collect()
}

#[test]
fn a_job_is_translated_text_by_text_in_order_and_each_engine_call_is_timed() {
    let (mt, clock) = scripted();
    let (mut w, stats, cache) = worker(mt.clone(), &clock);
    let token = CancellationToken::new();
    let reply = w.process(&job(
        7,
        &[(DraftRole::Candidate, A), (DraftRole::Reference, B)],
        &token,
    ));
    assert_eq!(reply.key, key(7));
    assert_eq!(
        texts_of(&reply),
        [
            (
                DraftRole::Candidate,
                A.to_owned(),
                Some("Nos vemos.".to_owned())
            ),
            (
                DraftRole::Reference,
                B.to_owned(),
                Some("Nos vemos en la oficina.".to_owned())
            ),
        ]
    );
    assert_eq!(reply.engine_ms, [80, 80]);
    assert_eq!(reply.cache_hits, 0);
    let asked: Vec<String> = mt.requests().iter().map(|r| r.text.clone()).collect();
    assert_eq!(asked, [A, B], "in the order of the job");
    let first = &mt.requests()[0];
    assert!(first.context.is_empty(), "drafts carry no context");
    assert!(!first.strict_language);
    assert_eq!(
        first.glossary,
        [("office".to_owned(), "oficina".to_owned())]
    );
    assert_eq!(cache.len(), 2, "every success is cached");
    assert_eq!(stats.counters(0).drafts_engine_calls, 2);
}

#[test]
fn a_cached_text_is_served_without_an_engine_call() {
    let (mt, clock) = scripted();
    let (mut w, stats, cache) = worker(mt.clone(), &clock);
    cache.put(
        A,
        Cached {
            text: "Nos vemos.".to_owned(),
            flags: vec![MtFlag::TooLong],
        },
    );
    let token = CancellationToken::new();
    let reply = w.process(&job(
        1,
        &[(DraftRole::Sentence, A), (DraftRole::Reference, B)],
        &token,
    ));
    assert_eq!(reply.cache_hits, 1);
    assert_eq!(
        reply.engine_ms.len(),
        1,
        "one call, for the text that was not cached"
    );
    let hit = reply.results[0].2.as_ref().expect("a hit");
    assert_eq!(
        hit.flags,
        [MtFlag::TooLong],
        "the entry's flags come with it"
    );
    assert_eq!(mt.requests().len(), 1);
    let c = stats.counters(0);
    assert_eq!((c.drafts_cache_hits, c.drafts_engine_calls), (1, 1));
}

#[test]
fn a_cancelled_token_gives_a_cancellation_and_no_cache_entry() {
    let (mt, clock) = scripted();
    let (mut w, stats, cache) = worker(mt.clone(), &clock);
    let token = CancellationToken::new();
    token.cancel();
    let reply = w.process(&job(1, &[(DraftRole::Reference, B)], &token));
    assert!(matches!(reply.results[0].2, Err(DraftError::Cancelled)));
    assert!(
        mt.requests().is_empty(),
        "no call is made for a cancelled job"
    );
    assert_eq!(cache.len(), 0);
    assert_eq!(
        stats.counters(0).drafts_failed,
        0,
        "a cancellation is not a failure"
    );
}

#[test]
fn a_failed_or_flagged_draft_is_an_error_that_is_counted_and_never_cached() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone())
        .with_script(
            A,
            MtScript::tokens(["x"]).failing_open(MtError::Rejected { status: 400 }),
        )
        .with_script(
            B,
            MtScript::tokens(["Hello."]).flagged_suspect("not a translation"),
        );
    let (mut w, stats, cache) = worker(mt, &clock);
    let token = CancellationToken::new();
    let reply = w.process(&job(
        1,
        &[(DraftRole::Candidate, A), (DraftRole::Reference, B)],
        &token,
    ));
    assert!(matches!(
        reply.results[0].2,
        Err(DraftError::Failed(FaultKind::Rejected))
    ));
    assert!(matches!(reply.results[1].2, Err(DraftError::Suspect)));
    assert_eq!(cache.len(), 0);
    assert_eq!(stats.counters(0).drafts_failed, 2);
}

#[test]
fn a_newer_job_replaces_a_queued_one_and_the_replaced_job_is_not_waited_for() {
    let queues = DraftQueues::new(1, 4);
    let stats = Arc::new(Stats::default());
    let mut link = ThreadedLink::new(&queues, Arc::clone(&stats));
    let token = CancellationToken::new();
    assert!(!link.in_flight());
    link.submit(job(1, &[(DraftRole::Reference, A)], &token));
    assert!(link.in_flight());
    link.submit(job(2, &[(DraftRole::Reference, B)], &token));
    let s = queues.input.stats();
    assert_eq!(
        (s.capacity, s.policy, s.len, s.coalesced),
        (1, Policy::DropOldest, 1, 1)
    );
    let queued = queues.input.try_pop().expect("the newer job");
    assert_eq!(queued.key.revision, 2);
    assert!(link.in_flight(), "the newer job is still unanswered");
    assert!(link.poll().is_none());
    let c = stats.counters(0);
    assert_eq!((c.drafts_requested, c.drafts_replied), (2, 1));
    queues.output.push(
        DraftReply {
            key: queued.key,
            results: Vec::new(),
            engine_ms: Vec::new(),
            cache_hits: 0,
        },
        None,
    );
    assert_eq!(link.poll().map(|r| r.key.revision), Some(2));
    assert!(!link.in_flight());
    let c = stats.counters(0);
    assert_eq!((c.drafts_requested, c.drafts_replied), (2, 2));
}

#[test]
fn the_worker_thread_answers_through_the_queues_and_ends_when_the_input_closes() {
    let (mt, clock) = scripted();
    let (w, stats, _cache) = worker(mt, &clock);
    let queues = DraftQueues::new(1, 4);
    let mut link = ThreadedLink::new(&queues, Arc::clone(&stats));
    let (input, output) = (Arc::clone(&queues.input), Arc::clone(&queues.output));
    let handle = thread::spawn(move || w.run(&input, &output));
    let token = CancellationToken::new();
    link.submit(job(3, &[(DraftRole::Reference, B)], &token));
    let mut seen = 0;
    let Popped::Item(reply) = queues.output.pop(Wait::Forever, &mut seen) else {
        panic!("a reply");
    };
    assert_eq!(reply.key.revision, 3);
    assert_eq!(
        texts_of(&reply)[0].2.as_deref(),
        Some("Nos vemos en la oficina.")
    );
    queues.input.close();
    handle.join().expect("the draft thread ends");
}

#[test]
fn the_first_failure_of_a_segment_is_the_only_one_logged() {
    let clock = ManualClock::new();
    let mt = FakeTranslator::new(clock.clone());
    let (mut w, stats, _) = worker(mt, &clock);
    let token = CancellationToken::new();
    for r in 0..3 {
        w.process(&job(r, &[(DraftRole::Reference, A)], &token));
    }
    assert_eq!(w.warned, Some(segment()));
    assert_eq!(
        stats.counters(0).drafts_failed,
        3,
        "every failure is counted"
    );
}
