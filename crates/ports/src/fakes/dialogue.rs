//! Fake dialogue backend that streams scripted reply tokens on an injected clock.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_media::{Clock, Lang};
use tokio_util::sync::CancellationToken;

use super::log::EventLog;
use super::paced::{PacedStream, Step};
use crate::descriptor::{ProviderDescriptor, StreamingKind};
use crate::dialogue::{
    ConversationId, DialogueBackend, DialogueDelta, DialogueError, DialogueRequest, DialogueStream,
};

/// What the fake backend streams for one user text.
#[derive(Clone, Debug)]
pub struct DialogueScript {
    /// The reply token deltas, in order.
    pub tokens: Vec<String>,
    /// Delay before each token and before `Done`, on the injected clock.
    pub token_delay: Duration,
    /// Token usage reported with `Done`.
    pub usage: (u32, u32),
    /// Fail after this many tokens have been delivered, instead of finishing.
    pub fail_after: Option<(usize, DialogueError)>,
    /// Fail `respond` itself.
    pub open_error: Option<DialogueError>,
}

impl DialogueScript {
    /// A script that streams `tokens`, then `Done`, with no delay.
    pub fn tokens(tokens: impl IntoIterator<Item = &'static str>) -> Self {
        let tokens: Vec<String> = tokens.into_iter().map(str::to_owned).collect();
        let n = u32::try_from(tokens.len()).unwrap_or(u32::MAX);
        Self {
            tokens,
            token_delay: Duration::ZERO,
            usage: (12, n),
            fail_after: None,
            open_error: None,
        }
    }

    /// Wait `delay` on the injected clock before every token.
    #[must_use]
    pub fn with_token_delay(mut self, delay: Duration) -> Self {
        self.token_delay = delay;
        self
    }

    /// Deliver `delivered` tokens, then fail with `error`.
    #[must_use]
    pub fn failing_after(mut self, delivered: usize, error: DialogueError) -> Self {
        self.fail_after = Some((delivered, error));
        self
    }

    /// Make `respond` itself fail.
    #[must_use]
    pub fn failing_open(mut self, error: DialogueError) -> Self {
        self.open_error = Some(error);
        self
    }

    fn steps(&self) -> Vec<Step<DialogueDelta, DialogueError>> {
        let step = |outcome| Step {
            delay: self.token_delay,
            outcome,
        };
        let delivered = self
            .fail_after
            .as_ref()
            .map_or(self.tokens.len(), |(n, _)| (*n).min(self.tokens.len()));
        let mut steps: Vec<_> = self.tokens[..delivered]
            .iter()
            .map(|t| step(Ok(DialogueDelta::Text(t.clone()))))
            .collect();
        match &self.fail_after {
            Some((_, error)) => steps.push(step(Err(error.clone()))),
            None => steps.push(step(Ok(DialogueDelta::Done {
                prompt_tokens: self.usage.0,
                completion_tokens: self.usage.1,
            }))),
        }
        steps
    }
}

struct Shared {
    clock: Arc<dyn Clock>,
    descriptor: Mutex<ProviderDescriptor>,
    by_text: Mutex<HashMap<String, DialogueScript>>,
    default_script: Mutex<Option<DialogueScript>>,
    requests: Mutex<Vec<DialogueRequest>>,
    ended: Mutex<Vec<ConversationId>>,
    log: EventLog,
}

/// A [`DialogueBackend`] that answers from scripts keyed by the text of the last user message.
///
/// Requests are named `turn<N>` (N from 1) in the [`EventLog`]. A text without a script and
/// without a default script is rejected with `Unavailable`: the fake never invents an answer.
#[derive(Clone)]
pub struct FakeDialogue {
    shared: Arc<Shared>,
}

impl FakeDialogue {
    /// A backend with no scripts, timed on `clock`.
    pub fn new(clock: impl Clock + 'static) -> Self {
        let descriptor = ProviderDescriptor::new("fake-dialogue", "fake", StreamingKind::Native)
            .with_languages([Lang::Zh, Lang::En]);
        Self {
            shared: Arc::new(Shared {
                clock: Arc::new(clock),
                descriptor: Mutex::new(descriptor),
                by_text: Mutex::new(HashMap::new()),
                default_script: Mutex::new(None),
                requests: Mutex::new(Vec::new()),
                ended: Mutex::new(Vec::new()),
                log: EventLog::new(),
            }),
        }
    }

    /// Requests whose last user text is `text` play `script`.
    #[must_use]
    pub fn with_script(self, text: &str, script: DialogueScript) -> Self {
        self.shared.by_text.lock().insert(text.to_owned(), script);
        self
    }

    /// Requests without a script of their own play `script`.
    #[must_use]
    pub fn with_default_script(self, script: DialogueScript) -> Self {
        *self.shared.default_script.lock() = Some(script);
        self
    }

    /// The log of every request's life.
    pub fn log(&self) -> EventLog {
        self.shared.log.clone()
    }

    /// Every request accepted so far, in order.
    pub fn requests(&self) -> Vec<DialogueRequest> {
        self.shared.requests.lock().clone()
    }

    /// The conversations that were ended, in order.
    pub fn ended(&self) -> Vec<ConversationId> {
        self.shared.ended.lock().clone()
    }
}

impl DialogueBackend for FakeDialogue {
    fn descriptor(&self) -> ProviderDescriptor {
        self.shared.descriptor.lock().clone()
    }

