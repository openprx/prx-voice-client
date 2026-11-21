//! The two workers of one reply: the dialogue stream cut into clauses, and the clauses synthesised in order.
//!
//! Everything streams: the first clause goes to synthesis as soon as the splitter closes it, while
//! the rest of the reply is still arriving, so the first audio never waits for the whole text. Each
//! worker holds the turn's cancellation token and ends promptly when it fires (which also abandons
//! the dialogue request and the synthesis request in flight).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;

use prxvc_media::{
    ClauseId, ClauseSplitter, Clock, Direction, Lang, PcmChunk, SegmentId, has_speakable_content,
};
use prxvc_ports::{
    CancellationToken, ConversationId, DialogueBackend, DialogueDelta, DialogueMessage,
    DialogueRequest, SpeechSynth, TraceSink, TraceStage, TtsItem, TtsRequest, VoiceSelection,
};

use super::fault::AssistantFault;
use crate::pipeline::Tracer;

/// Clauses waiting for synthesis; a full queue holds the dialogue worker back (backpressure).
const CLAUSE_QUEUE: usize = 8;

/// What a reply's workers report to the loop, tagged with the turn.
#[derive(Debug)]
pub(crate) struct ReplyMessage {
    pub(crate) turn: u64,
    pub(crate) kind: ReplyKind,
}

#[derive(Debug)]
pub(crate) enum ReplyKind {
    /// More reply text.
    Delta(String),
    /// The reply text is complete.
    TextDone,
    /// A chunk of the reply's audio.
    Audio(PcmChunk),
    /// All audio of the reply was produced.
    AudioDone,
    /// The reply failed.
    Failed(AssistantFault),
}

/// What a reply needs.
pub(crate) struct ReplyJob {
    pub(crate) turn: u64,
    pub(crate) text: String,
    pub(crate) lang: Lang,
    pub(crate) conversation: ConversationId,
    pub(crate) voice: VoiceSelection,
    pub(crate) token: CancellationToken,
}

/// The providers and instruments the workers use.
#[derive(Clone)]
pub(crate) struct ReplyTools {
    pub(crate) dialogue: Arc<dyn DialogueBackend>,
    pub(crate) tts: Arc<dyn SpeechSynth>,
    pub(crate) trace: Arc<dyn TraceSink>,
    pub(crate) clock: Arc<dyn Clock>,
}

/// The running workers of one reply.
pub(crate) struct ReplyWorkers {
    threads: Vec<JoinHandle<()>>,
}

impl ReplyWorkers {
    /// Whether both workers have ended.
    pub(crate) fn is_finished(&self) -> bool {
        self.threads.iter().all(JoinHandle::is_finished)
    }

    /// Wait for both workers to end. They end on their own once the reply is complete or the
    /// job's token was cancelled.
    pub(crate) fn join(self) {
        for t in self.threads {
            let _ = t.join();
        }
    }
}

/// Start the workers of `job`; their messages go to `tx`.
pub(crate) fn spawn(
    job: ReplyJob,
    tools: &ReplyTools,
    tx: &SyncSender<ReplyMessage>,
) -> Result<ReplyWorkers, std::io::Error> {
    let (clause_tx, clause_rx) = sync_channel::<String>(CLAUSE_QUEUE);
    let job = Arc::new(job);
    let tracer = Tracer::new(Arc::clone(&tools.trace), Arc::clone(&tools.clock));
    let text_thread = {
        let (job, tools, tx, tracer) =
            (Arc::clone(&job), tools.clone(), tx.clone(), tracer.clone());
        std::thread::Builder::new()
            .name("prxvc-assistant-dialogue".into())
            .spawn(move || run_dialogue(&job, &tools, &tx, &clause_tx, &tracer))?
    };
    let synth_thread = {
        let (job, tools, tx) = (Arc::clone(&job), tools.clone(), tx.clone());
        std::thread::Builder::new()
            .name("prxvc-assistant-synth".into())
            .spawn(move || run_synthesis(&job, &tools, &tx, &clause_rx, &tracer))?
    };
    Ok(ReplyWorkers {
        threads: vec![text_thread, synth_thread],
    })
}

/// The clause id the trace and the synthesiser use for clause `index` of turn `turn`.
fn clause_id(turn: u64, index: u16) -> ClauseId {
    let segment = SegmentId::new(Direction::Outgoing, turn.max(1))
        .unwrap_or_else(|| SegmentId::first(Direction::Outgoing));
    ClauseId::new(segment, index)
}

