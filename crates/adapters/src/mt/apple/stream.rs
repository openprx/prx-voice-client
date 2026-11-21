//! The stream of one system-translation request: a worker thread makes the blocking call, the
//! stream waits for it and watches the caller's cancellation token.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use prxvc_ports::{CancellationToken, MtDelta, MtError, MtFlag, MtRequest, MtStream};

use super::super::assess::{AssessInput, assess};
use super::super::report::{MtFailure, Suspect};
use super::fault::{SystemFault, SystemFaultKind};
use super::translator::Shared;

/// How often a waiting stream looks at its cancellation token. Results do not wait for it.
const CANCEL_POLL: Duration = Duration::from_millis(10);

type Answer = Result<String, SystemFault>;

/// Start the blocking call of one request on its own thread; returns its id and where the answer
/// arrives.
pub(super) fn spawn_request(
    shared: &Arc<Shared>,
    text: &str,
    cancelled: Arc<AtomicBool>,
) -> (u64, Receiver<Answer>) {
    let (tx, rx) = channel();
    let shared = Arc::clone(shared);
    let text = text.to_owned();
    let id = shared.next_id();
    let spawned = std::thread::Builder::new()
        .name("system-translation".into())
        .spawn({
            let tx = tx.clone();
            move || {
                let _gate = shared.gate.lock();
                if cancelled.load(Ordering::Acquire) {
                    let _ = tx.send(Err(SystemFault::new(SystemFaultKind::Cancelled, "")));
                    return;
                }
                shared.active.store(id, Ordering::Release);
                let answer = shared.session.translate(&text, shared.translate_timeout);
                shared.active.store(0, Ordering::Release);
                // The stream may be gone (cancelled or dropped): nobody waits then.
                let _ = tx.send(answer);
            }
        });
    if let Err(e) = spawned {
        let _ = tx.send(Err(SystemFault::new(
            SystemFaultKind::Framework,
            format!("cannot start the request thread: {e}"),
        )));
    }
    (id, rx)
}

enum State {
    Waiting,
    /// The translation was delivered; `Done` follows.
    DonePending,
    Ended,
}

/// The stream of one request: its own handle.
pub(super) struct SystemStream {
    shared: Arc<Shared>,
    req: MtRequest,
    rx: Receiver<Answer>,
    cancelled: Arc<AtomicBool>,
    /// The id the request thread registers while its call is inside the session.
    id: u64,
    state: State,
    flags: Vec<MtFlag>,
}

impl SystemStream {
    pub(super) fn new(
        shared: Arc<Shared>,
        req: MtRequest,
        id: u64,
        rx: Receiver<Answer>,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            shared,
            req,
            id,
            rx,
            cancelled,
            state: State::Waiting,
            flags: Vec::new(),
        }
    }

    /// Abandon the request: a call that has not started never will, one in the session is
    /// interrupted. Interrupting is done only while this request's call is the active one.
    fn abandon(&mut self) {
        self.state = State::Ended;
        self.cancelled.store(true, Ordering::Release);
        if self.shared.active.load(Ordering::Acquire) == self.id {
            self.shared.session.cancel();
        }
    }

    fn finish(&mut self, text: String) -> Result<MtDelta, MtError> {
        let text = text.trim().to_owned();
        // The checks that make sense for a translation service: an empty result and the wrong
        // language. The model-specific checks (answer-like, thinking text, repetition, refusal,
        // token limits) do not apply to it.
        let suspects = assess(&AssessInput {
            source: &self.req.text,
            source_lang: self.req.source,
            target_lang: self.req.target,
            output: &text,
            finish_reason: None,
        });
        if suspects.contains(&Suspect::Empty) {
            self.state = State::Ended;
            return Err(MtFailure::EmptyOutput.to_port_error());
        }
        if suspects.contains(&Suspect::WrongLanguage) {
            self.flags.push(MtFlag::WrongLanguage);
        }
        self.state = State::DonePending;
        Ok(MtDelta::Text(text))
    }
}

impl MtStream for SystemStream {
    fn flags(&self) -> Vec<MtFlag> {
        self.flags.clone()
    }

    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<MtDelta, MtError>> {
        match self.state {
            State::Ended => return None,
            State::DonePending => {
                self.state = State::Ended;
                return Some(Ok(MtDelta::Done {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                }));
            }
            State::Waiting => {}
        }
        loop {
            if cancel.is_cancelled() {
                self.abandon();
                return Some(Err(MtError::Cancelled));
            }
            match self.rx.recv_timeout(CANCEL_POLL) {
                Ok(Ok(text)) => return Some(self.finish(text)),
                Ok(Err(fault)) => {
                    self.state = State::Ended;
                    return Some(Err(fault.to_port_error()));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.state = State::Ended;
                    return Some(Err(MtError::Internal(
                        "the translation thread ended without an answer".into(),
                    )));
                }
            }
        }
    }
}

impl Drop for SystemStream {
    /// Dropping a stream abandons its request.
    fn drop(&mut self) {
        if !matches!(self.state, State::Ended | State::DonePending) {
            self.abandon();
        }
    }
}
