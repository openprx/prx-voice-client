//! Fake translator that streams scripted tokens on an injected clock.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_media::{Clock, Lang};
use tokio_util::sync::CancellationToken;

use super::log::EventLog;
use super::paced::{PacedStream, Step};
use crate::descriptor::{ProviderDescriptor, StreamingKind};
use crate::mt::{MtDelta, MtError, MtRequest, MtStream, Translator};

/// What the fake translator streams for one source text.
#[derive(Clone, Debug)]
pub struct MtScript {
    /// The token deltas, in order.
    pub tokens: Vec<String>,
    /// Delay before each token and before `Done`, on the injected clock.
    pub token_delay: Duration,
    /// Token usage reported with `Done`.
    pub usage: (u32, u32),
    /// Fail after this many tokens have been delivered, instead of finishing.
    pub fail_after: Option<(usize, MtError)>,
    /// Fail `translate` itself.
    pub open_error: Option<MtError>,
}

impl MtScript {
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
    pub fn failing_after(mut self, delivered: usize, error: MtError) -> Self {
        self.fail_after = Some((delivered, error));
        self
    }

    /// Make `translate` itself fail.
    #[must_use]
    pub fn failing_open(mut self, error: MtError) -> Self {
        self.open_error = Some(error);
        self
    }

    fn steps(&self) -> Vec<Step<MtDelta, MtError>> {
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
            .map(|t| step(Ok(MtDelta::Text(t.clone()))))
            .collect();
        match &self.fail_after {
            Some((_, error)) => steps.push(step(Err(error.clone()))),
            None => steps.push(step(Ok(MtDelta::Done {
                prompt_tokens: self.usage.0,
                completion_tokens: self.usage.1,
            }))),
        }
        steps
    }
}

struct MtShared {
    clock: Arc<dyn Clock>,
    descriptor: Mutex<ProviderDescriptor>,
    by_text: Mutex<HashMap<String, MtScript>>,
    default_script: Mutex<Option<MtScript>>,
    requests: Mutex<Vec<MtRequest>>,
    log: EventLog,
}

/// A [`Translator`] that answers from scripts keyed by the source text.
///
/// Requests are named by their clause id (`out#1.0`) in the [`EventLog`]. A
/// text without a script and without a default script is rejected with
/// `Unavailable`: the fake never invents a translation.
#[derive(Clone)]
pub struct FakeTranslator {
    shared: Arc<MtShared>,
}

impl FakeTranslator {
    /// A translator with no scripts, timed on `clock`.
    pub fn new(clock: impl Clock + 'static) -> Self {
        let descriptor = ProviderDescriptor::new("fake-mt", "fake", StreamingKind::Native)
            .with_languages([Lang::Zh, Lang::En]);
        Self {
            shared: Arc::new(MtShared {
                clock: Arc::new(clock),
                descriptor: Mutex::new(descriptor),
                by_text: Mutex::new(HashMap::new()),
                default_script: Mutex::new(None),
                requests: Mutex::new(Vec::new()),
                log: EventLog::new(),
            }),
        }
    }

    /// Requests whose text is `text` play `script`.
    #[must_use]
    pub fn with_script(self, text: &str, script: MtScript) -> Self {
        self.shared.by_text.lock().insert(text.to_owned(), script);
        self
    }

    /// Requests without a script of their own play `script`.
    #[must_use]
    pub fn with_default_script(self, script: MtScript) -> Self {
        *self.shared.default_script.lock() = Some(script);
        self
    }

    /// Replace the descriptor, for example to make the provider non-streaming.
    #[must_use]
    pub fn with_descriptor(self, descriptor: ProviderDescriptor) -> Self {
        *self.shared.descriptor.lock() = descriptor;
        self
    }

    /// The log of every request's life.
    pub fn log(&self) -> EventLog {
        self.shared.log.clone()
    }

    /// Every request accepted so far, in order.
    pub fn requests(&self) -> Vec<MtRequest> {
        self.shared.requests.lock().clone()
    }
}

impl Translator for FakeTranslator {
    fn descriptor(&self) -> ProviderDescriptor {
        self.shared.descriptor.lock().clone()
    }

