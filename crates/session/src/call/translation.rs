//! The translation pipelines of a running call: starting one per translated direction, connecting it to its path, carrying audio and events between them, and stopping it.
//!
//! # What runs where
//!
//! ```text
//!  path thread                      pipeline threads                 forwarder thread            coordinator thread
//!  frames -> on_frame -> audio().push ........ recognise .. translate .. synthesise
//!                                                    |  control: direction machine, segment book
//!                                                    v
//!                                      events: Audio, AudioEnd, Flush ---> playout (mixer lane)
//!                                              State, Output, Degraded,
//!                                              Failed, Recovered .........> channel ............> tick: apply
//!  mixer lane drains -> on_playback -> pipeline.playback_started/finished
//! ```
//!
//! * Audio never waits for the coordinator: the forwarder thread hands every
//!   `Audio`, `AudioEnd` and `Flush` event to the path's playout at once.
//! * What each listener hears is decided by the direction machine inside the
//!   pipeline; its `Output` events reach the coordinator through a bounded
//!   channel and are executed at the next tick ([`Coordinator::collect_translation`]),
//!   through the same `announce_output` as in audio-only mode, so that the snapshot
//!   shows every change and the mixer crossfades.
//! * The coordinator's own direction machine of a translated lane mirrors the
//!   pipeline's state (`PipelineEvent::State`); pause, resume, suspend and stop
//!   are commands to the pipeline, never fed to the mirror twice.
//! * Playback is reported by the path's mixer: `playback_started` when the lane hands
//!   out the first sample of a clause, `playback_finished` when it has drained.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_media::Direction;
use prxvc_ports::CancellationToken;

use super::coordinator::{BOTH, Coordinator, slot};
use super::error::{CallErrorCode, HostFault};
use super::host::{
    DirectionBuild, OutputRoute, PathLink, PlaybackReport, Playout, PreparedProviders,
};
use crate::call_state::CallEvent;
use crate::direction_state::DirectionState;
use crate::interrupt::CancelAction;
use crate::pipeline::{DirectionPipeline, PipelineEvent, SnapshotSubscriber, Started};

/// How long stopping a pipeline may wait for its threads.
const PIPELINE_STOP: Duration = Duration::from_secs(2);

/// Events for the coordinator that wait in the channel between two ticks.
const FORWARD_CAPACITY: usize = 512;

/// A running translated direction.
pub(super) struct Running {
    pipeline: Arc<DirectionPipeline>,
    events: Receiver<PipelineEvent>,
    snapshots: SnapshotSubscriber,
    playout: Arc<Mutex<Option<Arc<dyn Playout>>>>,
    forwarder: Option<JoinHandle<()>>,
    /// A degradation or failure was reported to the call machine and not yet cleared.
    pub(super) reported_problem: bool,
    /// The rate the pipeline's VAD wants its frames at.
    tap_rate: prxvc_media::SampleRate,
    languages: (prxvc_media::Lang, prxvc_media::Lang),
}

impl Running {
    /// The pipeline's newest snapshot.
    pub(super) fn snapshot(&self) -> Option<Arc<crate::pipeline::DirectionSnapshot>> {
        self.snapshots.latest()
    }

    /// The language spoken and the language heard.
    pub(super) fn languages(&self) -> (prxvc_media::Lang, prxvc_media::Lang) {
        self.languages
    }

    /// The lane the direction's translated audio goes to, once the path is linked.
    pub(super) fn playout(&self) -> Option<Arc<dyn Playout>> {
        self.playout.lock().clone()
    }

    pub(super) fn pause(&self) {
        self.pipeline.pause();
    }

    pub(super) fn resume(&self) {
        self.pipeline.resume();
    }

    pub(super) fn suspend(&self) {
        self.pipeline.suspend();
    }

    pub(super) fn skip(&self) {
        self.pipeline.skip();
    }
}

impl Coordinator {
    /// `StartDirections`: start a pipeline for every direction the providers were prepared for.
    /// A direction without a pipeline stays off and its listener keeps hearing the original.
    pub(super) fn start_translation(&mut self) {
        let Some(PreparedProviders { directions, trace }) = self.prepared.take() else {
            tracing::warn!("translation was requested but no providers were prepared");
            return;
        };
        for build in directions {
            let d = build.params.direction;
            if let Err(fault) = self.start_pipeline(build, &trace) {
                self.raise_fault(&fault);
                // The direction machine decides what the listener hears now (`call.*_on_failure`).
                self.apply_failure_policy(d);
            }
        }
    }

