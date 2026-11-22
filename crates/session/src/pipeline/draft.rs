//! Draft worker: translates the hypotheses the recognition stage asks about, off the recognition thread, and the link the recognition stage uses to submit jobs and poll replies (incremental-retranslation 4.4, 5.2).

use std::sync::Arc;

use prxvc_media::{ClauseId, SegmentId};
use prxvc_ports::{CancellationToken, MtDelta, MtError, MtRequest, Translator};

use super::draft_cache::{Cached, DraftCache};
use super::faults::mt_failure;
use super::params::PipelineParams;
use super::queue::{BoundedQueue, Policy, Popped, Pushed, Wait};
use super::snapshot::Stats;
use super::trace::Tracer;
use crate::fault::FaultKind;

/// Which round of which hypothesis a job was made for: a reply is used only while its key still
/// describes the open round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DraftKey {
    pub(crate) segment: SegmentId,
    /// Counts the segment's rounds (a round ends at every finalisation).
    pub(crate) round: u32,
    /// Bytes of the round's text that were committed when the job was made.
    pub(crate) committed_bytes: usize,
    /// The hypothesis revision.
    pub(crate) revision: u32,
}

/// What a text of a job stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DraftRole {
    /// The sentence already spoken.
    Sentence,
    /// A candidate unit, from the start of the sentence.
    Candidate,
    /// A candidate with its last units left off, from the start of the sentence.
    Shorter,
    /// The whole uncommitted hypothesis, from the start of the sentence.
    Reference,
}

/// One text to translate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DraftText {
    pub(crate) role: DraftRole,
    pub(crate) text: String,
}

/// Texts of one hypothesis to translate (at most five, distinct).
#[derive(Debug)]
pub(crate) struct DraftJob {
    pub(crate) key: DraftKey,
    /// The open clause, informational for the provider.
    pub(crate) clause: ClauseId,
    pub(crate) texts: Vec<DraftText>,
    /// The segment's token: cancelling the segment abandons the call in flight.
    pub(crate) token: CancellationToken,
}

/// Why a draft has no translation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DraftError {
    /// The job's token was cancelled before or during the call. Nothing was cached.
    Cancelled,
    /// The engine failed.
    Failed(FaultKind),
    /// The engine's output was flagged as not being a translation.
    Suspect,
}

/// The answer to one job: a result per text, in the order of the job.
#[derive(Debug)]
pub(crate) struct DraftReply {
    pub(crate) key: DraftKey,
    /// Role, source text and result.
    pub(crate) results: Vec<(DraftRole, String, Result<Cached, DraftError>)>,
    /// Elapsed time of each engine call on the pipeline clock, in milliseconds.
    pub(crate) engine_ms: Vec<u64>,
    /// Texts served from the cache.
    pub(crate) cache_hits: u32,
}

/// The recognition stage's view of the draft worker: threaded in production, inline in unit tests.
pub(crate) trait DraftLink: Send {
    /// Hand a job over. It never blocks; a job still queued is replaced by this one.
    fn submit(&mut self, job: DraftJob);
    /// A reply that is ready, if any. It never blocks.
    fn poll(&mut self) -> Option<DraftReply>;
    /// A job was submitted and its reply has not been polled yet.
    fn in_flight(&self) -> bool;
}

/// The queues between the recognition thread and the draft thread, declared where they are made.
#[derive(Clone)]
pub(crate) struct DraftQueues {
    /// Jobs: capacity 1, a newer job replaces a queued one (counted as coalesced).
    pub(crate) input: Arc<BoundedQueue<DraftJob>>,
    /// Replies: capacity 4, the oldest is dropped when full; the recognition thread drains it
    /// once per frame and never waits.
    pub(crate) output: Arc<BoundedQueue<DraftReply>>,
}

impl DraftQueues {
    pub(crate) fn new(input_cap: usize, output_cap: usize) -> Self {
        Self {
            input: Arc::new(BoundedQueue::new("draft_in", input_cap, Policy::DropOldest)),
            output: Arc::new(BoundedQueue::new(
                "draft_out",
                output_cap,
                Policy::DropOldest,
            )),
        }
    }
}

/// The link over the worker's queues.
pub(crate) struct ThreadedLink {
    queues: DraftQueues,
    stats: Arc<Stats>,
    /// Jobs submitted whose reply was not polled yet.
    outstanding: u32,
}

impl ThreadedLink {
    pub(crate) fn new(queues: &DraftQueues, stats: Arc<Stats>) -> Self {
        Self {
            queues: queues.clone(),
            stats,
            outstanding: 0,
        }
    }
}

impl DraftLink for ThreadedLink {
    fn submit(&mut self, job: DraftJob) {
        let coalesced_before = self.queues.input.stats().coalesced;
        if !matches!(
            self.queues.input.push_replacing(job, None, |_| true),
            Pushed::Accepted | Pushed::Evicted(_)
        ) {
            // Closed: the pipeline is stopping.
            return;
        }
        Stats::add(&self.stats.drafts_requested, 1);
        self.outstanding += 1;
        if self.queues.input.stats().coalesced > coalesced_before {
            // The queued job it replaced will never be answered.
            self.outstanding = self.outstanding.saturating_sub(1);
            Stats::add(&self.stats.drafts_replied, 1);
        }
    }

    fn poll(&mut self) -> Option<DraftReply> {
        let reply = self.queues.output.try_pop()?;
        self.outstanding = self.outstanding.saturating_sub(1);
        Stats::add(&self.stats.drafts_replied, 1);
        Some(reply)
    }

