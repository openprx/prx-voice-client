//! Port conformance tests of the playback endpoint: the scenarios of the `AudioSink` fake in `prxvc-ports`, plus the output-path guarantees (exact length, flush, drain) with the device callback played by the test.

use prxvc_media::{MonoTime, SampleRate};

use super::*;

const MS: fn(u64) -> Duration = Duration::from_millis;

fn frame(rate: u32, samples: usize, seq: u64) -> AudioFrame {
    let data = (0..samples)
        .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.5)
        .collect();
    AudioFrame::new(data, SampleRate::new(rate).unwrap(), MonoTime::ZERO, seq).unwrap()
}

fn ms_frame(rate: u32, ms: u64, seq: u64) -> AudioFrame {
    frame(rate, SampleRate::new(rate).unwrap().samples_in(MS(ms)), seq)
}

fn failing(error: EndpointError) -> PlaybackSink {
    PlaybackSink::new(48_000, 2, 1, Box::new(move |_, _| Err(error.clone()))).unwrap()
}

#[test]
fn one_second_at_16k_and_24k_yields_48000_stereo_frames() {
    for rate in [16_000u32, 24_000] {
        let mut sink = PlaybackSink::detached(48_000, 2);
        sink.write(&frame(rate, rate as usize, 0));
        // The resampler holds back its tail until the stream ends.
        sink.end_stream();
        assert_eq!(sink.buffer().buffered_samples(), 48_000 * 2, "{rate} Hz");
    }
}

#[test]
fn flush_empties_the_queue_and_reports_drained() {
    let mut sink = PlaybackSink::detached(48_000, 2);
    sink.write(&frame(16_000, 16_000, 0));
    assert!(!sink.is_drained());
    sink.flush();
    assert_eq!(sink.buffer().buffered_samples(), 0);
    assert!(sink.is_drained());
    assert!(sink.wait_drained(MS(1)));
    assert_eq!(sink.queued(), Duration::ZERO);
}

#[test]
fn the_queue_drains_as_the_device_plays() {
    let mut sink = PlaybackSink::detached(48_000, 2);
    assert_eq!(
        sink.write(&ms_frame(48_000, 100, 0)),
        WriteOutcome {
            accepted: 4_800,
            dropped: 0
        }
    );
    assert_eq!(sink.queued(), MS(100));
    // The device callback takes 40 ms of stereo audio.
    let mut out = vec![0.0; 1_920 * 2];
    sink.buffer().pull(&mut out);
    assert_eq!(sink.queued(), MS(60));
    sink.write(&ms_frame(48_000, 100, 1));
    assert_eq!(sink.queued(), MS(160));
    let health = sink.health();
    assert_eq!(
        (health.running, health.frames, health.underruns),
        (true, 2, 0)
    );
}

#[test]
fn a_full_sink_drops_the_excess_and_counts_it() {
    // One second of mono at 16 kHz.
    let mut sink = PlaybackSink::new(
        16_000,
        1,
        1,
        Box::new(|_, _| {
            Ok(OpenedStream {
                handle: Box::new(()),
                device: "test".to_owned(),
            })
        }),
    )
    .unwrap();
    sink.start().unwrap();
    assert_eq!(sink.write(&ms_frame(16_000, 600, 0)).dropped, 0);
    let second = sink.write(&ms_frame(16_000, 600, 1));
    assert_eq!(
        second,
        WriteOutcome {
            accepted: 6_400,
            dropped: 3_200
        },
        "only 400 ms of room was left"
    );
    assert_eq!(sink.queued(), Duration::from_secs(1));
    let health = sink.health();
    assert_eq!(health.dropped_samples, 3_200);
    assert_eq!(StreamSink::stats(&sink).overruns, 1);
}

#[test]
fn running_dry_mid_stream_is_counted_as_an_underrun_in_health() {
    let mut sink = PlaybackSink::detached(16_000, 1);
    sink.write(&ms_frame(16_000, 10, 0));
    let mut out = vec![0.0; 320];
    sink.buffer().pull(&mut out);
    assert_eq!(sink.health().underruns, 1);

    sink.write(&ms_frame(16_000, 10, 1));
    sink.end_stream();
    sink.buffer().pull(&mut out);
    assert_eq!(
        sink.health().underruns,
        1,
        "an announced end is not an underrun"
    );
}

#[test]
fn writing_before_start_drops_everything_and_start_can_fail() {
    let mut sink = failing(EndpointError::Unavailable {
        reason: "driver missing".to_owned(),
    });
    assert_eq!(
        sink.write(&ms_frame(48_000, 20, 0)),
        WriteOutcome {
            accepted: 0,
            dropped: 960
        }
    );
    let health = sink.health();
    assert!(health.last_error.is_some());
    assert_eq!(health.dropped_samples, 960);
    assert!(matches!(
        sink.start(),
        Err(EndpointError::Unavailable { .. })
    ));
    let health = sink.health();
    assert!(!health.running);
    assert!(
        health
            .last_error
            .is_some_and(|e| e.contains("driver missing"))
    );
    assert_eq!(sink.buffer().buffered_samples(), 0);
    sink.stop();
}

#[test]
fn start_reports_the_device_rate_and_name_and_cannot_be_repeated() {
    let mut sink = PlaybackSink::detached(48_000, 2);
    assert!(matches!(sink.start(), Err(EndpointError::Internal(_))));
    sink.stop();
    let info = sink.start().expect("restart after stop");
    assert_eq!(
        info,
        SinkInfo {
            rate: SampleRate::HZ_48000,
            device: "detached".to_owned()
        }
    );
}

#[test]
fn stop_discards_the_queue_and_silences_the_callback_flag() {
    let mut sink = PlaybackSink::detached(48_000, 2);
    sink.write(&frame(16_000, 1_600, 0));
    let running = Arc::clone(&sink.running);
    assert!(running.load(Ordering::Acquire));
    sink.stop();
    assert!(!running.load(Ordering::Acquire));
    assert!(sink.is_drained());
    assert!(!sink.health().running);
}

#[test]
fn the_opener_receives_the_ring_and_run_flag_the_callback_uses() {
    let seen = Arc::new(parking_lot::Mutex::new(None));
    let s = Arc::clone(&seen);
    let mut sink = PlaybackSink::new(
        48_000,
        2,
        1,
        Box::new(move |buffer, running| {
            *s.lock() = Some((Arc::clone(buffer), Arc::clone(running)));
            Ok(OpenedStream {
                handle: Box::new(()),
                device: "x".to_owned(),
            })
        }),
    )
    .unwrap();
    sink.start().unwrap();
    let (buffer, running) = seen.lock().take().expect("opener ran");
    assert!(Arc::ptr_eq(&buffer, sink.buffer()));
    assert!(running.load(Ordering::Acquire));

    sink.write(&ms_frame(48_000, 10, 0));
    let mut out = vec![1.0; 960];
    crate::cpal_output_rt::fill_output(&running, &buffer, &mut out);
    assert!(
        out.iter().any(|&v| v != 0.0),
        "queued audio reaches the callback buffer"
    );
    running.store(false, Ordering::Release);
    crate::cpal_output_rt::fill_output(&running, &buffer, &mut out);
    assert!(
        out.iter().all(|&v| v == 0.0),
        "a stopped sink plays silence"
    );
}
