//! The thread bodies that are not stages of their own: the recogniser loop, the control loop and the capture pump.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use prxvc_media::{AudioFrame, Clock};
use prxvc_ports::{AsrContext, AudioSource, CancellationToken, StreamingAsr, VadProvider};

use super::core::{Core, Gate};
use super::draft::{DraftQueues, ThreadedLink};
use super::faults::{asr_failure, vad_failure};
use super::input::AudioInput;
use super::messages::{Inbound, StageFailure, WorkerEvent};
use super::outbox::Outbox;
use super::params::PipelineParams;
use super::queue::{BoundedQueue, Popped, Wait};
use super::recognition::RecognitionStage;
use super::snapshot::Stats;
use super::trace::Tracer;
use super::tree::TokenTree;
use crate::fault::Stage;

/// The threads of one pipeline, with a count that tells when they all ended.
pub(crate) struct Threads {
    live: Arc<(Mutex<usize>, Condvar)>,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

/// Decrements the live count when its thread ends, however it ends.
struct Alive(Arc<(Mutex<usize>, Condvar)>);

impl Drop for Alive {
    fn drop(&mut self) {
        *self.0.0.lock() -= 1;
        self.0.1.notify_all();
    }
}

impl Threads {
    pub(crate) fn new() -> Self {
        Self {
            live: Arc::new((Mutex::new(0), Condvar::new())),
            handles: Mutex::new(Vec::new()),
        }
    }