    fn in_flight(&self) -> bool {
        self.outstanding > 0
    }
}

/// Translates the texts of draft jobs through the same translator the translation stage uses.
pub(crate) struct DraftWorker {
    pub(crate) mt: Arc<dyn Translator>,
    pub(crate) p: PipelineParams,
    /// Translations by exact source text; shared with the translation stage when it continues
    /// sentences, private to the worker otherwise.
    pub(crate) cache: Arc<DraftCache>,
    pub(crate) tracer: Tracer,
    pub(crate) stats: Arc<Stats>,
    /// The segment whose first failure was logged already.
    pub(crate) warned: Option<SegmentId>,
}

impl DraftWorker {
    /// The thread body: answer jobs until the input queue closes.
    pub(crate) fn run(mut self, input: &BoundedQueue<DraftJob>, output: &BoundedQueue<DraftReply>) {
        let mut seen = 0;
        loop {
            match input.pop(Wait::Forever, &mut seen) {
                Popped::Item(job) => {
                    let reply = self.process(&job);
                    // `DropOldest` with at most one job in flight: a reply is never lost.
                    let _ = output.push(reply, None);
                }
                Popped::Woken | Popped::Timeout => {}
                Popped::Closed => break,
            }
        }
    }

    /// Translate every text of `job`, in order, cache first.
    pub(crate) fn process(&mut self, job: &DraftJob) -> DraftReply {
        let mut reply = DraftReply {
            key: job.key,
            results: Vec::with_capacity(job.texts.len()),
            engine_ms: Vec::new(),
            cache_hits: 0,
        };
        for t in &job.texts {
            let result = self.one(job, &t.text, &mut reply);
            if let Err(DraftError::Failed(_) | DraftError::Suspect) = result {
                Stats::add(&self.stats.drafts_failed, 1);
                self.warn_once(job.key.segment, &result);
            }
            reply.results.push((t.role, t.text.clone(), result));
        }
        reply
    }

    fn one(
        &self,
        job: &DraftJob,
        text: &str,
        reply: &mut DraftReply,
    ) -> Result<Cached, DraftError> {
        if job.token.is_cancelled() {
            return Err(DraftError::Cancelled);
        }
        if let Some(hit) = self.cache.get(text) {
            Stats::add(&self.stats.drafts_cache_hits, 1);
            reply.cache_hits += 1;
            return Ok(hit);
        }
        let started = self.tracer.now();
        let outcome = self.engine(job, text);
        let elapsed = self.tracer.now().saturating_since(started);
        Stats::add(&self.stats.drafts_engine_calls, 1);
        reply
            .engine_ms
            .push(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX));
        let cached = outcome?;
        self.cache.put(text, cached.clone());
        Ok(cached)
    }

    /// One engine call, read to `Done`.
    fn engine(&self, job: &DraftJob, text: &str) -> Result<Cached, DraftError> {
        let request = MtRequest {
            clause: job.clause,
            source: self.p.source,
            target: self.p.target,
            text: text.to_owned(),
            context: Vec::new(),
            glossary: self.p.glossary.clone(),
            strict_language: false,
        };
        let mut stream = self.mt.translate(request).map_err(|e| failed(&e))?;
        let mut out = String::new();
        loop {
            match stream.next(&job.token) {
                Some(Ok(MtDelta::Text(t))) => out.push_str(&t),
                Some(Ok(MtDelta::Done { .. })) => break,
                Some(Err(e)) => return Err(failed(&e)),
                None => return Err(DraftError::Failed(FaultKind::Crashed)),
            }
        }
        if stream.suspect().is_some() {
            return Err(DraftError::Suspect);
        }
        Ok(Cached {
            text: out,
            flags: stream.flags(),
        })
    }

    /// A draft failure changes latency only: the first one of each segment is logged, without text.
    fn warn_once(&mut self, segment: SegmentId, result: &Result<Cached, DraftError>) {
        if self.warned == Some(segment) {
            return;
        }
        self.warned = Some(segment);
        let kind = match result {
            Err(DraftError::Failed(fault)) => format!("{fault:?}"),
            _ => "suspect".to_owned(),
        };
        tracing::warn!(
            stage = "draft",
            direction = ?self.p.direction,
            error = %kind,
            "draft translation failed; only latency is affected"
        );
    }
}

fn failed(e: &MtError) -> DraftError {
    if e.is_cancelled() {
        DraftError::Cancelled
    } else {
        DraftError::Failed(mt_failure(e, true).fault)
    }
}

/// A link that runs the worker on the caller's thread, for unit tests of the recognition stage.
#[cfg(test)]
pub(crate) struct InlineLink {
    worker: DraftWorker,
    replies: std::collections::VecDeque<DraftReply>,
    held: bool,
}

#[cfg(test)]
impl InlineLink {
    pub(crate) fn new(worker: DraftWorker) -> Self {
        Self {
            worker,
            replies: std::collections::VecDeque::new(),
            held: false,
        }
    }
}

#[cfg(test)]
impl DraftLink for InlineLink {
    fn submit(&mut self, job: DraftJob) {
        Stats::add(&self.worker.stats.drafts_requested, 1);
        let reply = self.worker.process(&job);
        self.replies.push_back(reply);
        self.held = true;
    }

    fn poll(&mut self) -> Option<DraftReply> {
        let reply = self.replies.pop_front()?;
        if self.replies.is_empty() {
            self.held = false;
        }
        Stats::add(&self.worker.stats.drafts_replied, 1);
        Some(reply)
    }

    fn in_flight(&self) -> bool {
        self.held
    }
}

#[cfg(test)]
mod tests;
