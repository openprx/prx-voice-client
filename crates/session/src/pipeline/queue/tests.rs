//! Tests of every overflow policy and of the interruption paths of the bounded queue.

use std::sync::Arc;
use std::thread;

use prxvc_media::ManualClock;

use super::*;

fn drain<T>(q: &BoundedQueue<T>) -> Vec<T> {
    let mut seen = 0;
    let mut out = Vec::new();
    while let Popped::Item(i) = q.pop(Wait::Until(MonoTime::ZERO, &ManualClock::new()), &mut seen) {
        out.push(i);
    }
    out
}

#[test]
fn items_come_out_in_order() {
    let q = BoundedQueue::new("t", 4, Policy::Block);
    for i in 0..3 {
        assert!(matches!(q.push(i, None), Pushed::Accepted));
    }
    assert_eq!(drain(&q), vec![0, 1, 2]);
    let s = q.stats();
    assert_eq!((s.pushed, s.popped, s.high_water), (3, 3, 3));
}

#[test]
fn drop_oldest_evicts_the_front_and_returns_it() {
    let q = BoundedQueue::new("t", 2, Policy::DropOldest);
    q.push(1, None);
    q.push(2, None);
    match q.push(3, None) {
        Pushed::Evicted(old) => assert_eq!(old, vec![1]),
        other => panic!("expected an eviction, got {other:?}"),
    }
    assert_eq!(drain(&q), vec![2, 3]);
    assert_eq!(q.stats().evicted, 1);
}

#[test]
fn drop_newest_and_fail_item_refuse_the_new_item() {
    for policy in [Policy::DropNewest, Policy::FailItem] {
        let q = BoundedQueue::new("t", 1, policy);
        q.push(1, None);
        match q.push(2, None) {
            Pushed::Rejected(item) => assert_eq!(item, 2),
            other => panic!("expected a rejection, got {other:?}"),
        }
        assert_eq!(drain(&q), vec![1]);
        assert_eq!(q.stats().rejected, 1);
    }
}

#[test]
fn weighted_capacity_counts_weight_and_accepts_one_oversize_item() {
    let q = BoundedQueue::weighted("audio", 100, Policy::DropOldest, |n: &usize| *n);
    q.push(60, None);
    match q.push(60, None) {
        Pushed::Evicted(old) => assert_eq!(old, vec![60]),
        other => panic!("{other:?}"),
    }
    assert_eq!(q.stats().weight, 60);
    let big = BoundedQueue::weighted("audio", 10, Policy::Block, |n: &usize| *n);
    assert!(
        matches!(big.push(500, None), Pushed::Accepted),
        "an empty queue takes any item"
    );
    assert!(big.stats().high_water >= 500);
}

#[test]
fn a_blocked_push_waits_for_room() {
    let q = Arc::new(BoundedQueue::new("t", 1, Policy::Block));
    q.push(1, None);
    let producer = {
        let q = Arc::clone(&q);
        thread::spawn(move || matches!(q.push(2, None), Pushed::Accepted))
    };
    let mut seen = 0;
    assert_eq!(q.pop(Wait::Forever, &mut seen), Popped::Item(1));
    assert!(producer.join().expect("producer"));
    assert_eq!(q.pop(Wait::Forever, &mut seen), Popped::Item(2));
}

#[test]
fn a_blocked_push_is_interrupted_by_its_token_after_a_wake() {
    let q = Arc::new(BoundedQueue::new("t", 1, Policy::Block));
    q.push(1, None);
    let token = CancellationToken::new();
    let producer = {
        let (q, token) = (Arc::clone(&q), token.clone());
        thread::spawn(move || matches!(q.push(2, Some(&token)), Pushed::Cancelled(2)))
    };
    token.cancel();
    q.wake();
    assert!(producer.join().expect("producer"));
}