    fn translate(&self, req: MtRequest) -> Result<Box<dyn MtStream>, MtError> {
        let script = self
            .shared
            .by_text
            .lock()
            .get(&req.text)
            .cloned()
            .or_else(|| self.shared.default_script.lock().clone())
            .ok_or_else(|| MtError::Unavailable {
                reason: format!("no script for `{}`", req.text),
            })?;
        if let Some(error) = &script.open_error {
            return Err(error.clone());
        }
        let request = req.clause.to_string();
        self.shared.requests.lock().push(req);
        let paced = PacedStream::open(
            Arc::clone(&self.shared.clock),
            script.steps(),
            request,
            self.shared.log.clone(),
            |d: &MtDelta| matches!(d, MtDelta::Done { .. }),
            || MtError::Cancelled,
        );
        Ok(Box::new(FakeMtStream(paced)))
    }
}

struct FakeMtStream(PacedStream<MtDelta, MtError>);

impl MtStream for FakeMtStream {
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<MtDelta, MtError>> {
        self.0.next(cancel)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use prxvc_media::{ClauseId, Direction, ManualClock, SegmentId};

    use super::*;
    use crate::fakes::log::EventKind;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn clause(direction: Direction, index: u16) -> ClauseId {
        ClauseId::new(SegmentId::first(direction), index)
    }

    fn request(direction: Direction, text: &str) -> MtRequest {
        MtRequest {
            clause: clause(direction, 0),
            source: Lang::Zh,
            target: Lang::En,
            text: text.to_owned(),
            context: vec![("before".to_owned(), "earlier".to_owned())],
            glossary: Vec::new(),
        }
    }

    fn collect(
        stream: &mut dyn MtStream,
        cancel: &CancellationToken,
    ) -> Vec<Result<MtDelta, MtError>> {
        std::iter::from_fn(|| stream.next(cancel)).collect()
    }

    #[test]
    fn tokens_stream_in_order_and_end_with_done_then_none() {
        let mt = FakeTranslator::new(ManualClock::new())
            .with_script("hi", MtScript::tokens(["Hel", "lo"]));
        let cancel = CancellationToken::new();
        let mut s = mt
            .translate(request(Direction::Outgoing, "hi"))
            .expect("stream");
        assert_eq!(
            collect(s.as_mut(), &cancel),
            vec![
                Ok(MtDelta::Text("Hel".to_owned())),
                Ok(MtDelta::Text("lo".to_owned())),
                Ok(MtDelta::Done {
                    prompt_tokens: 12,
                    completion_tokens: 2
                }),
            ]
        );
        assert!(
            s.next(&cancel).is_none(),
            "a finished stream stays finished"
        );
        assert_eq!(
            mt.log().kinds_for("out#1.0"),
            vec![
                EventKind::Opened,
                EventKind::Item,
                EventKind::Item,
                EventKind::Completed
            ]
        );
        assert_eq!(
            mt.requests()[0].context.len(),
            1,
            "the request is recorded as received"
        );
    }

    #[test]
    fn token_delays_are_driven_by_the_injected_clock() {
        let clock = ManualClock::new();
        let mt = FakeTranslator::new(clock.clone())
            .with_script("hi", MtScript::tokens(["a"]).with_token_delay(MS(100)));
        let mut s = mt
            .translate(request(Direction::Outgoing, "hi"))
            .expect("stream");
        let reader = thread::spawn(move || {
            let cancel = CancellationToken::new();
            let first = s.next(&cancel);
            (first, s)
        });
        clock.advance(MS(99));
        assert!(
            !reader.is_finished(),
            "nothing is delivered before the delay has passed"
        );
        clock.advance(MS(1));
        let (first, _stream) = reader.join().expect("reader");
        assert_eq!(first, Some(Ok(MtDelta::Text("a".to_owned()))));
    }

    #[test]
    fn cancelling_mid_stream_stops_that_request_and_a_concurrent_one_continues() {
        let clock = ManualClock::new();
        let script = MtScript::tokens(["one", "two", "three"]).with_token_delay(MS(10));
        let mt = FakeTranslator::new(clock.clone()).with_default_script(script);
        let mut outgoing = mt
            .translate(request(Direction::Outgoing, "x"))
            .expect("outgoing");
        let mut incoming = mt
            .translate(request(Direction::Incoming, "y"))
            .expect("incoming");
        let (cancel_out, cancel_in) = (CancellationToken::new(), CancellationToken::new());

        let out_thread = {
            let token = cancel_out.clone();
            thread::spawn(move || {
                let first = outgoing.next(&token);
                let second = outgoing.next(&token);
                let after = outgoing.next(&token);
                (first, second, after)
            })
        };
        let in_thread = {
            let token = cancel_in.clone();
            thread::spawn(move || collect(incoming.as_mut(), &token))
        };

        clock.advance(MS(10));
        // The outgoing worker now blocks on its second token; cancel it there.
        while mt
            .log()
            .kinds_for("out#1.0")
            .iter()
            .filter(|k| **k == EventKind::Item)
            .count()
            < 1
        {
            thread::yield_now();
        }
        cancel_out.cancel();
        let (first, second, after) = out_thread.join().expect("outgoing worker");
        assert_eq!(first, Some(Ok(MtDelta::Text("one".to_owned()))));
        assert_eq!(second, Some(Err(MtError::Cancelled)));
        assert_eq!(after, None, "no output after cancellation");

        clock.advance(MS(30));
        let incoming_items = in_thread.join().expect("incoming worker");
        assert_eq!(
            incoming_items.len(),
            4,
            "the other request still delivers everything"
        );
        assert!(matches!(
            incoming_items.last(),
            Some(Ok(MtDelta::Done { .. }))
        ));
        assert_eq!(
            mt.log().kinds_for("out#1.0").last(),
            Some(&EventKind::Cancelled)
        );
        assert_eq!(
            mt.log().kinds_for("in#1.0").last(),
            Some(&EventKind::Completed)
        );
    }

    #[test]
    fn a_failure_after_n_tokens_is_delivered_once_then_the_stream_ends() {
        let script =
            MtScript::tokens(["a", "b", "c"]).failing_after(1, MtError::Rejected { status: 503 });
        let mt = FakeTranslator::new(ManualClock::new()).with_default_script(script);
        let mut s = mt
            .translate(request(Direction::Incoming, "x"))
            .expect("stream");
        let items = collect(s.as_mut(), &CancellationToken::new());
        assert_eq!(
            items,
            vec![
                Ok(MtDelta::Text("a".to_owned())),
                Err(MtError::Rejected { status: 503 })
            ]
        );
        assert_eq!(
            mt.log().kinds_for("in#1.0").last(),
            Some(&EventKind::Failed)
        );
    }

    #[test]
    fn open_failures_and_missing_scripts_are_typed_errors_not_made_up_output() {
        let mt = FakeTranslator::new(ManualClock::new())
            .with_script("down", MtScript::tokens([]).failing_open(MtError::Timeout));
        assert!(matches!(
            mt.translate(request(Direction::Outgoing, "down")).err(),
            Some(MtError::Timeout)
        ));
        assert!(matches!(
            mt.translate(request(Direction::Outgoing, "unscripted"))
                .err(),
            Some(MtError::Unavailable { .. })
        ));
    }

    #[test]
    fn dropping_a_stream_is_visible_as_an_abandoned_request() {
        let mt =
            FakeTranslator::new(ManualClock::new()).with_default_script(MtScript::tokens(["a"]));
        drop(
            mt.translate(request(Direction::Outgoing, "x"))
                .expect("stream"),
        );
        assert_eq!(
            mt.log().kinds_for("out#1.0"),
            vec![EventKind::Opened, EventKind::Dropped]
        );
    }

    #[test]
    fn capabilities_refuse_a_non_streaming_provider_and_an_unsupported_pair() {
        let mt = FakeTranslator::new(ManualClock::new());
        assert!(mt.descriptor().require_native().is_ok());
        let zh_only = mt.descriptor().with_languages([Lang::Zh]);
        assert!(zh_only.require_pair(Lang::Zh, Lang::En).is_err());
        let batch = ProviderDescriptor::new("batch-mt", "m", StreamingKind::None);
        assert!(
            mt.with_descriptor(batch)
                .descriptor()
                .require_live()
                .is_err()
        );
    }
}
