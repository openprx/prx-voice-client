//! The assistant's voice loop: microphone, speech detection, recognition, dialogue, synthesis and the reply's playback, on one loop thread and two short-lived workers per reply.
//!
//! The loop thread owns the microphone, the recognition stage (the direction pipeline's own: VAD,
//! segmenter, streaming recogniser, commit policy and filler filter), the state machine
//! ([`AssistantState`]) and the output. It reads frames, feeds the stage, turns what the stage
//! says into machine events, executes the machine's actions and plays the reply's audio as the
//! workers deliver it (`reply`). Time comes from the injected [`Clock`] and from the frames'
//! own timestamps, never from the system.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

use prxvc_media::{AudioFrame, Clock, MonoTime, SampleRate};
use prxvc_ports::{
    AsrContext, AsrError, AudioSource, CancellationToken, ConversationId, DialogueBackend,
    EndpointError, SpeechSynth, StreamingAsr, TraceSink, VadError, VadProvider, VoiceSelection,
};

use super::events::{AssistantEvent, Command, Phase};
use super::fault::AssistantFault;
use super::machine::{AssistantAction, AssistantInput, AssistantState};
use super::reply::{self, ReplyJob, ReplyKind, ReplyMessage, ReplyTools, ReplyWorkers};
use super::seams::{MicGate, MicState, ReplyOutput};
use crate::fault::Stage;
use crate::pipeline::{
    PipelineParams, RecognitionEvent, RecognitionFault, RecognitionStage, Stats, TokenTree, Tracer,
};

/// How long one read of the microphone may wait.
const READ_WAIT: Duration = Duration::from_millis(10);
/// How long the loop sleeps when a turn of it found nothing to do.
const IDLE_WAIT: Duration = Duration::from_millis(2);
/// Capacity of the queues between the loop and its host and workers.
const QUEUE: usize = 256;

/// The providers and endpoints a session runs on.
pub struct AssistantDeps {
    /// The microphone; frames at the voice activity detector's rate.
    pub source: Box<dyn AudioSource>,
    /// Voice activity detection.
    pub vad: Arc<dyn VadProvider>,
    /// Streaming recognition.
    pub asr: Arc<dyn StreamingAsr>,
    /// The dialogue backend.
    pub dialogue: Arc<dyn DialogueBackend>,
    /// Speech synthesis.
    pub tts: Arc<dyn SpeechSynth>,
    /// Where the reply plays.
    pub output: Box<dyn ReplyOutput>,
    /// Half duplex.
    pub gate: Box<dyn MicGate>,
    /// The session's clock.
    pub clock: Arc<dyn Clock>,
    /// Where stage boundaries are recorded (no call content unless the sink asks for it).
    pub trace: Arc<dyn TraceSink>,
}

/// What a session is configured with.
#[derive(Clone, Debug)]
pub struct AssistantParams {
    /// The conversation the dialogue backend keeps for this session.
    pub conversation: ConversationId,
    /// The language the user speaks and the assistant answers in.
    pub lang: prxvc_media::Lang,
    /// The voice the assistant speaks with.
    pub voice: VoiceSelection,
    /// The recognition stage's parameters (`[pipeline]` tuning with the assistant's end-of-turn
    /// silence). The direction is `Outgoing` and recognition is `Streaming`.
    pub recognition: PipelineParams,
}

impl AssistantParams {
    /// The parameters of a session in `lang` with `voice`, ending the user's turn after
    /// `end_silence` of silence; the rest of the recognition stage takes the pipeline defaults.
    pub fn new(
        conversation: ConversationId,
        lang: prxvc_media::Lang,
        voice: VoiceSelection,
        end_silence: Duration,
    ) -> Self {
        Self {
            conversation,
            lang,
            recognition: recognition_params(lang, voice.clone(), end_silence),
            voice,
        }
    }
}

