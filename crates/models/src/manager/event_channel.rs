//! `EventReceiver`: the bounded, drop-oldest event subscription of the model
//! manager (design 4.7, CS-4.3). Single responsibility: a small fan-out queue
//! whose sender never blocks and never grows without bound.
//!
//! The snapshot is the source of truth. A subscriber that falls behind loses
//! the oldest events, `dropped_count` says how many, and the subscriber
//! re-reads the snapshot instead of trusting its event history.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{RecvError, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

/// Events kept per subscriber before the oldest is dropped.
pub const EVENT_CAPACITY: usize = 64;

struct State<T> {
    queue: VecDeque<T>,
    dropped: u64,
    receiver_gone: bool,
    sender_gone: bool,
}

struct Inner<T> {
    state: Mutex<State<T>>,
    ready: Condvar,
    capacity: usize,
}

/// Producer half, held by the manager. Pushing never blocks.
pub(crate) struct EventSender<T> {
    inner: Arc<Inner<T>>,
}

/// Consumer half handed to subscribers.
pub struct EventReceiver<T> {
    inner: Arc<Inner<T>>,
}

pub(crate) fn event_channel<T>(capacity: usize) -> (EventSender<T>, EventReceiver<T>) {
    let capacity = capacity.max(1);
    let inner = Arc::new(Inner {
        state: Mutex::new(State {
            queue: VecDeque::with_capacity(capacity),
            dropped: 0,
            receiver_gone: false,
            sender_gone: false,
        }),
        ready: Condvar::new(),
        capacity,
    });
    (
        EventSender {
            inner: inner.clone(),
        },
        EventReceiver { inner },
    )
}

/// The manager's subscriber list: one bounded queue per subscriber.
pub(crate) struct Subscribers<T> {
    list: Mutex<Vec<EventSender<T>>>,
}

impl<T> Default for Subscribers<T> {
    fn default() -> Self {
        Self {
            list: Mutex::new(Vec::new()),
        }
    }
}

impl<T: Clone> Subscribers<T> {
    /// Add a subscriber with its own queue of `EVENT_CAPACITY` events.
    pub(crate) fn subscribe(&self) -> EventReceiver<T> {
        self.subscribe_with(EVENT_CAPACITY)
    }

    fn subscribe_with(&self, capacity: usize) -> EventReceiver<T> {
        let (tx, rx) = event_channel(capacity);
        self.list.lock().push(tx);
        rx
    }

    /// Deliver to every subscriber without blocking; forget the gone ones.
    pub(crate) fn emit(&self, ev: T) {
        self.list.lock().retain(|s| s.push(ev.clone()));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.list.lock().len()
    }
}

impl<T> EventSender<T> {
    /// Queue `ev`, dropping the oldest event when full. Returns `false` once
    /// the receiver is gone, so the caller can forget this subscriber.
    pub(crate) fn push(&self, ev: T) -> bool {
        let mut st = self.inner.state.lock();
        if st.receiver_gone {
            return false;
        }
        if st.queue.len() >= self.inner.capacity {
            st.queue.pop_front();
            st.dropped += 1;
        }
        st.queue.push_back(ev);
        drop(st);
        self.inner.ready.notify_one();
        true
    }
}

impl<T> Drop for EventSender<T> {
    fn drop(&mut self) {
        self.inner.state.lock().sender_gone = true;
        self.inner.ready.notify_all();
    }
}

impl<T> Drop for EventReceiver<T> {
    fn drop(&mut self) {
        let mut st = self.inner.state.lock();
        st.receiver_gone = true;
        st.queue.clear();
    }
}

impl<T> EventReceiver<T> {
    /// Events dropped so far because this receiver did not keep up. When the
    /// value changed since the last look, re-read the manager snapshot.
    pub fn dropped_count(&self) -> u64 {
        self.inner.state.lock().dropped
    }

