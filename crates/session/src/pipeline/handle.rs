//! The public handle of a direction pipeline: start it, feed it audio, command it, read its events and snapshots, stop it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use prxvc_media::{AudioFrame, ClauseId, Clock, MonoTime, SegmentId};
use prxvc_ports::{
    AudioSource, CancellationToken, RefineAsr, SpeechSynth, StreamingAsr, TraceSink, Translator,
    VadProvider,
};

use super::core::{Core, Gate, Wiring};
use super::draft::{DraftQueues, DraftWorker};
use super::draft_cache::DraftCache;
use super::events::PipelineEvent;
use super::input::AudioInput;
use super::messages::{Command, Inbound, MtItem, TtsItem};
use super::outbox::Outbox;
use super::params::PipelineParams;
use super::queue::{BoundedQueue, Policy, Popped, Pushed, QueueStats, Wait};
use super::refine::RefineStage;
use super::snapshot::{SnapshotSubscriber, Stats, snapshot_channel};
use super::synth::TtsStage;
use super::trace::Tracer;
use super::translate::MtStage;
use super::tree::TokenTree;
use super::workers::{RecognizerThread, Threads, run_capture, run_control};

/// The providers one direction uses. Both directions may hold clones of the
/// same synthesiser and translator: requests are independent and cancelled
/// one by one.
#[derive(Clone)]
pub struct PipelineProviders {
    /// Opens the direction's own VAD engine.
    pub vad: Arc<dyn VadProvider>,
    /// Opens the direction's own streaming recognition stream.
    pub asr: Arc<dyn StreamingAsr>,
    /// The refine pass; required by `RecognitionMode::TwoPass`.
    pub refine: Option<Arc<dyn RefineAsr>>,
    /// Translation.
    pub mt: Arc<dyn Translator>,
    /// Synthesis.
    pub tts: Arc<dyn SpeechSynth>,
}

/// A pipeline could not be started.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    /// Two-pass recognition was configured without a refine provider.
    #[error("two-pass recognition needs a refine provider")]
    MissingRefine,
    /// A thread could not be created.
    #[error("cannot start the {stage} thread: {reason}")]
    Spawn {
        /// Which thread.
        stage: &'static str,
        /// The operating system's answer.
        reason: String,
    },
}

/// What [`DirectionPipeline::stop`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopReport {
    /// Every stage thread ended within the time allowed.
    pub clean: bool,
    /// Threads that were still running when the wait ended.
    pub leaked: usize,
}

/// Counters of every queue, for monitoring and tests.
#[derive(Clone, Debug)]
pub struct PipelineStats {
    /// One entry per queue of the module table.
    pub queues: Vec<QueueStats>,
    /// Audio frames the recogniser has taken from the capture queue.
    pub frames_consumed: u64,
}

/// The reading end of the events a pipeline produces.
#[derive(Clone)]
pub struct EventReceiver {
    queue: Arc<BoundedQueue<PipelineEvent>>,
}

impl EventReceiver {
    /// The next event; `None` once the pipeline stopped and every event was read.
    pub fn recv(&self) -> Option<PipelineEvent> {
        let mut seen = 0;
        loop {
            match self.queue.pop(Wait::Forever, &mut seen) {
                Popped::Item(e) => return Some(e),
                Popped::Closed => return None,
                Popped::Woken | Popped::Timeout => {}
            }
        }
    }

    /// The next event if one is waiting.
    pub fn try_recv(&self) -> Option<PipelineEvent> {
        self.queue.try_pop()
    }
}

/// Everything [`DirectionPipeline::start`] returns.
pub struct Started {
    /// The running pipeline.
    pub pipeline: DirectionPipeline,
    /// Its events: audio, output changes, failures, cancellations.
    pub events: EventReceiver,
    /// Its snapshots, latest wins.
    pub snapshots: SnapshotSubscriber,
}

/// The queues of the module table, built from the configured capacities.
struct Queues {
    audio_in: Arc<BoundedQueue<AudioFrame>>,
    inbox: Arc<BoundedQueue<Inbound>>,
    refine_in: Arc<BoundedQueue<prxvc_media::Envelope<super::messages::RefineJob>>>,
    mt_in: Arc<BoundedQueue<MtItem>>,
    tts_in: Arc<BoundedQueue<TtsItem>>,
    events: Arc<BoundedQueue<PipelineEvent>>,
    /// Present only when re-translation is on.
    draft: Option<DraftQueues>,
}

