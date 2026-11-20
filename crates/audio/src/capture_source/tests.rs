//! Port conformance tests of the capture endpoint: the scenarios of the `AudioSource` fake in `prxvc-ports`, run against the real ring, plus a long concurrent run.

use std::sync::atomic::AtomicBool;

use parking_lot::Mutex;
use prxvc_media::{ManualClock, SampleRate};

use super::*;

const RATE: SampleRate = SampleRate::HZ_16000;
const MS: fn(u64) -> Duration = Duration::from_millis;

/// The test's handle on what the opener was given, standing in for the device.
#[derive(Default, Clone)]
struct Device {
    writer: Arc<Mutex<Option<CaptureWriter>>>,
    lost: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    opens: Arc<Mutex<u32>>,
}

impl Device {
    fn source(&self, channels: u16, fail: Option<EndpointError>) -> CaptureSource {
        let dev = self.clone();
        CaptureSource::new(
            RATE,
            channels,
            Arc::new(ManualClock::new()),
            Box::new(move |links: CaptureLinks| {
                *dev.opens.lock() += 1;
                if let Some(e) = &fail {
                    return Err(e.clone());
                }
                *dev.writer.lock() = Some(links.writer);
                *dev.lost.lock() = Some(links.lost);
                Ok(OpenedStream {
                    handle: Box::new(()),
                    device: "test mic".to_owned(),
                })
            }),
        )
    }

    /// One device callback delivering `frames` interleaved frames of `value`.
    fn callback(&self, channels: usize, frames: usize, value: f32) {
        let data = vec![value; frames * channels];
        self.writer.lock().as_mut().expect("opened").write(&data);
    }
}

