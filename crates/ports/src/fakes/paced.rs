//! A scripted stream whose items fall due on an injected clock, used by the MT and TTS fakes.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use prxvc_media::{Clock, MonoTime};
use tokio_util::sync::CancellationToken;

use super::log::{EventKind, EventLog};

/// One scripted step: how long after the previous step it falls due, and what it yields.
#[derive(Clone, Debug)]
pub(crate) struct Step<I, E> {
    pub(crate) delay: Duration,
    pub(crate) outcome: Result<I, E>,
}

/// Follows the stream protocol of the ports: items, then one terminal item or
/// one error, then `None`. Due times accumulate from the moment the request
/// was opened, so the schedule does not depend on how fast the consumer reads.
pub(crate) struct PacedStream<I, E> {
    clock: Arc<dyn Clock>,
    steps: VecDeque<Step<I, E>>,
    due: MonoTime,
    request: String,
    log: EventLog,
    finished: bool,
    is_terminal: fn(&I) -> bool,
    cancelled: fn() -> E,
}

impl<I, E> PacedStream<I, E> {
    pub(crate) fn open(
        clock: Arc<dyn Clock>,
        steps: Vec<Step<I, E>>,
        request: String,
        log: EventLog,
        is_terminal: fn(&I) -> bool,
        cancelled: fn() -> E,
    ) -> Self {
        // The schedule is anchored before `Opened` is logged, so a test that
        // waits for `Opened` and then advances the clock cannot move the anchor.
        let due = clock.now();
        log.record(&request, EventKind::Opened);
        Self {
            due,
            clock,
            steps: steps.into(),
            request,
            log,
            finished: false,
            is_terminal,
            cancelled,
        }
    }

    pub(crate) fn next(&mut self, cancel: &CancellationToken) -> Option<Result<I, E>> {
        if self.finished {
            return None;
        }
        if cancel.is_cancelled() {
            return Some(Err(self.finish_cancelled()));
        }
        let Some(step) = self.steps.pop_front() else {
            // The script ended without a terminal item: end the stream quietly.
            self.finished = true;
            self.log.record(&self.request, EventKind::Completed);
            return None;
        };
        self.due = self.due.saturating_add(step.delay);
        if !self.clock.sleep_until(self.due, &|| cancel.is_cancelled()) {
            return Some(Err(self.finish_cancelled()));
        }
        match step.outcome {
            Err(e) => {
                self.finished = true;
                self.log.record(&self.request, EventKind::Failed);
                Some(Err(e))
            }
            Ok(item) => {
                if (self.is_terminal)(&item) {
                    self.finished = true;
                    self.log.record(&self.request, EventKind::Completed);
                } else {
                    self.log.record(&self.request, EventKind::Item);
                }
                Some(Ok(item))
            }
        }
    }

    fn finish_cancelled(&mut self) -> E {
        self.finished = true;
        self.steps.clear();
        self.log.record(&self.request, EventKind::Cancelled);
        (self.cancelled)()
    }
}

impl<I, E> Drop for PacedStream<I, E> {
    fn drop(&mut self) {
        if !self.finished {
            self.log.record(&self.request, EventKind::Dropped);
        }
    }
}