impl Queues {
    fn new(params: &PipelineParams) -> Self {
        let caps = params.queues;
        let audio_ms = usize::try_from(caps.audio_in.as_millis()).unwrap_or(usize::MAX);
        Self {
            audio_in: Arc::new(BoundedQueue::weighted(
                "audio_in",
                audio_ms,
                Policy::DropOldest,
                |f: &AudioFrame| {
                    usize::try_from(f.duration().as_millis())
                        .unwrap_or(1)
                        .max(1)
                },
            )),
            inbox: Arc::new(BoundedQueue::new("inbox", caps.inbox, Policy::Block)),
            refine_in: Arc::new(BoundedQueue::new(
                "refine_in",
                caps.refine_in,
                Policy::DropOldest,
            )),
            mt_in: Arc::new(BoundedQueue::new("mt_in", caps.mt_in, Policy::DropOldest)),
            tts_in: Arc::new(BoundedQueue::new("tts_in", caps.tts_in, Policy::Block)),
            events: Arc::new(BoundedQueue::new(
                "events_out",
                caps.events_out,
                Policy::Block,
            )),
            draft: params
                .retranslate
                .enabled
                .then(|| DraftQueues::new(caps.draft_in, caps.draft_out)),
        }
    }
}

/// What every stage thread of one direction shares.
struct StageContext {
    params: PipelineParams,
    tree: Arc<TokenTree>,
    tracer: Tracer,
    stats: Arc<Stats>,
    gate: Arc<Gate>,
}

/// One direction of the call as a running pipeline. See the module
/// documentation for the threads and queues.
pub struct DirectionPipeline {
    tree: Arc<TokenTree>,
    inbox: Arc<BoundedQueue<Inbound>>,
    audio_in: Arc<BoundedQueue<AudioFrame>>,
    refine_in: Arc<BoundedQueue<prxvc_media::Envelope<super::messages::RefineJob>>>,
    mt_in: Arc<BoundedQueue<MtItem>>,
    tts_in: Arc<BoundedQueue<TtsItem>>,
    events: Arc<BoundedQueue<PipelineEvent>>,
    draft: Option<DraftQueues>,
    input: AudioInput,
    gate: Arc<Gate>,
    outbox: Outbox,
    stats: Arc<Stats>,
    workers: Arc<Threads>,
    control: Threads,
    stopped: AtomicBool,
}

impl DirectionPipeline {
    /// Start the threads of one direction. `call` is the token of the call
    /// session; cancelling it stops every direction below it. The pipeline
    /// takes a child of it, so stopping this pipeline leaves the call alone.
    pub fn start(
        params: PipelineParams,
        providers: PipelineProviders,
        clock: Arc<dyn Clock>,
        trace: Arc<dyn TraceSink>,
        call: &CancellationToken,
    ) -> Result<Started, PipelineError> {
        if params.refines() && providers.refine.is_none() {
            return Err(PipelineError::MissingRefine);
        }
        let q = Queues::new(&params);
        let tree = TokenTree::new(call.child_token());
        let stats = Arc::new(Stats::default());
        let gate = Arc::new(Gate::default());
        let tracer = Tracer::new(trace, Arc::clone(&clock));
        let (publisher, subscriber) = snapshot_channel();
        let core = Core::new(
            Wiring {
                tree: Arc::clone(&tree),
                inbox: Arc::clone(&q.inbox),
                audio_in: Arc::clone(&q.audio_in),
                refine_in: Arc::clone(&q.refine_in),
                mt_in: Arc::clone(&q.mt_in),
                tts_in: Arc::clone(&q.tts_in),
                events: Arc::clone(&q.events),
                gate: Arc::clone(&gate),
                snapshots: publisher,
                stats: Arc::clone(&stats),
                tracer: tracer.clone(),
            },
            params.clone(),
        );
        let pipeline = Self {
            input: AudioInput::new(Arc::clone(&q.audio_in), Arc::clone(&stats)),
            gate: Arc::clone(&gate),
            tree: Arc::clone(&tree),
            inbox: Arc::clone(&q.inbox),
            audio_in: Arc::clone(&q.audio_in),
            refine_in: Arc::clone(&q.refine_in),
            mt_in: Arc::clone(&q.mt_in),
            tts_in: Arc::clone(&q.tts_in),
            events: Arc::clone(&q.events),
            draft: q.draft.clone(),
            outbox: Outbox::new(Arc::clone(&q.inbox)),
            stats: Arc::clone(&stats),
            workers: Arc::new(Threads::new()),
            control: Threads::new(),
            stopped: AtomicBool::new(false),
        };
        // `Start` is the first item of the inbox, before any worker can report.
        pipeline.command(Command::Start);
        let name = |stage: &str| format!("prxvc-{:?}-{stage}", params.direction).to_lowercase();
        let control_inbox = Arc::clone(&q.inbox);
        pipeline
            .control
            .spawn(name("control"), move || {
                let mut core = run_control(core, &control_inbox, &*clock);
                core.shutdown();
            })
            .map_err(|e| spawn_error("control", &e))?;
        let ctx = StageContext {
            params,
            tree,
            tracer,
            stats,
            gate,
        };
        pipeline.spawn_workers(&ctx, providers, &q)?;
        Ok(Started {
            pipeline,
            events: EventReceiver { queue: q.events },
            snapshots: subscriber,
        })
    }