#[test]
fn a_source_must_be_started_and_then_delivers_frames_in_order() {
    let dev = Device::default();
    let mut source = dev.source(1, None);
    let mut out = Vec::new();
    assert_eq!(source.read(&mut out, MS(1)), Err(EndpointError::NotStarted));
    let info = source.start().expect("start");
    assert_eq!(
        info,
        SourceInfo {
            rate: RATE,
            device: "test mic".to_owned()
        }
    );
    for i in 0..3 {
        dev.callback(1, 320, i as f32 + 1.0);
    }
    source.read(&mut out, MS(10)).expect("read");
    assert_eq!(
        out.iter().map(AudioFrame::seq).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(out[2].samples()[0], 3.0);
    source
        .read(&mut out, MS(1))
        .expect("an idle source reads empty, not an error");
    assert_eq!(out.len(), 3);
    let health = source.health();
    assert_eq!(
        (health.running, health.frames, health.dropped_samples),
        (true, 3, 0)
    );
    source.stop();
    assert!(!source.health().running);
    assert_eq!(source.read(&mut out, MS(1)), Err(EndpointError::NotStarted));
}

#[test]
fn read_waits_for_the_first_frame_up_to_max_wait() {
    let dev = Device::default();
    let mut source = dev.source(1, None);
    source.start().unwrap();
    let mut out = Vec::new();
    let began = Instant::now();
    source.read(&mut out, MS(30)).unwrap();
    assert!(out.is_empty());
    assert!(
        began.elapsed() >= MS(25),
        "waited for audio that never came"
    );

    let feeder = {
        let dev = dev.clone();
        std::thread::spawn(move || {
            std::thread::sleep(MS(20));
            dev.callback(1, 320, 0.5);
        })
    };
    source.read(&mut out, Duration::from_secs(5)).unwrap();
    feeder.join().unwrap();
    assert_eq!(out.len(), 1, "returned as soon as the frame arrived");
}

#[test]
fn a_source_can_fail_to_start_and_says_so_in_health() {
    let dev = Device::default();
    let mut source = dev.source(
        1,
        Some(EndpointError::Unavailable {
            reason: "no permission".to_owned(),
        }),
    );
    assert!(matches!(
        source.start(),
        Err(EndpointError::Unavailable { .. })
    ));
    let health = source.health();
    assert!(!health.running);
    assert!(
        health
            .last_error
            .is_some_and(|e| e.contains("no permission"))
    );
}

#[test]
fn starting_twice_is_refused_and_a_stopped_source_restarts() {
    let dev = Device::default();
    let mut source = dev.source(2, None);
    source.start().unwrap();
    assert!(matches!(source.start(), Err(EndpointError::Internal(_))));
    source.stop();
    source.start().expect("restart after stop");
    assert_eq!(*dev.opens.lock(), 2);
    dev.callback(2, 320, 0.5);
    let mut out = Vec::new();
    source.read(&mut out, MS(10)).unwrap();
    assert_eq!(out[0].seq(), 0, "sequence restarts with the new capture");
}

#[test]
fn a_vanished_device_is_reported_once_audio_is_drained_and_stops_the_source() {
    let dev = Device::default();
    let mut source = dev.source(1, None);
    source.start().unwrap();
    dev.callback(1, 320, 0.5);
    dev.lost
        .lock()
        .as_ref()
        .unwrap()
        .store(true, Ordering::Release);
    let mut out = Vec::new();
    source
        .read(&mut out, MS(10))
        .expect("audio captured before the loss is delivered");
    assert_eq!(out.len(), 1);
    assert_eq!(
        source.read(&mut out, MS(10)),
        Err(EndpointError::DeviceLost {
            device: "test mic".to_owned()
        })
    );
    let health = source.health();
    assert!(!health.running);
    assert!(health.last_error.is_some_and(|e| e.contains("test mic")));
}

#[test]
fn an_overrun_is_counted_in_health_and_marked_on_a_frame() {
    let dev = Device::default();
    let mut source = dev.source(1, None);
    source.start().unwrap();
    // The ring holds `RING_SLOTS` callbacks; one more does not fit.
    for _ in 0..=RING_SLOTS {
        dev.callback(1, 320, 0.5);
    }
    let mut out = Vec::new();
    source.read(&mut out, MS(1)).unwrap();
    assert_eq!(out.len(), RING_SLOTS);
    assert_eq!(source.health().dropped_samples, 320);
    assert!(out[0].discontinuity());
}

/// A device thread writes 4,000 callbacks of 512 stereo frames (over 40 s of
/// audio) while this thread reads. Every mono sample is a distinct counter, so
/// order and accounting show that no audio was repeated, reordered or lost
/// without being counted.
#[test]
fn a_device_thread_and_a_reader_thread_keep_order_and_account_for_every_sample() {
    const CALLBACKS: usize = 4_000;
    const FRAMES: usize = 512;
    let dev = Device::default();
    let mut source = dev.source(2, None);
    source.start().unwrap();

    let producer = {
        let dev = dev.clone();
        std::thread::spawn(move || {
            let mut counter = 1u32;
            let mut buf = vec![0.0f32; FRAMES * 2];
            for _ in 0..CALLBACKS {
                for frame in buf.chunks_exact_mut(2) {
                    frame.fill(counter as f32);
                    counter += 1;
                }
                dev.writer.lock().as_mut().unwrap().write(&buf);
                if counter.is_multiple_of(16) {
                    std::thread::yield_now();
                }
            }
        })
    };

    let mut received = 0u64;
    let mut checksum = 0u64;
    let mut last = 0f32;
    let mut out = Vec::new();
    while !producer.is_finished() || !out.is_empty() {
        out.clear();
        source.read(&mut out, MS(2)).unwrap();
        for f in &out {
            for &s in f.samples() {
                assert!(s > last, "samples repeat or go backwards: {s} after {last}");
                last = s;
                received += 1;
                checksum += s as u64;
            }
        }
        if producer.is_finished() && out.is_empty() {
            break;
        }
    }
    producer.join().unwrap();
    out.clear();
    source.read(&mut out, MS(1)).unwrap();
    for f in &out {
        received += f.len() as u64;
        checksum += f.samples().iter().map(|&s| s as u64).sum::<u64>();
    }

    let total = (CALLBACKS * FRAMES) as u64;
    let dropped_frames = source.health().dropped_samples / 2;
    let unread = total - received - dropped_frames;
    assert!(
        unread < 960,
        "{unread} samples are neither read nor counted as dropped"
    );
    if dropped_frames == 0 {
        let upto = total - unread;
        assert_eq!(checksum, upto * (upto + 1) / 2);
    }
}
