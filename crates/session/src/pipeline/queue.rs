//! Bounded queue with a declared overflow policy: the only way items cross between the threads of a direction pipeline.

use std::collections::VecDeque;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use prxvc_media::{Clock, MonoTime};
use prxvc_ports::CancellationToken;

/// How long a consumer that waits for a clock deadline sleeps between looks at the clock.
///
/// Only the latency of noticing that an injected clock moved depends on it.
const CLOCK_SLICE: Duration = Duration::from_millis(1);

/// Commands that jump the line (stop, skip, playback feedback) are few; this
/// bounds the line they jump into.
const URGENT_LANE: usize = 64;

/// What a full queue does with the next item (CS-4.3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Policy {
    /// The producer waits (backpressure upstream) until there is room, the
    /// queue closes or the producer's token is cancelled.
    Block,
    /// The oldest queued items make room; the push returns them.
    DropOldest,
    /// The new item is refused and returned.
    DropNewest,
    /// The new item is refused and returned, and the owner of the item fails
    /// it (a clause, a segment) instead of losing it silently.
    FailItem,
}

/// The result of a push.
#[derive(Debug)]
pub enum Pushed<T> {
    /// Queued.
    Accepted,
    /// Queued after the returned (older) items were evicted to make room.
    Evicted(Vec<T>),
    /// Refused because the queue was full; the item comes back.
    Rejected(T),
    /// The queue is closed; the item comes back.
    Closed(T),
    /// A blocked push gave up because its token was cancelled; the item comes back.
    Cancelled(T),
}

/// The result of a pop.
#[derive(Debug, PartialEq, Eq)]
pub enum Popped<T> {
    /// The next item.
    Item(T),
    /// The queue is closed and empty.
    Closed,
    /// The clock deadline passed with nothing queued.
    Timeout,
    /// [`BoundedQueue::wake`] was called since the consumer last looked.
    Woken,
}

/// How long a pop waits.
#[derive(Clone, Copy)]
pub enum Wait<'a> {
    /// Until an item arrives, the queue closes or it is woken.
    Forever,
    /// Additionally until `clock` reaches the time.
    Until(MonoTime, &'a dyn Clock),
}

/// Counters of one queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueStats {
    /// The queue's name in the module table.
    pub name: &'static str,
    /// Capacity in the queue's unit (items, or milliseconds of audio).
    pub capacity: usize,
    /// The overflow policy.
    pub policy: Policy,
    /// Items queued now.
    pub len: usize,
    /// Weight queued now.
    pub weight: usize,
    /// The largest weight ever queued (at most `capacity`, except for one item heavier than the capacity).
    pub high_water: usize,
    /// Items accepted.
    pub pushed: u64,
    /// Items taken by the consumer.
    pub popped: u64,
    /// Items evicted by `DropOldest`.
    pub evicted: u64,
    /// Items refused by `DropNewest` or `FailItem`.
    pub rejected: u64,
    /// Items removed by [`BoundedQueue::purge`].
    pub purged: u64,
    /// Items replaced in place by [`BoundedQueue::push_replacing`].
    pub coalesced: u64,
}

struct Inner<T> {
    items: VecDeque<(T, usize)>,
    urgent: VecDeque<T>,
    weight: usize,
    closed: bool,
    wakeups: u64,
    high_water: usize,
    pushed: u64,
    popped: u64,
    evicted: u64,
    rejected: u64,
    purged: u64,
    coalesced: u64,
}

/// A bounded multi-producer queue with one policy and a weight per item.
///
/// Capacity is a total weight: item count for most queues, milliseconds of
/// audio for the capture queue. An item heavier than the capacity is accepted
/// into an empty queue, so a single large item cannot wedge a stage.
pub struct BoundedQueue<T> {
    name: &'static str,
    capacity: usize,
    policy: Policy,
    weigh: fn(&T) -> usize,
    inner: Mutex<Inner<T>>,
    not_empty: Condvar,
    not_full: Condvar,
}