    /// Start the stage threads: recognise, translate, synthesise and, in two-pass mode, refine.
    fn spawn_workers(
        &self,
        ctx: &StageContext,
        providers: PipelineProviders,
        q: &Queues,
    ) -> Result<(), PipelineError> {
        let (params, tree, tracer, stats) = (&ctx.params, &ctx.tree, &ctx.tracer, &ctx.stats);
        let name = |stage: &str| format!("prxvc-{:?}-{stage}", params.direction).to_lowercase();
        let w = &self.workers;
        let recognizer = RecognizerThread {
            vad: providers.vad,
            asr: providers.asr,
            p: params.clone(),
            tree: Arc::clone(tree),
            tracer: tracer.clone(),
            stats: Arc::clone(stats),
            outbox: self.outbox.clone(),
            audio_in: Arc::clone(&q.audio_in),
            gate: Arc::clone(&ctx.gate),
            draft: q.draft.clone(),
        };
        w.spawn(name("recognize"), move || recognizer.run())
            .map_err(|e| spawn_error("recognize", &e))?;
        let cache = DraftCache::for_params(&params.retranslate);
        if let Some(draft) = q.draft.clone() {
            let worker = DraftWorker {
                mt: Arc::clone(&providers.mt),
                p: params.clone(),
                // Shared with the translation stage when it continues sentences; the worker's
                // own otherwise.
                cache: cache.clone().unwrap_or_default(),
                tracer: tracer.clone(),
                stats: Arc::clone(stats),
                warned: None,
            };
            w.spawn(name("draft"), move || {
                worker.run(&draft.input, &draft.output);
            })
            .map_err(|e| spawn_error("draft", &e))?;
        }
        let mt = MtStage {
            mt: providers.mt,
            p: params.clone(),
            outbox: self.outbox.clone(),
            input: Arc::clone(&q.mt_in),
            tts_in: Arc::clone(&q.tts_in),
            call: tree.call().clone(),
            tracer: tracer.clone(),
            stats: Arc::clone(stats),
            cache,
        };
        w.spawn(name("translate"), move || mt.run())
            .map_err(|e| spawn_error("translate", &e))?;
        let tts = TtsStage {
            tts: providers.tts,
            p: params.clone(),
            outbox: self.outbox.clone(),
            input: Arc::clone(&q.tts_in),
            call: tree.call().clone(),
            tracer: tracer.clone(),
            stats: Arc::clone(stats),
        };
        w.spawn(name("synthesize"), move || tts.run())
            .map_err(|e| spawn_error("synthesize", &e))?;
        if let Some(refine) = providers.refine.filter(|_| params.refines()) {
            let stage = RefineStage {
                refine,
                lang: params.source,
                hotwords: params.hotwords.clone(),
                outbox: self.outbox.clone(),
                input: Arc::clone(&q.refine_in),
                tracer: tracer.clone(),
            };
            w.spawn(name("refine"), move || stage.run())
                .map_err(|e| spawn_error("refine", &e))?;
        }
        Ok(())
    }

    /// The audio input. Frames must be at the VAD engine's rate.
    pub fn audio(&self) -> AudioInput {
        self.input.clone()
    }

    /// Pull frames from `source` on a thread of the pipeline until it stops.
    /// The source's end or failure is reported as [`PipelineEvent::CaptureEnded`].
    pub fn attach_source(&self, source: Box<dyn AudioSource>) -> Result<(), PipelineError> {
        let input = self.input.clone();
        let stop = self.tree.call().clone();
        let outbox = self.outbox.clone();
        self.workers
            .spawn("prxvc-capture".to_owned(), move || {
                run_capture(source, &input, &stop, &outbox);
            })
            .map_err(|e| spawn_error("capture", &e))
    }