/// The recognition stage's parameters for the assistant: one direction (`Outgoing`) that listens
/// to the user in `lang`, streaming, committing the whole turn in one decode at its end (no clause
/// commits at a shorter pause and no early commit from the stable prefix: the dialogue needs the
/// whole turn), the turn ending after `end_silence` of silence.
pub fn recognition_params(
    lang: prxvc_media::Lang,
    voice: VoiceSelection,
    end_silence: Duration,
) -> PipelineParams {
    let mut p = PipelineParams::new(prxvc_media::Direction::Outgoing, lang, lang, voice);
    p.recognition_mode = crate::pipeline::RecognitionMode::Streaming;
    p.commit.segment_end_pause = end_silence;
    p.commit.long_segment_pause = end_silence;
    p.commit.clause_pause = end_silence;
    p.commit.early = crate::EarlyCommit::SegmentEnd;
    p.commit.split_target_clauses = false;
    p
}

/// The session could not start; nothing runs and the microphone is closed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StartError {
    /// The voice activity detector could not be opened.
    #[error("speech detection cannot start: {0}")]
    Vad(VadError),
    /// The recogniser could not be opened.
    #[error("speech recognition cannot start: {0}")]
    Recognition(AsrError),
    /// The microphone could not be opened.
    #[error("the microphone cannot be opened: {0}")]
    Microphone(EndpointError),
    /// The microphone delivers a rate the detector does not take.
    #[error("the microphone delivers {got} and the speech detector needs {expected}")]
    Rate {
        /// What the detector needs.
        expected: SampleRate,
        /// What the microphone delivers.
        got: SampleRate,
    },
    /// The loop thread could not be created.
    #[error("the assistant thread cannot start: {0}")]
    Thread(String),
}

/// A running session.
pub struct AssistantHandle {
    commands: SyncSender<Command>,
    events: Receiver<AssistantEvent>,
    thread: Option<JoinHandle<()>>,
}

impl AssistantHandle {
    /// Cut what the assistant says now; the session goes on listening.
    pub fn interrupt(&self) {
        let _ = self.commands.try_send(Command::Interrupt);
    }

    /// End the session. The loop reports [`AssistantEvent::Ended`] when it is over.
    pub fn stop(&self) {
        let _ = self.commands.try_send(Command::Stop);
    }

    /// The next event, if one is ready.
    pub fn try_event(&self) -> Option<AssistantEvent> {
        self.events.try_recv().ok()
    }