    /// Start a named thread that this registry waits for.
    pub(crate) fn spawn(
        &self,
        name: String,
        body: impl FnOnce() + Send + 'static,
    ) -> std::io::Result<()> {
        *self.live.0.lock() += 1;
        let alive = Alive(Arc::clone(&self.live));
        let spawned = thread::Builder::new().name(name).spawn(move || {
            let _alive = alive;
            body();
        });
        match spawned {
            Ok(handle) => {
                self.handles.lock().push(handle);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Wait up to `timeout` (a real-time bound, not a pipeline timer) until
    /// every thread ended; returns whether they did.
    pub(crate) fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut live = self.live.0.lock();
        while *live > 0 {
            if self.live.1.wait_until(&mut live, deadline).timed_out() {
                return *live == 0;
            }
        }
        true
    }

    /// Join the threads that ended; the handles of threads still running are dropped (detached).
    pub(crate) fn join_finished(&self) {
        for handle in self.handles.lock().drain(..) {
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

/// The control thread: feed the core until it has finished the stop.
pub(crate) fn run_control(
    mut core: Core,
    inbox: &BoundedQueue<Inbound>,
    clock: &dyn Clock,
) -> Core {
    let mut seen = 0;
    loop {
        let wait = match core.next_deadline() {
            Some(due) => Wait::Until(due, clock),
            None => Wait::Forever,
        };
        match inbox.pop(wait, &mut seen) {
            Popped::Item(inbound) => core.handle(inbound),
            Popped::Timeout => core.on_timer(clock.now()),
            Popped::Woken => {}
            Popped::Closed => break,
        }
        if core.is_finished() {
            break;
        }
    }
    core
}

/// The providers and queues of the recognition thread.
pub(crate) struct RecognizerThread {
    pub(crate) vad: Arc<dyn VadProvider>,
    pub(crate) asr: Arc<dyn StreamingAsr>,
    pub(crate) p: PipelineParams,
    pub(crate) tree: Arc<TokenTree>,
    pub(crate) tracer: Tracer,
    pub(crate) stats: Arc<Stats>,
    pub(crate) outbox: Outbox,
    pub(crate) audio_in: Arc<BoundedQueue<AudioFrame>>,
    pub(crate) gate: Arc<Gate>,
    /// The queues of the draft worker, when re-translation is on.
    pub(crate) draft: Option<DraftQueues>,
}

/// What the loop keeps between frames.
struct LoopState {
    stage: Option<RecognitionStage>,
    failed_stage: Stage,
    was_active: bool,
    overrun: bool,
    clean_frames: u32,
    evicted_seen: u64,
}

impl RecognizerThread {
    fn open(&self) -> Result<RecognitionStage, (Stage, StageFailure)> {
        let vad = self
            .vad
            .open()
            .map_err(|e| (Stage::Vad, vad_failure(&e, false)))?;
        let ctx = AsrContext {
            previous_text: None,
            hotwords: self.p.hotwords.clone(),
        };
        let asr = self
            .asr
            .open(self.p.source, ctx)
            .map_err(|e| (Stage::Asr, asr_failure(&e, false)))?;
        let stage = RecognitionStage::new(
            &self.p,
            Arc::clone(&self.tree),
            self.tracer.clone(),
            Arc::clone(&self.stats),
            vad,
            asr,
        );
        Ok(match &self.draft {
            Some(queues) => {
                stage.with_drafts(Box::new(ThreadedLink::new(queues, Arc::clone(&self.stats))))
            }
            None => stage,
        })
    }

    /// The thread body.
    pub(crate) fn run(self) {
        let mut st = LoopState {
            stage: None,
            failed_stage: Stage::Vad,
            was_active: false,
            overrun: false,
            clean_frames: 0,
            evicted_seen: 0,
        };
        match self.open() {
            Ok(stage) => {
                st.stage = Some(stage);
                self.outbox.worker(WorkerEvent::RecognizerReady);
            }
            Err((stage, failure)) => {
                st.failed_stage = stage;
                self.outbox
                    .worker(WorkerEvent::RecognizerFailed { stage, failure });
            }
        }
        let mut seen = 0;
        loop {
            match self.audio_in.pop(Wait::Forever, &mut seen) {
                Popped::Closed => break,
                Popped::Woken | Popped::Timeout => self.handle_gate(&mut st),
                Popped::Item(frame) => self.on_frame(&mut st, &frame),
            }
            self.finish_if_asked(&mut st);
        }
    }

    /// A request of the control thread: reset the stream or probe the engines.
    fn handle_gate(&self, st: &mut LoopState) {
        if self.gate.reset.swap(false, Ordering::SeqCst)
            && let Some(stage) = st.stage.as_mut()
        {
            stage.reset();
        }
        if self.gate.probe.swap(false, Ordering::SeqCst) {
            let result = match self.open() {
                Ok(stage) => {
                    st.stage = Some(stage);
                    Ok(())
                }
                Err((stage, failure)) => {
                    st.failed_stage = stage;
                    Err(failure)
                }
            };
            self.outbox.worker(WorkerEvent::ProbeResult {
                stage: st.failed_stage,
                result,
            });
        }
    }

    /// A graceful stop: once the queued audio is processed, end the open segment so that what the
    /// speaker said last is committed. Then the flag the coordinator waits for is set.
    fn finish_if_asked(&self, st: &mut LoopState) {
        if !self.gate.finish_open.load(Ordering::SeqCst) || self.audio_in.len() > 0 {
            return;
        }
        self.gate.finish_open.store(false, Ordering::SeqCst);
        if let Some(stage) = st.stage.as_mut()
            && self.gate.is_active()
        {
            let mut out = Vec::new();
            let result = stage.finish_open(&mut out);
            self.forward(out);
            if let Err(fault) = result {
                st.failed_stage = fault.stage;
                st.stage = None;
                self.outbox.worker(WorkerEvent::RecognizerFailed {
                    stage: fault.stage,
                    failure: fault.failure,
                });
            }
        }
        self.gate.flushed.store(true, Ordering::SeqCst);
    }

    fn forward(&self, out: Vec<prxvc_media::Envelope<super::messages::RecognitionEvent>>) {
        for envelope in out {
            let token = self
                .tree
                .segment(envelope.segment)
                .unwrap_or_else(|| self.tree.direction());
            self.outbox.recognition(envelope, &token);
        }
    }

    fn on_frame(&self, st: &mut LoopState, frame: &AudioFrame) {
        self.stats.frames_consumed.fetch_add(1, Ordering::Relaxed);
        self.track_overrun(st);
        if !self.gate.is_active() {
            if std::mem::take(&mut st.was_active)
                && let Some(stage) = st.stage.as_mut()
            {
                stage.reset();
            }
            return;
        }
        st.was_active = true;
        let Some(stage) = st.stage.as_mut() else {
            return;
        };
        let mut out = Vec::new();
        let result = stage.on_frame(frame, &mut out);
        self.forward(out);
        if let Err(fault) = result {
            st.failed_stage = fault.stage;
            st.stage = None;
            self.outbox.worker(WorkerEvent::RecognizerFailed {
                stage: fault.stage,
                failure: fault.failure,
            });
        }
    }

    /// A capture overrun is raised when the queue in front of this thread drops frames and cleared after a clean stretch of audio.
    fn track_overrun(&self, st: &mut LoopState) {
        let evicted = self.audio_in.stats().evicted;
        if evicted > st.evicted_seen {
            st.evicted_seen = evicted;
            st.clean_frames = 0;
            if !st.overrun {
                st.overrun = true;
                self.outbox.worker(WorkerEvent::Overrun);
            }
        } else if st.overrun {
            st.clean_frames += 1;
            if st.clean_frames >= self.p.overrun_clear_frames {
                st.overrun = false;
                self.outbox.worker(WorkerEvent::OverrunCleared);
            }
        }
    }
}

/// Pull frames from an endpoint into the capture queue until stopped.
pub(crate) fn run_capture(
    mut source: Box<dyn AudioSource>,
    input: &AudioInput,
    stop: &CancellationToken,
    outbox: &Outbox,
) {
    if let Err(e) = source.start() {
        outbox.worker(WorkerEvent::CaptureEnded(Some(e)));
        return;
    }
    let mut frames = Vec::new();
    let mut error = None;
    while !stop.is_cancelled() {
        frames.clear();
        if let Err(e) = source.read(&mut frames, Duration::from_millis(20)) {
            error = Some(e);
            break;
        }
        if frames.is_empty() {
            thread::yield_now();
        }
        for frame in frames.drain(..) {
            input.push(frame);
        }
    }
    source.stop();
    outbox.worker(WorkerEvent::CaptureEnded(error));
}