    fn command(&self, c: Command) {
        if !matches!(
            self.inbox.push_urgent(Inbound::Command(c)),
            Pushed::Accepted
        ) {
            tracing::debug!("command refused: the pipeline is stopping");
        }
    }

    /// Pause the direction: everything in flight is cancelled and the
    /// configured passthrough is announced (`PipelineEvent::Output`).
    pub fn pause(&self) {
        self.command(Command::Pause);
    }

    /// Resume after a pause.
    pub fn resume(&self) {
        self.command(Command::Resume);
    }

    /// Suspend after a system event (sleep, lost endpoint); resume with [`DirectionPipeline::resume`].
    pub fn suspend(&self) {
        self.command(Command::Suspend);
    }

    /// Skip: cancel every in-flight and queued segment of this direction.
    pub fn skip(&self) {
        self.command(Command::Skip);
    }

    /// Cancel one segment, including a clause of it that is playing.
    pub fn cancel_segment(&self, segment: SegmentId) {
        self.command(Command::CancelSegment(segment));
    }

    /// Retry a failed direction now instead of at the next scheduled probe.
    pub fn retry(&self) {
        self.command(Command::Retry);
    }

    /// The playout side began to play `clause` at `at`.
    pub fn playback_started(&self, clause: ClauseId, at: MonoTime) {
        self.command(Command::PlaybackStarted { clause, at });
    }

    /// The playout side finished `clause` at `at`; the next clause is released.
    pub fn playback_finished(&self, clause: ClauseId, at: MonoTime) {
        self.command(Command::PlaybackFinished { clause, at });
    }

    /// A graceful stop: no new audio is accepted, the open segment is ended so that what was said
    /// last is committed, and everything already committed is translated, synthesised and
    /// played to its end (the caller keeps reporting playback). [`Self::drained`] says when it is
    /// all over.
    pub fn drain(&self) {
        self.input.drain();
        self.gate.finish_open.store(true, Ordering::SeqCst);
        self.audio_in.wake();
    }

    /// Whether the recogniser has ended the segment that was open at [`Self::drain`] (the commits
    /// it made are in the control thread's queue or beyond). With the snapshot showing nothing
    /// left in flight, the direction has said everything.
    pub fn drain_flushed(&self) -> bool {
        self.gate.flushed.load(Ordering::SeqCst)
    }

    /// Counters of every queue.
    pub fn stats(&self) -> PipelineStats {
        let mut queues = vec![
            self.audio_in.stats(),
            self.inbox.stats(),
            self.refine_in.stats(),
            self.mt_in.stats(),
            self.tts_in.stats(),
            self.events.stats(),
        ];
        if let Some(draft) = &self.draft {
            queues.push(draft.input.stats());
            queues.push(draft.output.stats());
        }
        PipelineStats {
            queues,
            frames_consumed: self.stats.frames_consumed.load(Ordering::Relaxed),
        }
    }

    /// Stop everything: cancel every request, close the queues, wait up to
    /// `timeout` (real time) for the threads and report what is left.
    /// Calling it again does nothing.
    pub fn stop(&self, timeout: Duration) -> StopReport {
        self.stop_inner(timeout)
    }

    fn stop_inner(&self, timeout: Duration) -> StopReport {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return StopReport {
                clean: true,
                leaked: 0,
            };
        }
        self.command(Command::Stop);
        self.tree.call().cancel();
        self.audio_in.close_discard();
        self.mt_in.close_discard();
        self.tts_in.close_discard();
        self.refine_in.close_discard();
        if let Some(draft) = &self.draft {
            draft.input.close_discard();
            draft.output.close_discard();
        }
        self.events.wake();
        let clean = self.workers.wait_idle(timeout);
        self.command(Command::Joined { clean });
        let mut control_done = self.control.wait_idle(timeout);
        if !control_done {
            // The control thread waits for a consumer that is gone: let go of its events.
            self.events.close();
            self.inbox.close();
            control_done = self.control.wait_idle(timeout);
        }
        self.workers.join_finished();
        self.control.join_finished();
        let leaked = usize::from(!clean) + usize::from(!control_done);
        StopReport {
            clean: clean && control_done,
            leaked,
        }
    }
}

impl Drop for DirectionPipeline {
    fn drop(&mut self) {
        let _ = self.stop_inner(Duration::from_secs(2));
    }
}

fn spawn_error(stage: &'static str, e: &std::io::Error) -> PipelineError {
    PipelineError::Spawn {
        stage,
        reason: e.to_string(),
    }
}