#[test]
fn a_blocked_push_ends_when_the_queue_closes() {
    let q = Arc::new(BoundedQueue::new("t", 1, Policy::Block));
    q.push(1, None);
    let producer = {
        let q = Arc::clone(&q);
        thread::spawn(move || matches!(q.push(2, None), Pushed::Closed(2)))
    };
    q.close();
    assert!(producer.join().expect("producer"));
    let mut seen = 0;
    assert_eq!(
        q.pop(Wait::Forever, &mut seen),
        Popped::Item(1),
        "the consumer drains first"
    );
    assert_eq!(q.pop(Wait::Forever, &mut seen), Popped::Closed);
}

#[test]
fn urgent_items_jump_the_line_in_order_even_when_full() {
    let q = BoundedQueue::new("t", 1, Policy::Block);
    q.push(10, None);
    assert!(matches!(q.push_urgent(1), Pushed::Accepted));
    assert!(matches!(q.push_urgent(2), Pushed::Accepted));
    assert_eq!(drain(&q), vec![1, 2, 10]);
}

#[test]
fn the_urgent_lane_is_bounded() {
    let q: BoundedQueue<usize> = BoundedQueue::new("t", 1, Policy::Block);
    let refused = (0..URGENT_LANE + 5)
        .filter(|i| matches!(q.push_urgent(*i), Pushed::Rejected(_)))
        .count();
    assert_eq!(refused, 5);
}

#[test]
fn push_replacing_coalesces_a_matching_item_in_place() {
    let q = BoundedQueue::new("t", 2, Policy::Block);
    q.push((1, "a"), None);
    q.push((2, "b"), None);
    assert!(matches!(
        q.push_replacing((1, "a2"), None, |(k, _)| *k == 1),
        Pushed::Accepted
    ));
    assert_eq!(drain(&q), vec![(1, "a2"), (2, "b")]);
    assert_eq!(q.stats().coalesced, 1);
}

#[test]
fn purge_removes_only_matching_items_and_frees_room() {
    let q = BoundedQueue::new("t", 4, Policy::Block);
    for i in 0..4 {
        q.push(i, None);
    }
    assert_eq!(q.purge(|i| i % 2 == 1), vec![1, 3]);
    assert_eq!(q.stats().weight, 2);
    assert_eq!(drain(&q), vec![0, 2]);
    assert_eq!(q.stats().purged, 2);
}

#[test]
fn wake_returns_woken_once_per_wake() {
    let q: BoundedQueue<u8> = BoundedQueue::new("t", 1, Policy::Block);
    let mut seen = 0;
    q.wake();
    assert_eq!(q.pop(Wait::Forever, &mut seen), Popped::Woken);
    q.push(7, None);
    q.wake();
    assert_eq!(
        q.pop(Wait::Forever, &mut seen),
        Popped::Item(7),
        "items first"
    );
    assert_eq!(q.pop(Wait::Forever, &mut seen), Popped::Woken);
}

#[test]
fn a_pop_with_a_deadline_times_out_on_the_injected_clock_only() {
    let q: Arc<BoundedQueue<u8>> = Arc::new(BoundedQueue::new("t", 1, Policy::Block));
    let clock = ManualClock::new();
    let consumer = {
        let (q, clock) = (Arc::clone(&q), clock.clone());
        thread::spawn(move || {
            let mut seen = 0;
            q.pop(Wait::Until(MonoTime::from_millis(100), &clock), &mut seen)
        })
    };
    clock.advance(Duration::from_millis(60));
    clock.advance(Duration::from_millis(60));
    assert_eq!(consumer.join().expect("consumer"), Popped::Timeout);
}

#[test]
fn close_discard_drops_everything_queued() {
    let q = BoundedQueue::new("t", 4, Policy::Block);
    q.push(1, None);
    q.push_urgent(2);
    assert_eq!(q.close_discard().len(), 2);
    assert!(matches!(q.push(3, None), Pushed::Closed(3)));
    let mut seen = 0;
    assert_eq!(q.pop(Wait::Forever, &mut seen), Popped::Closed);
}