    fn respond(&self, req: DialogueRequest) -> Result<Box<dyn DialogueStream>, DialogueError> {
        let text = req
            .messages
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default();
        let script = self
            .shared
            .by_text
            .lock()
            .get(&text)
            .cloned()
            .or_else(|| self.shared.default_script.lock().clone())
            .ok_or_else(|| DialogueError::Unavailable {
                reason: format!("no script for `{text}`"),
            })?;
        if let Some(error) = &script.open_error {
            return Err(error.clone());
        }
        let mut requests = self.shared.requests.lock();
        requests.push(req);
        let name = format!("turn{}", requests.len());
        drop(requests);
        let paced = PacedStream::open(
            Arc::clone(&self.shared.clock),
            script.steps(),
            name,
            self.shared.log.clone(),
            |d: &DialogueDelta| matches!(d, DialogueDelta::Done { .. }),
            || DialogueError::Cancelled,
        );
        Ok(Box::new(FakeDialogueStream { paced }))
    }

    fn end_conversation(&self, conversation: &ConversationId) {
        self.shared.ended.lock().push(conversation.clone());
    }
}

struct FakeDialogueStream {
    paced: PacedStream<DialogueDelta, DialogueError>,
}

impl DialogueStream for FakeDialogueStream {
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<DialogueDelta, DialogueError>> {
        self.paced.next(cancel)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use prxvc_media::ManualClock;

    use super::*;
    use crate::dialogue::DialogueMessage;
    use crate::fakes::log::EventKind;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn request(text: &str) -> DialogueRequest {
        DialogueRequest {
            conversation: ConversationId::new("c1"),
            messages: vec![DialogueMessage::user(text)],
            lang: Lang::En,
        }
    }

    fn collect(
        stream: &mut dyn DialogueStream,
        cancel: &CancellationToken,
    ) -> Vec<Result<DialogueDelta, DialogueError>> {
        std::iter::from_fn(|| stream.next(cancel)).collect()
    }

    #[test]
    fn a_script_streams_tokens_then_done_and_the_stream_ends() {
        let backend = FakeDialogue::new(ManualClock::new())
            .with_script("hi", DialogueScript::tokens(["Hello", " there."]));
        let mut stream = backend.respond(request("hi")).expect("opens");
        let items = collect(stream.as_mut(), &CancellationToken::new());
        assert_eq!(
            items,
            vec![
                Ok(DialogueDelta::Text("Hello".into())),
                Ok(DialogueDelta::Text(" there.".into())),
                Ok(DialogueDelta::Done {
                    prompt_tokens: 12,
                    completion_tokens: 2
                }),
            ]
        );
        assert!(stream.next(&CancellationToken::new()).is_none());
        assert_eq!(
            backend.log().kinds_for("turn1").last(),
            Some(&EventKind::Completed)
        );
    }

    #[test]
    fn a_failure_after_some_tokens_ends_the_stream_with_that_error() {
        let backend = FakeDialogue::new(ManualClock::new()).with_default_script(
            DialogueScript::tokens(["a", "b", "c"]).failing_after(
                1,
                DialogueError::Unavailable {
                    reason: "gone".into(),
                },
            ),
        );
        let mut stream = backend.respond(request("x")).expect("opens");
        let items = collect(stream.as_mut(), &CancellationToken::new());
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], Ok(DialogueDelta::Text("a".into())));
        assert!(matches!(items[1], Err(DialogueError::Unavailable { .. })));
    }

    #[test]
    fn an_unscripted_text_is_refused_not_invented() {
        let backend = FakeDialogue::new(ManualClock::new());
        assert!(matches!(
            backend.respond(request("unknown")),
            Err(DialogueError::Unavailable { .. })
        ));
        let scripted = FakeDialogue::new(ManualClock::new()).with_default_script(
            DialogueScript::tokens(["x"]).failing_open(DialogueError::Rejected { status: 400 }),
        );
        assert!(matches!(
            scripted.respond(request("a")),
            Err(DialogueError::Rejected { status: 400 })
        ));
    }

    #[test]
    fn cancelling_a_waiting_stream_ends_it_with_cancelled_and_leaves_others_alone() {
        let clock = ManualClock::new();
        let backend = FakeDialogue::new(clock.clone())
            .with_default_script(DialogueScript::tokens(["a", "b"]).with_token_delay(MS(100)));
        let log = backend.log();
        let mut first = backend.respond(request("one")).expect("opens");
        let mut second = backend.respond(request("two")).expect("opens");
        let token = CancellationToken::new();
        let waiter = {
            let token = token.clone();
            thread::spawn(move || first.next(&token))
        };
        log.wait_for(EventKind::Opened, 2);
        token.cancel();
        assert_eq!(
            waiter.join().expect("thread"),
            Some(Err(DialogueError::Cancelled))
        );
        clock.advance(MS(100));
        let other = CancellationToken::new();
        assert_eq!(
            second.next(&other),
            Some(Ok(DialogueDelta::Text("a".into())))
        );
    }

    #[test]
    fn requests_and_ended_conversations_are_recorded() {
        let backend = FakeDialogue::new(ManualClock::new())
            .with_default_script(DialogueScript::tokens(["ok"]));
        let _ = backend.respond(request("hello")).expect("opens");
        assert_eq!(backend.requests().len(), 1);
        assert_eq!(backend.requests()[0].messages[0].text, "hello");
        backend.end_conversation(&ConversationId::new("c1"));
        assert_eq!(backend.ended(), vec![ConversationId::new("c1")]);
    }
}
