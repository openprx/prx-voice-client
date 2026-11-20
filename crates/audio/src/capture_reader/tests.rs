//! Tests of the capture reader: downmix, 20 ms re-cutting, timestamps and overrun marking, with the callback side driven from the test.

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Instant;

use super::*;
use crate::cpal_input_rt::CaptureWriter;
use crate::tap_ring::tap_ring;

struct Rig {
    writer: CaptureWriter,
    reader: CaptureReader,
    dropped: Arc<AtomicU64>,
}

fn rig(channels: u16, rate: u32, slots: usize) -> Rig {
    let (producer, consumer) = tap_ring(slots);
    let dropped = Arc::new(AtomicU64::new(0));
    let writer = CaptureWriter::new(
        producer,
        channels,
        rate,
        Arc::clone(&dropped),
        Arc::new(AtomicBool::new(true)),
        Instant::now(),
    );
    let reader = CaptureReader::new(consumer, MonoTime::from_millis(1_000), Arc::clone(&dropped));
    Rig {
        writer,
        reader,
        dropped,
    }
}

fn drain(reader: &mut CaptureReader) -> Vec<AudioFrame> {
    let mut out = Vec::new();
    reader.drain(&mut out);
    out
}

#[test]
fn stereo_input_is_downmixed_to_mono() {
    let mut r = rig(2, 16_000, 8);
    let data: Vec<f32> = (0..320).flat_map(|_| [1.0, -0.5]).collect();
    r.writer.write(&data);
    let frames = drain(&mut r.reader);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].len(), 320);
    assert!(frames[0].samples().iter().all(|v| (*v - 0.25).abs() < 1e-6));
}

#[test]
fn callbacks_of_any_size_are_recut_into_20_ms_frames() {
    let mut r = rig(1, 48_000, 64);
    let mut next = 0u32;
    for len in [100usize, 512, 700, 1_000, 33, 2_600] {
        let data: Vec<f32> = (next..next + len as u32).map(|v| v as f32).collect();
        next += len as u32;
        r.writer.write(&data);
    }
    let frames = drain(&mut r.reader);
    assert_eq!(
        frames.len(),
        (next as usize) / 960,
        "only whole 20 ms frames"
    );
    let mut expected = 0u32;
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(f.len(), 960);
        assert_eq!(f.seq(), i as u64);
        assert_eq!(f.rate(), SampleRate::HZ_48000);
        assert!(!f.discontinuity());
        for &s in f.samples() {
            assert_eq!(s, expected as f32);
            expected += 1;
        }
    }
}

#[test]
fn a_partial_frame_waits_for_the_next_drain() {
    let mut r = rig(1, 16_000, 8);
    r.writer.write(&[0.5; 200]);
    assert!(drain(&mut r.reader).is_empty());
    r.writer.write(&[0.5; 200]);
    let frames = drain(&mut r.reader);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].len(), 320);
}

#[test]
fn timestamps_start_at_the_origin_and_never_go_backwards() {
    let mut r = rig(1, 16_000, 64);
    for _ in 0..10 {
        r.writer.write(&[0.1; 320]);
    }
    let frames = drain(&mut r.reader);
    assert_eq!(frames.len(), 10);
    assert!(
        frames[0].ts() >= MonoTime::from_millis(1_000),
        "stamped on the pipeline clock"
    );
    for pair in frames.windows(2) {
        assert!(pair[1].ts() >= pair[0].ts());
        assert_eq!(pair[1].seq(), pair[0].seq() + 1);
    }
}

#[test]
fn frames_cut_from_one_callback_are_exactly_20_ms_apart() {
    let mut r = rig(1, 16_000, 8);
    r.writer.write(&[0.1; 1_920]);
    let frames = drain(&mut r.reader);
    assert_eq!(frames.len(), 6);
    for pair in frames.windows(2) {
        assert_eq!(pair[1].ts().saturating_since(pair[0].ts()), FRAME_DURATION);
        assert_eq!(pair[0].end_ts(), pair[1].ts());
    }
}

#[test]
fn a_full_ring_drops_audio_counts_it_and_marks_the_next_frame() {
    // Two slots, nobody reading: the third callback does not fit.
    let mut r = rig(1, 16_000, 2);
    r.writer.write(&[0.1; 320]);
    r.writer.write(&[0.2; 320]);
    r.writer.write(&[0.3; 320]);
    assert_eq!(r.dropped.load(Ordering::Relaxed), 320);
    let first = drain(&mut r.reader);
    assert_eq!(first.len(), 2);
    assert!(
        first[0].discontinuity(),
        "the overrun is reported on the next frame read"
    );
    assert!(!first[1].discontinuity());
    // Space is back and the mark is not repeated.
    r.writer.write(&[0.4; 320]);
    let later = drain(&mut r.reader);
    assert_eq!(later.len(), 1);
    assert!(!later[0].discontinuity());
}

#[test]
fn a_stopped_writer_ignores_the_device() {
    let (producer, consumer) = tap_ring(4);
    let running = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicU64::new(0));
    let mut writer = CaptureWriter::new(
        producer,
        1,
        16_000,
        Arc::clone(&dropped),
        running,
        Instant::now(),
    );
    let mut reader = CaptureReader::new(consumer, MonoTime::ZERO, dropped);
    writer.write(&[0.5; 320]);
    assert!(drain(&mut reader).is_empty());
}

#[test]
fn clear_discards_queued_audio_and_the_partial_frame() {
    let mut r = rig(1, 16_000, 8);
    r.writer.write(&[0.5; 100]);
    r.writer.write(&[0.5; 320]);
    r.reader.clear();
    assert!(drain(&mut r.reader).is_empty());
}
