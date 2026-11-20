//! Tests of the playback ring: ordering, wrap-around, accounting, flush and drain semantics, and a long concurrent run.

use std::sync::Arc;
use std::time::Duration;

use super::*;

#[test]
fn overrun_drops_the_excess_and_counts_it() {
    let buf = PlaybackBuffer::new(10);
    assert_eq!(buf.push(&[0.1; 8]), 8);
    assert_eq!(buf.push(&[0.2; 8]), 2);
    assert_eq!(buf.buffered_samples(), 10);
    let stats = buf.stats();
    assert_eq!(stats.overruns, 1);
    assert_eq!(stats.dropped_samples, 6);
    assert_eq!(stats.underruns, 0);
    // A write that fits does not count.
    let mut out = [0.0; 10];
    buf.pull(&mut out);
    buf.push(&[0.3; 5]);
    assert_eq!(buf.stats().overruns, 1);
}

#[test]
fn running_dry_mid_stream_is_an_underrun() {
    let buf = PlaybackBuffer::new(100);
    buf.push(&[0.5; 4]);
    let mut out = [1.0; 8];
    buf.pull(&mut out);
    assert_eq!(&out[..4], &[0.5; 4]);
    assert_eq!(&out[4..], &[0.0; 4], "padded with silence");
    assert_eq!(buf.stats().underruns, 1);
    assert!(buf.is_drained());
    // Idle silence afterwards is not counted.
    buf.pull(&mut out);
    assert_eq!(buf.stats().underruns, 1);
}

#[test]
fn announced_end_of_stream_is_not_an_underrun() {
    let buf = PlaybackBuffer::new(100);
    buf.push(&[0.5; 4]);
    buf.end_stream();
    let mut out = [0.0; 8];
    buf.pull(&mut out);
    assert_eq!(buf.stats().underruns, 0);
    assert!(buf.is_drained());

    // An exact fit is not an underrun either.
    buf.push(&[0.5; 8]);
    buf.pull(&mut out);
    assert_eq!(buf.stats().underruns, 0);
}

#[test]
fn data_pushed_after_the_announced_end_starts_a_new_stream() {
    let buf = PlaybackBuffer::new(100);
    buf.push(&[0.5; 4]);
    buf.end_stream();
    buf.push(&[0.5; 4]);
    let mut out = [0.0; 16];
    buf.pull(&mut out);
    assert_eq!(
        buf.stats().underruns,
        1,
        "the second stream ran dry unannounced"
    );
}

#[test]
fn samples_wrap_around_the_ring_in_order() {
    let buf = PlaybackBuffer::new(7);
    let mut next = 0u32;
    let mut expected = 0u32;
    let mut out = [0.0; 5];
    for _ in 0..200 {
        let chunk: Vec<f32> = (0..5).map(|i| (next + i) as f32).collect();
        assert_eq!(buf.push(&chunk), 5);
        next += 5;
        buf.pull(&mut out);
        for v in out {
            assert_eq!(v, expected as f32);
            expected += 1;
        }
    }
    assert_eq!(buf.stats().overruns, 0);
    assert_eq!(
        buf.stats().underruns,
        0,
        "every pull emptied the ring exactly"
    );
}

#[test]
fn flush_is_visible_to_the_writer_before_any_callback_runs() {
    let buf = PlaybackBuffer::new(100);
    buf.push(&[0.5; 40]);
    assert_eq!(buf.flush(), 40);
    assert_eq!(buf.buffered_samples(), 0);
    assert!(buf.is_drained());
    assert!(buf.wait_drained(Duration::from_millis(1)));
    // Room is available again at once, and only the new samples play.
    assert_eq!(
        buf.push(&[0.9; 100]),
        100,
        "flushed samples no longer take room"
    );
    let mut out = [0.0; 100];
    buf.pull(&mut out);
    assert_eq!(out, [0.9; 100]);
    assert_eq!(buf.stats().overruns, 0);
}

#[test]
fn the_callback_drops_flushed_samples_and_keeps_later_ones() {
    let buf = PlaybackBuffer::new(100);
    buf.push(&[0.1; 30]);
    buf.flush();
    buf.push(&[0.2; 10]);
    let mut out = [1.0; 16];
    buf.pull(&mut out);
    assert_eq!(&out[..10], &[0.2; 10]);
    assert_eq!(&out[10..], &[0.0; 6]);
    assert_eq!(buf.stats().underruns, 1);
}

#[test]
fn flushing_an_idle_buffer_discards_nothing() {
    let buf = PlaybackBuffer::new(10);
    assert_eq!(buf.flush(), 0);
    assert_eq!(buf.push(&[0.5; 10]), 10);
    assert_eq!(buf.flush(), 10);
}

#[test]
fn wait_drained_wakes_when_the_callback_empties_the_buffer() {
    let buf = Arc::new(PlaybackBuffer::new(100));
    buf.push(&[0.5; 16]);
    assert!(
        !buf.wait_drained(Duration::from_millis(5)),
        "times out while data is queued"
    );

    let consumer = {
        let buf = Arc::clone(&buf);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            let mut out = [0.0; 16];
            buf.pull(&mut out);
        })
    };
    assert!(buf.wait_drained(Duration::from_secs(5)));
    consumer.join().unwrap();
}

/// A producer and a consumer thread move two million samples through a small
/// ring. Every sample is a distinct positive integer (zero is the silence the callback pads with), so the order and the checksum
/// show that nothing was lost, repeated or reordered.
#[test]
fn producer_and_consumer_threads_keep_order_and_checksum_over_a_long_run() {
    const TOTAL: u32 = 2_000_000;
    let buf = Arc::new(PlaybackBuffer::new(4_099));

    let producer = {
        let buf = Arc::clone(&buf);
        std::thread::spawn(move || {
            let mut next = 0u32;
            let mut chunk_len = 1usize;
            while next < TOTAL {
                chunk_len = chunk_len % 977 + 1;
                let end = (next as usize + chunk_len).min(TOTAL as usize) as u32;
                let chunk: Vec<f32> = (next..end).map(|v| (v + 1) as f32).collect();
                let mut sent = 0;
                while sent < chunk.len() {
                    let accepted = buf.push(&chunk[sent..]);
                    sent += accepted;
                    if accepted == 0 {
                        std::thread::yield_now();
                    }
                }
                next = end;
            }
        })
    };

    let mut received = 0u32;
    let mut checksum = 0u64;
    let mut out = vec![0.0f32; 313];
    while received < TOTAL {
        let before = buf.buffered_samples();
        buf.pull(&mut out);
        let got = before.min(out.len());
        // Only the leading `got` samples can be real; the rest is silence
        // or samples that arrived during the pull, so count what is in order.
        let mut i = 0;
        while i < out.len() && out[i] == (received + 1) as f32 && received < TOTAL {
            checksum += u64::from(received) + 1;
            received += 1;
            i += 1;
        }
        assert!(
            i >= got,
            "pull returned {i} in-order samples, {got} were queued"
        );
        if i == 0 {
            std::thread::yield_now();
        }
    }
    producer.join().unwrap();
    let expected: u64 = (1..=u64::from(TOTAL)).sum();
    assert_eq!(checksum, expected);
    assert!(buf.is_drained());
}
