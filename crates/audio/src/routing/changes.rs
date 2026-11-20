//! Bounded queue of operating-system change notices: native callbacks post a kind, the controller drains and coalesces them.
//!
//! The callbacks of the HAL listener and the power monitor run on private
//! dispatch queues and must only record that something happened (CS-2.10.4):
//! [`ChangeSink::post`] is a non-blocking `try_send` into a bounded channel and
//! counts what it had to drop. What changed is read afterwards, on the polling
//! thread, so a notice carries no payload.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

/// Unread notices held; further ones are dropped (and counted). A notice is
/// posted per property change and the controller drains every poll (about once
/// a second at most), so a storm of 64 is far beyond what real hardware makes.
pub const QUEUE_CAPACITY: usize = 64;

/// What the system reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// The default input changed.
    DefaultInput,
    /// The default output changed.
    DefaultOutput,
    /// A device appeared or disappeared.
    DeviceList,
    /// The machine is going to sleep.
    Sleep,
    /// The machine woke.
    Wake,
}

/// The posting end, shared with native callbacks.
#[derive(Debug)]
pub struct ChangeSink {
    tx: SyncSender<Notice>,
    dropped: AtomicU64,
}

impl ChangeSink {
    /// Record a notice without blocking; a full queue counts a drop instead.
    pub fn post(&self, notice: Notice) {
        if self.tx.try_send(notice).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// What a drain found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drained {
    /// The notices, oldest first.
    pub notices: Vec<Notice>,
    /// Some notices were dropped because the queue was full.
    pub overflowed: bool,
}

/// The reading end plus the sink to hand to listeners.
#[derive(Debug)]
pub struct ChangeQueue {
    rx: Receiver<Notice>,
    sink: Arc<ChangeSink>,
}

impl ChangeQueue {
    /// A queue holding up to `capacity` unread notices.
    pub fn new(capacity: usize) -> Self {
        let (tx, rx) = sync_channel(capacity);
        Self {
            rx,
            sink: Arc::new(ChangeSink {
                tx,
                dropped: AtomicU64::new(0),
            }),
        }
    }

    /// The sink native callbacks post to.
    pub fn sink(&self) -> Arc<ChangeSink> {
        Arc::clone(&self.sink)
    }

    /// Take everything queued so far.
    pub fn drain(&self) -> Drained {
        let notices: Vec<Notice> = self.rx.try_iter().collect();
        Drained {
            notices,
            overflowed: self.sink.dropped.swap(0, Ordering::Relaxed) > 0,
        }
    }
}

/// Reduce a drained batch to what must be reported.
///
/// What a data notice (default input, default output, device list) means is
/// read at poll time, so only its last occurrence matters; sleep and wake keep
/// every occurrence and their order. After an overflow the default input and
/// the device list are reported again, because a dropped notice may have been
/// either.
pub fn coalesce(drained: &Drained) -> Vec<Notice> {
    let mut all = drained.notices.clone();
    if drained.overflowed {
        all.push(Notice::DefaultInput);
        all.push(Notice::DeviceList);
    }
    let is_data = |n: &Notice| !matches!(n, Notice::Sleep | Notice::Wake);
    let mut out: Vec<Notice> = Vec::with_capacity(all.len());
    for (i, n) in all.iter().enumerate() {
        if is_data(n) && all[i + 1..].contains(n) {
            continue;
        }
        out.push(*n);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_returns_notices_in_order_and_empties_the_queue() {
        let q = ChangeQueue::new(8);
        let sink = q.sink();
        sink.post(Notice::DeviceList);
        sink.post(Notice::Sleep);
        let d = q.drain();
        assert_eq!(d.notices, vec![Notice::DeviceList, Notice::Sleep]);
        assert!(!d.overflowed);
        assert!(q.drain().notices.is_empty());
    }

    #[test]
    fn a_full_queue_drops_without_blocking_and_reports_the_overflow_once() {
        let q = ChangeQueue::new(2);
        let sink = q.sink();
        for _ in 0..5 {
            sink.post(Notice::DefaultInput);
        }
        let d = q.drain();
        assert_eq!(d.notices.len(), 2);
        assert!(d.overflowed);
        assert!(!q.drain().overflowed);
    }

    #[test]
    fn data_notices_keep_their_last_occurrence_and_sleep_wake_keep_all() {
        let d = Drained {
            notices: vec![
                Notice::DefaultInput,
                Notice::DeviceList,
                Notice::DefaultInput,
                Notice::Sleep,
                Notice::Wake,
                Notice::Sleep,
            ],
            overflowed: false,
        };
        assert_eq!(
            coalesce(&d),
            vec![
                Notice::DeviceList,
                Notice::DefaultInput,
                Notice::Sleep,
                Notice::Wake,
                Notice::Sleep
            ]
        );
    }

    #[test]
    fn an_overflow_adds_a_resync_of_input_and_device_list() {
        let d = Drained {
            notices: vec![Notice::DefaultOutput],
            overflowed: true,
        };
        assert_eq!(
            coalesce(&d),
            vec![
                Notice::DefaultOutput,
                Notice::DefaultInput,
                Notice::DeviceList
            ]
        );
    }
}
