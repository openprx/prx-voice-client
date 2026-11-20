//! The way a stage thread reports to the control thread: envelopes pushed into the inbox under the queue's policy.

use std::sync::Arc;

use prxvc_media::{ClauseId, Envelope, MonoTime};
use prxvc_ports::CancellationToken;

use super::messages::{Inbound, RecognitionEvent, StageEvent, WorkerEvent};
use super::queue::{BoundedQueue, Pushed};

/// What the envelope of a stage event copies from the job it answers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Meta {
    pub(crate) revision: u32,
    pub(crate) source_ts: MonoTime,
}

/// The sending end of the control thread's inbox (Q2).
#[derive(Clone)]
pub(crate) struct Outbox {
    inbox: Arc<BoundedQueue<Inbound>>,
}

impl Outbox {
    pub(crate) fn new(inbox: Arc<BoundedQueue<Inbound>>) -> Self {
        Self { inbox }
    }

    /// Report a clause-level event. Blocks while the inbox is full (stages
    /// wait for the control thread, CS-4.3.2) and gives up when `token` is
    /// cancelled or the inbox closes; returns whether the event was queued.
    pub(crate) fn stage(
        &self,
        clause: ClauseId,
        meta: Meta,
        token: &CancellationToken,
        event: StageEvent,
    ) -> bool {
        let envelope = Envelope::new(clause.segment(), meta.revision, meta.source_ts, event);
        matches!(
            self.inbox.push(Inbound::Stage(envelope), Some(token)),
            Pushed::Accepted | Pushed::Evicted(_)
        )
    }

    /// Report an event that belongs to no clause.
    pub(crate) fn worker(&self, event: WorkerEvent) -> bool {
        matches!(
            self.inbox.push(Inbound::Worker(event), None),
            Pushed::Accepted | Pushed::Evicted(_)
        )
    }

    /// Report a segment-level event of the recogniser; hypotheses of a
    /// segment replace their predecessor still waiting in the inbox.
    pub(crate) fn recognition(
        &self,
        envelope: Envelope<RecognitionEvent>,
        token: &CancellationToken,
    ) -> bool {
        let pushed = if matches!(envelope.payload, RecognitionEvent::Hypothesis { .. }) {
            let segment = envelope.segment;
            self.inbox.push_replacing(Inbound::Recognition(envelope), Some(token), |queued| {
                matches!(
                    queued,
                    Inbound::Recognition(e)
                        if e.segment == segment && matches!(e.payload, RecognitionEvent::Hypothesis { .. })
                )
            })
        } else {
            self.inbox.push(Inbound::Recognition(envelope), Some(token))
        };
        matches!(pushed, Pushed::Accepted | Pushed::Evicted(_))
    }
}