    fn start_pipeline(
        &mut self,
        build: DirectionBuild,
        trace: &Arc<dyn prxvc_ports::TraceSink>,
    ) -> Result<(), HostFault> {
        let d = build.params.direction;
        let tap_rate = vad_rate(&build);
        let languages = (build.params.source, build.params.target);
        if d == Direction::Outgoing {
            self.start_disclosure(&build);
        }
        let Started {
            pipeline,
            events,
            snapshots,
        } = DirectionPipeline::start(
            build.params,
            build.providers,
            Arc::clone(&self.clock),
            Arc::clone(trace),
            &self.call_cancel,
        )
        .map_err(|e| HostFault::new(CallErrorCode::TranslationUnavailable, e.to_string()))?;
        let pipeline = Arc::new(pipeline);
        let playout: Arc<Mutex<Option<Arc<dyn Playout>>>> = Arc::default();
        let (tx, rx) = sync_channel(FORWARD_CAPACITY);
        let notice = (d == Direction::Outgoing).then(|| self.disclosure.clone()).flatten();
        let forwarder = spawn_forwarder(d, events, Arc::clone(&playout), tx, notice)?;
        self.lanes[slot(d)].running = Some(Running {
            pipeline,
            events: rx,
            snapshots,
            playout,
            forwarder: Some(forwarder),
            reported_problem: false,
            tap_rate,
            languages,
        });
        if let Err(fault) = self.link_path(d) {
            self.stop_translation(d);
            return Err(fault);
        }
        // The lane's machine follows the pipeline's from its first event on.
        self.lanes[slot(d)].machine = DirectionState::Starting {
            after_failure: false,
        };
        Ok(())
    }

    /// Connect the lane's path to its pipeline (also after the path was replaced).
    pub(super) fn link_path(&mut self, d: Direction) -> Result<(), HostFault> {
        let clock = Arc::clone(&self.clock);
        let lane = &mut self.lanes[slot(d)];
        let (Some(running), Some(path)) = (lane.running.as_ref(), lane.path.as_mut()) else {
            return Err(HostFault::new(
                CallErrorCode::TranslationUnavailable,
                format!("the {d:?} path is not running"),
            ));
        };
        let input = running.pipeline.audio();
        let on_playback = {
            let pipeline = Arc::clone(&running.pipeline);
            let notice = (d == Direction::Outgoing).then(|| self.disclosure.clone()).flatten();
            let now = Arc::clone(&clock);
            Box::new(move |report: PlaybackReport| {
                // The spoken notice is the coordinator's, not a clause of the pipeline.
                if notice.as_ref().is_some_and(|n| n.report(report, now.now())) {
                    return;
                }
                match report {
                    PlaybackReport::Started { clause, at } => pipeline.playback_started(clause, at),
                    PlaybackReport::Finished { clause, at } => {
                        pipeline.playback_finished(clause, at);
                    }
                }
            })
        };
        let playout = path.connect(PathLink {
            tap_rate: running.tap_rate,
            on_frame: Box::new(move |frame| {
                let _ = input.push(frame);
            }),
            on_playback,
            clock,
        })?;
        *running.playout.lock() = Some(playout);
        Ok(())
    }

    /// Apply what the pipelines reported since the last tick.
    pub(super) fn collect_translation(&mut self) {
        for d in BOTH {
            loop {
                let event = match self.lanes[slot(d)].running.as_ref() {
                    Some(r) => r.events.try_recv().ok(),
                    None => None,
                };
                let Some(event) = event else { break };
                self.on_pipeline_event(d, event);
            }
        }
    }

    fn on_pipeline_event(&mut self, d: Direction, event: PipelineEvent) {
        match event {
            PipelineEvent::State(state) => {
                let healthy = matches!(state, DirectionState::Running);
                let lane = &mut self.lanes[slot(d)];
                lane.machine = state;
                if healthy && lane.running.as_ref().is_some_and(|r| r.reported_problem) {
                    if let Some(r) = lane.running.as_mut() {
                        r.reported_problem = false;
                    }
                    self.feed(CallEvent::DirectionHealthy(d));
                }
            }
            PipelineEvent::Output { from, to, cause } => self.announce_output(d, from, to, cause),
            PipelineEvent::Passthrough { on } => self.on_passthrough(d, on),
            PipelineEvent::Degraded(reason) => {
                self.mark_problem(d);
                self.feed(CallEvent::DirectionDegraded {
                    direction: d,
                    reason,
                });
            }
            PipelineEvent::Recovered(reason) => {
                tracing::info!(direction = ?d, ?reason, "a reduced mode ended");
            }
            PipelineEvent::Failed { stage, fault } => {
                self.mark_problem(d);
                self.feed(CallEvent::DirectionFailed {
                    direction: d,
                    stage,
                    fault,
                });
            }
            PipelineEvent::CaptureEnded(error) => {
                tracing::warn!(direction = ?d, ?error, "the pipeline's attached source ended");
            }
            // Handled on the forwarder thread, or shown through the snapshot.
            PipelineEvent::Audio(_)
            | PipelineEvent::AudioEnd(_)
            | PipelineEvent::Flush(_)
            | PipelineEvent::ClauseSkipped { .. }
            | PipelineEvent::ClauseFailed { .. } => {}
        }
    }

    fn mark_problem(&mut self, d: Direction) {
        if let Some(r) = self.lanes[slot(d)].running.as_mut() {
            r.reported_problem = true;
        }
    }