    /// The next event, waiting up to `timeout`.
    pub fn next_event(&self, timeout: Duration) -> Option<AssistantEvent> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Whether the loop thread has ended.
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Wait for the loop thread to end, handing the remaining events to `on_event`. Call after
    /// [`AssistantHandle::stop`].
    pub fn join(mut self, mut on_event: impl FnMut(AssistantEvent)) {
        while let Some(thread) = self.thread.as_ref() {
            match self.events.recv_timeout(Duration::from_millis(20)) {
                Ok(e) => on_event(e),
                Err(RecvTimeoutError::Timeout) if !thread.is_finished() => {}
                Err(_) => break,
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        while let Ok(e) = self.events.try_recv() {
            on_event(e);
        }
    }
}

/// Start a session: open the detector, the recogniser and the microphone, then run the loop.
///
/// # Errors
/// [`StartError`] for the first thing that could not be opened; everything opened before it is
/// closed again.
pub fn start(deps: AssistantDeps, params: AssistantParams) -> Result<AssistantHandle, StartError> {
    let AssistantDeps {
        mut source,
        vad,
        asr,
        dialogue,
        tts,
        output,
        gate,
        clock,
        trace,
    } = deps;
    let vad_engine = vad.open().map_err(StartError::Vad)?;
    let stream = asr
        .open(
            params.lang,
            AsrContext {
                previous_text: None,
                hotwords: params.recognition.hotwords.clone(),
            },
        )
        .map_err(StartError::Recognition)?;
    let expected = vad_engine.rate();
    let info = source.start().map_err(StartError::Microphone)?;
    if info.rate != expected {
        source.stop();
        return Err(StartError::Rate {
            expected,
            got: info.rate,
        });
    }
    let tracer = Tracer::new(Arc::clone(&trace), Arc::clone(&clock));
    let tree = TokenTree::new(CancellationToken::new());
    let stage = RecognitionStage::new(
        &params.recognition,
        tree,
        tracer,
        Arc::new(Stats::default()),
        vad_engine,
        stream,
    );
    let (command_tx, command_rx) = sync_channel(16);
    let (event_tx, event_rx) = sync_channel(QUEUE);
    let (reply_tx, reply_rx) = sync_channel(QUEUE);
    let session = Session {
        source,
        stage,
        state: AssistantState::Idle,
        dialogue: Arc::clone(&dialogue),
        output,
        gate,
        clock: Arc::clone(&clock),
        tools: ReplyTools {
            dialogue,
            tts,
            trace,
            clock,
        },
        params,
        events: event_tx,
        commands: command_rx,
        reply_rx,
        reply_tx,
        reply: None,
        turns: 0,
        phase: Phase::Idle,
        segment_text: Vec::new(),
        mic_paused: false,
        device: info.device,
        done: false,
    };
    let thread = std::thread::Builder::new()
        .name("prxvc-assistant".into())
        .spawn(move || session.run())
        .map_err(|e| StartError::Thread(e.to_string()))?;
    Ok(AssistantHandle {
        commands: command_tx,
        events: event_rx,
        thread: Some(thread),
    })
}

/// The reply of the current turn.
struct Reply {
    turn: u64,
    token: CancellationToken,
    workers: Option<ReplyWorkers>,
    /// The first audio reached the output.
    audible: bool,
    /// The workers produced all the audio and the output was told so.
    audio_complete: bool,
}

struct Session {
    source: Box<dyn AudioSource>,
    stage: RecognitionStage,
    state: AssistantState,
    dialogue: Arc<dyn DialogueBackend>,
    output: Box<dyn ReplyOutput>,
    gate: Box<dyn MicGate>,
    clock: Arc<dyn Clock>,
    tools: ReplyTools,
    params: AssistantParams,
    events: SyncSender<AssistantEvent>,
    commands: Receiver<Command>,
    reply_rx: Receiver<ReplyMessage>,
    reply_tx: SyncSender<ReplyMessage>,
    reply: Option<Reply>,
    turns: u64,
    phase: Phase,
    /// The committed texts of the speech segment being heard.
    segment_text: Vec<String>,
    mic_paused: bool,
    device: String,
    done: bool,
}

impl Session {
    fn run(mut self) {
        self.emit(AssistantEvent::Started {
            device: self.device.clone(),
        });
        self.apply(AssistantInput::Start);
        let mut frames = Vec::new();
        while !self.done {
            let mut busy = self.take_commands();
            busy |= self.take_reply_messages();
            self.tick_output();
            if self.done {
                break;
            }
            frames.clear();
            if let Err(e) = self.source.read(&mut frames, READ_WAIT) {
                self.apply(AssistantInput::Fatal(AssistantFault::Microphone(e)));
                break;
            }
            busy |= !frames.is_empty();
            for frame in frames.drain(..) {
                if self.done {
                    break;
                }
                self.on_frame(&frame);
            }
            if !busy && !self.done {
                std::thread::sleep(IDLE_WAIT);
            }
        }
        self.finish();
    }

    fn now(&self) -> MonoTime {
        self.clock.now()
    }

    fn emit(&self, event: AssistantEvent) {
        // A host that is gone has nothing left to be told.
        let _ = self.events.send(event);
    }

    fn set_phase(&mut self, phase: Phase) {
        if self.phase != phase {
            self.phase = phase;
            self.emit(AssistantEvent::Phase(phase));
        }
    }

    fn phase_of(&self) -> Phase {
        match &self.state {
            AssistantState::Idle => Phase::Idle,
            AssistantState::Listening { speaking: true, .. } => Phase::UserSpeaking,
            AssistantState::Listening { .. } => Phase::Listening,
            AssistantState::Thinking { .. } => Phase::Thinking,
            AssistantState::Speaking { .. } => Phase::Speaking,
        }
    }

    /// Feed the machine and execute its actions. An illegal event is logged and changes nothing.
    fn apply(&mut self, input: AssistantInput) {
        let state = std::mem::replace(&mut self.state, AssistantState::Idle);
        match state.on(input) {
            Ok((next, actions)) => {
                self.state = next;
                for action in actions {
                    self.execute(action);
                }
                let phase = self.phase_of();
                self.set_phase(phase);
            }
            Err(rejected) => {
                tracing::debug!(%rejected, "the assistant ignored an event that does not fit its state");
                self.state = rejected.from;
            }
        }
    }

    fn execute(&mut self, action: AssistantAction) {
        match action {
            AssistantAction::StartReply(text) => self.start_reply(text),
            AssistantAction::CancelReply => self.cancel_reply(),
            AssistantAction::FlushOutput => {
                self.output.flush();
                let now = self.now();
                self.gate.flushed(now);
            }
            AssistantAction::ShowFault(fault) => self.emit(AssistantEvent::Fault(fault)),
            AssistantAction::EndSession => self.done = true,
        }
    }

    fn start_reply(&mut self, text: String) {
        self.turns += 1;
        let turn = self.turns;
        self.emit(AssistantEvent::UserTurn { text: text.clone() });
        let token = CancellationToken::new();
        let job = ReplyJob {
            turn,
            text,
            lang: self.params.lang,
            conversation: self.params.conversation.clone(),
            voice: self.params.voice.clone(),
            token: token.clone(),
        };
        match reply::spawn(job, &self.tools, &self.reply_tx) {
            Ok(workers) => {
                self.reply = Some(Reply {
                    turn,
                    token,
                    workers: Some(workers),
                    audible: false,
                    audio_complete: false,
                });
            }
            Err(e) => {
                token.cancel();
                self.apply(AssistantInput::ReplyFailed(AssistantFault::Dialogue(
                    prxvc_ports::DialogueError::Internal(format!("cannot start the reply: {e}")),
                )));
            }
        }
    }

    /// Cancel the reply in flight and wait for its workers; the host is told the reply was cut.
    fn cancel_reply(&mut self) {
        let Some(mut reply) = self.reply.take() else {
            return;
        };
        reply.token.cancel();
        // Messages of the cancelled turn still in the queue are stale; the turn check drops them.
        // Workers blocked on a full queue are released by the loop draining it while it waits.
        if let Some(workers) = reply.workers.take() {
            self.drain_while(|| workers_done(&workers), &mut |_| {});
            workers.join();
        }
        self.emit(AssistantEvent::ReplyCut);
    }

    /// Discard reply messages until `done()`; used while workers wind down.
    fn drain_while(&self, done: impl Fn() -> bool, on: &mut dyn FnMut(ReplyMessage)) {
        while !done() {
            match self.reply_rx.recv_timeout(Duration::from_millis(1)) {
                Ok(m) => on(m),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        while let Ok(m) = self.reply_rx.try_recv() {
            on(m);
        }
    }

    fn take_commands(&mut self) -> bool {
        let mut any = false;
        while let Ok(c) = self.commands.try_recv() {
            any = true;
            match c {
                Command::Interrupt => self.apply(AssistantInput::Interrupt),
                Command::Stop => self.apply(AssistantInput::Stop),
            }
        }
        any
    }

    fn take_reply_messages(&mut self) -> bool {
        let mut any = false;
        while let Ok(m) = self.reply_rx.try_recv() {
            any = true;
            self.on_reply(m);
        }
        any
    }

    fn on_reply(&mut self, message: ReplyMessage) {
        let current = self.reply.as_ref().map(|r| r.turn);
        if current != Some(message.turn) {
            return; // a cancelled or finished turn
        }
        match message.kind {
            ReplyKind::Delta(text) => self.emit(AssistantEvent::ReplyDelta { text }),
            ReplyKind::TextDone => {}
            ReplyKind::Audio(chunk) => {
                let now = self.now();
                self.gate.reply_activity(now);
                if let Err(detail) = self.output.play(&chunk) {
                    self.apply(AssistantInput::Fatal(AssistantFault::Output(detail)));
                    return;
                }
                if let Some(reply) = self.reply.as_mut()
                    && !reply.audible
                {
                    reply.audible = true;
                    self.apply(AssistantInput::ReplyAudio);
                }
            }
            ReplyKind::AudioDone => {
                let now = self.now();
                self.output.end_stream();
                self.gate.reply_ended(now);
                if let Some(reply) = self.reply.as_mut() {
                    reply.audio_complete = true;
                }
            }
            ReplyKind::Failed(fault) => self.apply(AssistantInput::ReplyFailed(fault)),
        }
    }

    /// Keep the gate informed and finish the reply once its output has drained.
    fn tick_output(&mut self) {
        let now = self.now();
        let drained = self.output.is_drained();
        self.gate.update(drained, now);
        let finished = self
            .reply
            .as_ref()
            .is_some_and(|r| r.audio_complete && drained);
        if finished {
            if let Some(mut reply) = self.reply.take()
                && let Some(workers) = reply.workers.take()
            {
                workers.join();
            }
            self.emit(AssistantEvent::ReplyDone);
            self.apply(AssistantInput::ReplyDone);
        }
    }

    fn on_frame(&mut self, frame: &AudioFrame) {
        let now = self.now();
        let paused = match self.gate.poll(now) {
            MicState::Closed => true,
            MicState::Open { just_reopened } => {
                if just_reopened {
                    // Whatever was accumulated may hold the tail of the assistant's voice.
                    self.stage.reset();
                    self.segment_text.clear();
                }
                false
            }
        };
        if paused != self.mic_paused {
            self.mic_paused = paused;
            self.emit(AssistantEvent::MicPaused(paused));
        }
        if paused {
            return;
        }
        let mut out = Vec::new();
        if let Err(fault) = self.stage.on_frame(frame, &mut out) {
            self.apply(AssistantInput::Fatal(listening_fault(&fault)));
            return;
        }
        for envelope in out {
            self.on_recognition(envelope.payload);
        }
    }

    fn on_recognition(&mut self, event: RecognitionEvent) {
        match event {
            RecognitionEvent::SpeechStarted => {
                self.segment_text.clear();
                self.apply(AssistantInput::SpeechStarted);
            }
            RecognitionEvent::Hypothesis { text, .. } => {
                let shown = join_words(&self.segment_text, &text);
                self.emit(AssistantEvent::Partial { text: shown });
            }
            RecognitionEvent::Commit(commit) => {
                self.segment_text.push(commit.text);
                let shown = join_words(&self.segment_text, "");
                self.emit(AssistantEvent::Partial { text: shown });
            }
            RecognitionEvent::EndSegment => {
                let text = join_words(&self.segment_text, "");
                self.segment_text.clear();
                if text.trim().is_empty() {
                    self.apply(AssistantInput::NoTurn);
                } else {
                    self.apply(AssistantInput::Turn(text));
                }
            }
        }
    }

    /// The loop is over: nothing of the session stays running.
    fn finish(mut self) {
        if let Some(reply) = self.reply.take() {
            reply.token.cancel();
            if let Some(workers) = reply.workers {
                self.drain_while(|| workers_done(&workers), &mut |_| {});
                workers.join();
            }
        }
        self.output.flush();
        let now = self.now();
        self.gate.flushed(now);
        self.source.stop();
        self.dialogue.end_conversation(&self.params.conversation);
        if self.phase != Phase::Idle {
            self.set_phase(Phase::Idle);
        }
        if self.mic_paused {
            self.emit(AssistantEvent::MicPaused(false));
        }
        self.emit(AssistantEvent::Ended);
    }
}

fn workers_done(workers: &ReplyWorkers) -> bool {
    workers.is_finished()
}

/// The user's words so far: the committed texts and the recogniser's reading of the rest.
fn join_words(committed: &[String], tail: &str) -> String {
    let mut out = String::new();
    for part in committed
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(tail))
    {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let cjk_join =
            out.chars().last().is_some_and(is_cjk) && part.chars().next().is_some_and(is_cjk);
        if !out.is_empty() && !cjk_join {
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{3000}'..='\u{303F}' | '\u{FF00}'..='\u{FFEF}')
}

fn listening_fault(fault: &RecognitionFault) -> AssistantFault {
    AssistantFault::Listening {
        stage: match fault.stage {
            Stage::Vad => Stage::Vad,
            _ => Stage::Asr,
        },
        kind: fault.failure.fault,
        detail: fault.failure.detail.clone(),
    }
}