impl<T> BoundedQueue<T> {
    /// A queue of `capacity` items.
    pub fn new(name: &'static str, capacity: usize, policy: Policy) -> Self {
        Self::weighted(name, capacity, policy, |_| 1)
    }

    /// A queue whose capacity counts `weigh(item)` per item (at least 1).
    pub fn weighted(
        name: &'static str,
        capacity: usize,
        policy: Policy,
        weigh: fn(&T) -> usize,
    ) -> Self {
        Self {
            name,
            capacity: capacity.max(1),
            policy,
            weigh,
            inner: Mutex::new(Inner {
                items: VecDeque::new(),
                urgent: VecDeque::new(),
                weight: 0,
                closed: false,
                wakeups: 0,
                high_water: 0,
                pushed: 0,
                popped: 0,
                evicted: 0,
                rejected: 0,
                purged: 0,
                coalesced: 0,
            }),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
        }
    }

    /// Queue `item` under the queue's policy. `cancel` interrupts a push that
    /// has to wait (after a [`BoundedQueue::wake`]); it never refuses an item that fits.
    pub fn push(&self, item: T, cancel: Option<&CancellationToken>) -> Pushed<T> {
        let weight = (self.weigh)(&item).max(1);
        let mut g = self.inner.lock();
        let mut evicted = Vec::new();
        loop {
            if g.closed {
                return Pushed::Closed(item);
            }
            let fits = g.items.is_empty() || g.weight + weight <= self.capacity;
            if fits {
                self.enqueue(&mut g, item, weight);
                return if evicted.is_empty() {
                    Pushed::Accepted
                } else {
                    Pushed::Evicted(evicted)
                };
            }
            match self.policy {
                Policy::Block => {
                    // The token only ends a wait: an item that fits is queued whatever the token says.
                    if cancel.is_some_and(CancellationToken::is_cancelled) {
                        return Pushed::Cancelled(item);
                    }
                    self.not_full.wait(&mut g);
                }
                Policy::DropOldest => {
                    if let Some((old, w)) = g.items.pop_front() {
                        g.weight -= w;
                        g.evicted += 1;
                        evicted.push(old);
                    }
                }
                Policy::DropNewest | Policy::FailItem => {
                    g.rejected += 1;
                    return Pushed::Rejected(item);
                }
            }
        }
    }

    /// Queue `item`, replacing in place the queued item for which `same`
    /// holds (the newest partial of a segment replaces its predecessor).
    pub fn push_replacing(
        &self,
        item: T,
        cancel: Option<&CancellationToken>,
        same: impl Fn(&T) -> bool,
    ) -> Pushed<T> {
        {
            let mut g = self.inner.lock();
            if g.closed {
                return Pushed::Closed(item);
            }
            let weight = (self.weigh)(&item).max(1);
            if let Some(slot) = g.items.iter().position(|(queued, _)| same(queued)) {
                let old_weight = g.items[slot].1;
                g.items[slot] = (item, weight);
                g.weight = g.weight - old_weight + weight;
                g.coalesced += 1;
                return Pushed::Accepted;
            }
        }
        self.push(item, cancel)
    }

    /// Queue a command ahead of everything queued, whatever the policy and
    /// however full the queue is. The lane is bounded; a full lane refuses.
    pub fn push_urgent(&self, item: T) -> Pushed<T> {
        let mut g = self.inner.lock();
        if g.closed {
            return Pushed::Closed(item);
        }
        if g.urgent.len() >= URGENT_LANE {
            g.rejected += 1;
            return Pushed::Rejected(item);
        }
        g.urgent.push_back(item);
        g.pushed += 1;
        drop(g);
        self.not_empty.notify_one();
        Pushed::Accepted
    }

    fn enqueue(&self, g: &mut Inner<T>, item: T, weight: usize) {
        g.items.push_back((item, weight));
        g.weight += weight;
        g.high_water = g.high_water.max(g.weight);
        g.pushed += 1;
        self.not_empty.notify_one();
    }