    /// Stop one pipeline: flush what plays, end the threads, apply the last events (the listener
    /// returns to the original), disconnect the path and let go of the providers.
    pub(super) fn stop_translation(&mut self, d: Direction) {
        let Some(playout) = self.lanes[slot(d)]
            .running
            .as_ref()
            .map(|r| Arc::clone(&r.playout))
        else {
            return;
        };
        if let Some(p) = playout.lock().as_ref() {
            p.flush();
        }
        let pipeline = self.lanes[slot(d)]
            .running
            .as_ref()
            .map(|r| Arc::clone(&r.pipeline));
        if let Some(pipeline) = pipeline {
            let report = pipeline.stop(PIPELINE_STOP);
            if !report.clean {
                self.leaked = true;
                self.raise(
                    CallErrorCode::StopLeak,
                    format!("{d:?} pipeline: {} thread(s) did not end", report.leaked),
                );
            }
        }
        if let Some(handle) = self.lanes[slot(d)]
            .running
            .as_mut()
            .and_then(|r| r.forwarder.take())
        {
            let _ = handle.join();
        }
        self.collect_translation();
        self.capture_totals(d);
        let lane = &mut self.lanes[slot(d)];
        if let Some(path) = lane.path.as_mut() {
            path.disconnect();
        }
        *playout.lock() = None;
        lane.running = None;
        lane.machine = DirectionState::Off;
    }

    /// Show the headphone warning while the translation plays on the built-in loudspeakers: the
    /// microphone would pick the translation up and the outgoing direction would translate it
    /// back (design section 6).
    pub(super) fn check_output_route(&mut self) {
        let route = self.lanes[slot(Direction::Incoming)]
            .stats
            .as_ref()
            .map(|s| (s.sink_route, s.sink_device.clone()));
        match route {
            Some((OutputRoute::Speakers, device)) if self.translate => {
                if !self.issues.iter().any(|i| i.code == CallErrorCode::SpeakerOutput) {
                    self.raise(
                        CallErrorCode::SpeakerOutput,
                        format!("local output: {}", device.unwrap_or_default()),
                    );
                }
            }
            _ => self.clear(CallErrorCode::SpeakerOutput),
        }
    }

    /// Cancel everything the call token covers.
    pub(super) fn cancel_call(&mut self) {
        self.call_cancel.cancel();
    }

    pub(super) fn new_call_token(&mut self) {
        self.call_cancel = CancellationToken::new();
    }
}

/// The sample rate the pipeline's VAD wants its frames at.
fn vad_rate(build: &DirectionBuild) -> prxvc_media::SampleRate {
    build
        .providers
        .vad
        .descriptor()
        .sample_rates
        .first()
        .copied()
        .unwrap_or(prxvc_media::SampleRate::HZ_16000)
}

fn spawn_forwarder(
    d: Direction,
    events: crate::pipeline::EventReceiver,
    playout: Arc<Mutex<Option<Arc<dyn Playout>>>>,
    tx: SyncSender<PipelineEvent>,
    notice: Option<Arc<super::disclosure::Disclosure>>,
) -> Result<JoinHandle<()>, HostFault> {
    std::thread::Builder::new()
        .name(format!("prxvc-call-fwd-{d:?}").to_lowercase())
        .spawn(move || forward(&events, &playout, &tx, notice.as_deref()))
        .map_err(|e| {
            HostFault::new(
                CallErrorCode::Internal,
                format!("cannot start the event forwarder: {e}"),
            )
        })
}

/// Carry audio to the playout at once and everything else to the coordinator.
fn forward(
    events: &crate::pipeline::EventReceiver,
    playout: &Mutex<Option<Arc<dyn Playout>>>,
    tx: &SyncSender<PipelineEvent>,
    notice: Option<&super::disclosure::Disclosure>,
) {
    let current = || playout.lock().clone();
    let mut last_clause = None;
    while let Some(event) = events.recv() {
        match event {
            PipelineEvent::Audio(a) => {
                if let Some(p) = current() {
                    // The first audio of a clause: the notice, when owed, goes in the lane first.
                    if let Some(n) = notice
                        && last_clause != Some(a.payload.clause)
                    {
                        n.before_audio(&*p);
                    }
                    p.push(a.payload.clause, &a.payload.chunk);
                }
                last_clause = Some(a.payload.clause);
            }
            PipelineEvent::AudioEnd(e) => {
                if let Some(p) = current() {
                    p.end_clause(e.payload);
                }
            }
            PipelineEvent::Flush(action) => {
                // A clause that has not started playing leaves nothing in the lane.
                if !matches!(action, CancelAction::CancelClause(_))
                    && let Some(p) = current()
                {
                    p.flush();
                    if let Some(n) = notice {
                        n.flushed();
                    }
                }
            }
            other => match tx.try_send(other) {
                Ok(()) => {}
                Err(TrySendError::Full(lost)) => {
                    tracing::warn!(event = ?lost, "the coordinator did not read events in time; one was dropped");
                }
                Err(TrySendError::Disconnected(_)) => return,
            },
        }
    }
}