    /// The next event if one is queued.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let mut st = self.inner.state.lock();
        match st.queue.pop_front() {
            Some(ev) => Ok(ev),
            None if st.sender_gone => Err(TryRecvError::Disconnected),
            None => Err(TryRecvError::Empty),
        }
    }

    /// Wait for the next event until `timeout` has passed.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        let mut st = self.inner.state.lock();
        loop {
            if let Some(ev) = st.queue.pop_front() {
                return Ok(ev);
            }
            if st.sender_gone {
                return Err(RecvTimeoutError::Disconnected);
            }
            if self.inner.ready.wait_until(&mut st, deadline).timed_out() {
                return match st.queue.pop_front() {
                    Some(ev) => Ok(ev),
                    None => Err(RecvTimeoutError::Timeout),
                };
            }
        }
    }

    /// Wait for the next event with no time limit.
    pub fn recv(&self) -> Result<T, RecvError> {
        let mut st = self.inner.state.lock();
        loop {
            if let Some(ev) = st.queue.pop_front() {
                return Ok(ev);
            }
            if st.sender_gone {
                return Err(RecvError);
            }
            self.inner.ready.wait(&mut st);
        }
    }

    /// Drain what is queued now without waiting.
    pub fn try_iter(&self) -> impl Iterator<Item = T> + '_ {
        std::iter::from_fn(|| self.try_recv().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivers_in_order_and_reports_empty_then_disconnected() {
        let (tx, rx) = event_channel::<u32>(4);
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
        assert!(tx.push(1));
        assert!(tx.push(2));
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![1, 2]);
        drop(tx);
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
        assert_eq!(rx.recv(), Err(RecvError));
    }

    #[test]
    fn a_full_queue_drops_the_oldest_and_counts_it() {
        let (tx, rx) = event_channel::<u32>(3);
        for i in 0..10 {
            assert!(tx.push(i));
        }
        assert_eq!(rx.dropped_count(), 7);
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![7, 8, 9]);
    }

    #[test]
    fn recv_timeout_times_out_and_wakes_on_push() {
        let (tx, rx) = event_channel::<u32>(2);
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(20)),
            Err(RecvTimeoutError::Timeout)
        );
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            tx.push(5);
            tx
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(5));
        let _tx = t.join().unwrap();
    }

    #[test]
    fn push_reports_a_dropped_receiver() {
        let (tx, rx) = event_channel::<u32>(2);
        drop(rx);
        assert!(!tx.push(1));
    }

    #[test]
    fn a_subscriber_that_never_reads_neither_blocks_nor_grows() {
        let subs = Subscribers::<u64>::default();
        let idle = subs.subscribe();
        let active = subs.subscribe();
        let started = Instant::now();
        let mut seen = 0u64;
        for i in 0..100_000u64 {
            subs.emit(i);
            if let Ok(v) = active.try_recv() {
                assert_eq!(v, i);
                seen += 1;
            }
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(seen, 100_000);
        assert_eq!(idle.inner.state.lock().queue.len(), EVENT_CAPACITY);
        assert_eq!(idle.dropped_count(), 100_000 - EVENT_CAPACITY as u64);
        assert_eq!(active.dropped_count(), 0);
    }

    #[test]
    fn a_lagging_subscriber_sees_the_marker_and_the_latest_events() {
        let subs = Subscribers::<u64>::default();
        let rx = subs.subscribe_with(8);
        // The "snapshot" is the last value the manager published.
        let mut snapshot = 0;
        for i in 1..=20u64 {
            snapshot = i;
            subs.emit(i);
        }
        assert_eq!(rx.dropped_count(), 12);
        let got: Vec<u64> = rx.try_iter().collect();
        assert_eq!(got, (13..=20).collect::<Vec<_>>());
        // After noticing the gap the subscriber re-reads the snapshot; the
        // newest queued event agrees with it.
        assert_eq!(*got.last().unwrap(), snapshot);
    }

    #[test]
    fn a_dropped_subscriber_is_removed_on_the_next_emit() {
        let subs = Subscribers::<u64>::default();
        let kept = subs.subscribe();
        let gone = subs.subscribe();
        assert_eq!(subs.len(), 2);
        drop(gone);
        subs.emit(1);
        assert_eq!(subs.len(), 1);
        assert_eq!(kept.try_recv(), Ok(1));
    }
}
