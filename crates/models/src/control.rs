//! Shared flags between a running job and its owner: stop requests and
//! progress counters. Single responsibility: cheap cross-thread signalling.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// Keep the partial; the job can be resumed.
    Pause,
    /// Delete the partial.
    Cancel,
}

/// Stop request for one job, polled by the engine between reads.
#[derive(Debug, Default)]
pub struct JobControl {
    stop: AtomicU8,
}

impl JobControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request(&self, stop: Stop) {
        let v = match stop {
            Stop::Pause => 1,
            Stop::Cancel => 2,
        };
        // A cancel is never downgraded to a pause.
        self.stop.fetch_max(v, Ordering::SeqCst);
    }

    pub fn signal(&self) -> Option<Stop> {
        match self.stop.load(Ordering::SeqCst) {
            1 => Some(Stop::Pause),
            2 => Some(Stop::Cancel),
            _ => None,
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.signal().is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Downloading,
    Verifying,
}

/// Progress counters of one job, written by the worker, read by the owner.
#[derive(Debug, Default)]
pub struct JobProgress {
    done: AtomicU64,
    total: AtomicU64,
    phase: AtomicU8,
    verify_done: AtomicU64,
    verify_total: AtomicU64,
}

impl JobProgress {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }
    pub fn set_done(&self, done: u64) {
        self.done.store(done, Ordering::Relaxed);
    }
    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    pub fn start_verify(&self, total: u64) {
        self.verify_done.store(0, Ordering::Relaxed);
        self.verify_total.store(total, Ordering::Relaxed);
        self.phase.store(1, Ordering::Relaxed);
    }
    pub fn set_verify_done(&self, done: u64) {
        self.verify_done.store(done, Ordering::Relaxed);
    }
    pub fn end_verify(&self) {
        self.phase.store(0, Ordering::Relaxed);
    }
    pub fn verify(&self) -> (u64, u64) {
        (
            self.verify_done.load(Ordering::Relaxed),
            self.verify_total.load(Ordering::Relaxed),
        )
    }

    pub fn phase(&self) -> Phase {
        if self.phase.load(Ordering::Relaxed) == 1 {
            Phase::Verifying
        } else {
            Phase::Downloading
        }
    }
}
