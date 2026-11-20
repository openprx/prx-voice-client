//! The audio input of a direction: the capture queue (Q1) with its drop-oldest policy and the discontinuity mark.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use prxvc_media::AudioFrame;

use super::queue::{BoundedQueue, Pushed};
use super::snapshot::Stats;

/// What happened to a pushed frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushOutcome {
    /// Queued.
    Accepted,
    /// Queued after this many older frames were dropped because the recogniser
    /// cannot keep up; the next frame is marked as a discontinuity.
    DroppedOldest(usize),
    /// The pipeline stopped.
    Closed,
}

/// The write end of the capture queue; clones feed the same queue.
///
/// Frames must be at the VAD engine's sample rate: resampling is the endpoint
/// worker's job (pipeline-design 1.2). The queue holds `pipeline` capacity
/// worth of audio (2 s by default) and drops the oldest frames when it is
/// full, never blocking the caller.
#[derive(Clone)]
pub struct AudioInput {
    queue: Arc<BoundedQueue<AudioFrame>>,
    discontinuity: Arc<AtomicBool>,
    stats: Arc<Stats>,
}

impl AudioInput {
    pub(crate) fn new(queue: Arc<BoundedQueue<AudioFrame>>, stats: Arc<Stats>) -> Self {
        Self {
            queue,
            discontinuity: Arc::new(AtomicBool::new(false)),
            stats,
        }
    }

    /// Queue a frame without blocking.
    pub fn push(&self, frame: AudioFrame) -> PushOutcome {
        let frame = if self.discontinuity.swap(false, Ordering::SeqCst) {
            frame.with_discontinuity(true)
        } else {
            frame
        };
        match self.queue.push(frame, None) {
            Pushed::Accepted => PushOutcome::Accepted,
            Pushed::Evicted(old) => {
                Stats::add(&self.stats.capture_overruns, old.len() as u64);
                self.discontinuity.store(true, Ordering::SeqCst);
                PushOutcome::DroppedOldest(old.len())
            }
            Pushed::Rejected(_) | Pushed::Closed(_) | Pushed::Cancelled(_) => PushOutcome::Closed,
        }
    }

    /// Frames waiting for the recogniser.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }
}