/// Send `kind`; false when the loop is gone or the turn was cancelled meanwhile.
fn send(tx: &SyncSender<ReplyMessage>, job: &ReplyJob, kind: ReplyKind) -> bool {
    !job.token.is_cancelled()
        && tx
            .send(ReplyMessage {
                turn: job.turn,
                kind,
            })
            .is_ok()
}

fn run_dialogue(
    job: &ReplyJob,
    tools: &ReplyTools,
    tx: &SyncSender<ReplyMessage>,
    clauses: &SyncSender<String>,
    tracer: &Tracer,
) {
    let first = clause_id(job.turn, 0);
    let request = DialogueRequest {
        conversation: job.conversation.clone(),
        messages: vec![DialogueMessage::user(job.text.clone())],
        lang: job.lang,
    };
    tracer.mark(first, TraceStage::MtRequestSent);
    let mut stream = match tools.dialogue.respond(request) {
        Ok(s) => s,
        Err(e) => {
            if !e.is_cancelled() {
                send(tx, job, ReplyKind::Failed(AssistantFault::Dialogue(e)));
            }
            return;
        }
    };
    let mut splitter = ClauseSplitter::new(job.lang);
    let mut first_token = true;
    loop {
        match stream.next(&job.token) {
            Some(Ok(DialogueDelta::Text(delta))) => {
                if first_token {
                    first_token = false;
                    tracer.mark(first, TraceStage::MtFirstToken);
                }
                if !send(tx, job, ReplyKind::Delta(delta.clone())) {
                    return;
                }
                for clause in splitter.push(&delta) {
                    if clauses.send(clause).is_err() {
                        return;
                    }
                }
            }
            Some(Ok(DialogueDelta::Done { .. })) => {
                for clause in splitter.finish() {
                    if clauses.send(clause).is_err() {
                        return;
                    }
                }
                tracer.mark(first, TraceStage::MtDone);
                send(tx, job, ReplyKind::TextDone);
                return;
            }
            Some(Err(e)) => {
                if !e.is_cancelled() && !job.token.is_cancelled() {
                    send(tx, job, ReplyKind::Failed(AssistantFault::Dialogue(e)));
                }
                return;
            }
            None => return,
        }
    }
}

fn run_synthesis(
    job: &ReplyJob,
    tools: &ReplyTools,
    tx: &SyncSender<ReplyMessage>,
    clauses: &Receiver<String>,
    tracer: &Tracer,
) {
    let mut produced_audio = false;
    let mut index: u16 = 0;
    // Ends when the dialogue worker has dropped its sender and the queue is empty.
    while let Ok(text) = clauses.recv() {
        if job.token.is_cancelled() {
            return;
        }
        if !has_speakable_content(&text) {
            continue;
        }
        let clause = clause_id(job.turn, index);
        index = index.saturating_add(1);
        tracer.mark(clause, TraceStage::TtsRequestSent);
        let request = TtsRequest {
            clause,
            text,
            lang: job.lang,
            voice: job.voice.clone(),
        };
        let mut stream = match tools.tts.synthesize(request) {
            Ok(s) => s,
            Err(e) => {
                if !e.is_cancelled() {
                    send(tx, job, ReplyKind::Failed(AssistantFault::Synthesis(e)));
                }
                return;
            }
        };
        let mut first_chunk = true;
        loop {
            match stream.next(&job.token) {
                Some(Ok(TtsItem::Chunk(chunk))) => {
                    if first_chunk {
                        first_chunk = false;
                        tracer.mark(clause, TraceStage::TtsFirstChunk);
                    }
                    produced_audio = true;
                    if !send(tx, job, ReplyKind::Audio(chunk)) {
                        return;
                    }
                }
                Some(Ok(TtsItem::Done { .. })) => {
                    tracer.mark(clause, TraceStage::TtsDone);
                    break;
                }
                Some(Err(e)) => {
                    if !e.is_cancelled() && !job.token.is_cancelled() {
                        send(tx, job, ReplyKind::Failed(AssistantFault::Synthesis(e)));
                    }
                    return;
                }
                None => break,
            }
        }
    }
    if job.token.is_cancelled() {
        return;
    }
    if produced_audio {
        send(tx, job, ReplyKind::AudioDone);
    } else {
        send(tx, job, ReplyKind::Failed(AssistantFault::NoAudio));
    }
}