    /// Take the next item; urgent commands come first. `seen_wake` is the
    /// consumer's own count of wake-ups it has handled.
    pub fn pop(&self, wait: Wait<'_>, seen_wake: &mut u64) -> Popped<T> {
        let mut g = self.inner.lock();
        loop {
            if let Some(item) = g.urgent.pop_front() {
                g.popped += 1;
                return Popped::Item(item);
            }
            if let Some((item, w)) = g.items.pop_front() {
                g.weight -= w;
                g.popped += 1;
                drop(g);
                self.not_full.notify_one();
                return Popped::Item(item);
            }
            if g.wakeups != *seen_wake {
                *seen_wake = g.wakeups;
                return Popped::Woken;
            }
            if g.closed {
                return Popped::Closed;
            }
            match wait {
                Wait::Forever => self.not_empty.wait(&mut g),
                Wait::Until(deadline, clock) => {
                    if clock.now() >= deadline {
                        return Popped::Timeout;
                    }
                    let _ = self.not_empty.wait_for(&mut g, CLOCK_SLICE);
                }
            }
        }
    }

    /// Take the next item if there is one, without waiting.
    pub fn try_pop(&self) -> Option<T> {
        let mut g = self.inner.lock();
        if let Some(item) = g.urgent.pop_front() {
            g.popped += 1;
            return Some(item);
        }
        let (item, w) = g.items.pop_front()?;
        g.weight -= w;
        g.popped += 1;
        drop(g);
        self.not_full.notify_one();
        Some(item)
    }

    /// Remove and return every queued item for which `gone` holds (the items
    /// of a cancelled scope); blocked producers are woken.
    pub fn purge(&self, gone: impl Fn(&T) -> bool) -> Vec<T> {
        let mut g = self.inner.lock();
        let mut removed = Vec::new();
        let mut kept = VecDeque::with_capacity(g.items.len());
        let mut weight = 0;
        for (item, w) in std::mem::take(&mut g.items) {
            if gone(&item) {
                removed.push(item);
            } else {
                weight += w;
                kept.push_back((item, w));
            }
        }
        g.items = kept;
        g.weight = weight;
        g.purged += removed.len() as u64;
        drop(g);
        self.not_full.notify_all();
        removed
    }

    /// Wake the consumer (one `Popped::Woken`) and every blocked producer, so
    /// that they look at their cancellation tokens and flags again.
    pub fn wake(&self) {
        self.inner.lock().wakeups += 1;
        self.not_empty.notify_all();
        self.not_full.notify_all();
    }

    /// Refuse further pushes; the consumer drains what is queued, then sees `Closed`.
    pub fn close(&self) {
        self.inner.lock().closed = true;
        self.not_empty.notify_all();
        self.not_full.notify_all();
    }

    /// Close and drop everything queued.
    pub fn close_discard(&self) -> Vec<T> {
        let mut g = self.inner.lock();
        g.closed = true;
        let mut dropped: Vec<T> = g.items.drain(..).map(|(item, _)| item).collect();
        dropped.extend(g.urgent.drain(..));
        g.weight = 0;
        drop(g);
        self.not_empty.notify_all();
        self.not_full.notify_all();
        dropped
    }

    /// Items queued now.
    pub fn len(&self) -> usize {
        let g = self.inner.lock();
        g.items.len() + g.urgent.len()
    }

    /// Counters and fill level.
    pub fn stats(&self) -> QueueStats {
        let g = self.inner.lock();
        QueueStats {
            name: self.name,
            capacity: self.capacity,
            policy: self.policy,
            len: g.items.len() + g.urgent.len(),
            weight: g.weight,
            high_water: g.high_water,
            pushed: g.pushed,
            popped: g.popped,
            evicted: g.evicted,
            rejected: g.rejected,
            purged: g.purged,
            coalesced: g.coalesced,
        }
    }
}

#[cfg(test)]
mod tests;
