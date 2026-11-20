//! Loopback test of the installed virtual microphone driver: a tone written into the hidden sink through the endpoint comes out of the microphone at the right pitch.
//!
//! Needs the driver installed and loaded (`DriverStatus::detect` is reported
//! first). Ignored by default; run with
//! `cargo test -p prxvc-audio --test live_virtual_mic -- --ignored --nocapture`.
//! It is silent: the sink makes no sound, and capture opens the virtual
//! microphone only, never a real one.

#![cfg(target_os = "macos")]

use std::sync::Arc;
use std::time::Duration;

use prxvc_audio::cpal_input::open_with_clock;
use prxvc_audio::driver_status::DriverStatus;
use prxvc_audio::input_devices::list_input_devices;
use prxvc_audio::input_policy::OWN_MIC_UID;
use prxvc_audio::stream_sink::StreamSink;
use prxvc_audio::virtual_mic::{VirtualMicOptions, VirtualMicSink};
use prxvc_media::{AudioFrame, Clock, MonoTime, SampleRate, SystemClock};
use prxvc_ports::{AudioSink, AudioSource};

const RATE: u32 = 48_000;
const TONE_HZ: f32 = 440.0;
const AMPLITUDE: f32 = 0.5;

fn tone(seconds: f32) -> Vec<f32> {
    (0..(RATE as f32 * seconds) as usize)
        .map(|i| AMPLITUDE * (2.0 * std::f32::consts::PI * TONE_HZ * i as f32 / RATE as f32).sin())
        .collect()
}

/// Magnitude of `samples` at `hz` (Goertzel).
fn power_at(samples: &[f32], hz: f32) -> f32 {
    let w = 2.0 * std::f32::consts::PI * hz / RATE as f32;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for &x in samples {
        let s0 = x + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0).sqrt() / samples.len() as f32
}

fn dominant_hz(samples: &[f32]) -> f32 {
    (100..4_000)
        .map(|hz| hz as f32)
        .max_by(|a, b| power_at(samples, *a).total_cmp(&power_at(samples, *b)))
        .unwrap()
}

#[test]
#[ignore = "needs the PRX Voice Microphone driver installed and loaded"]
fn live_virtual_mic_loopback() {
    let status = DriverStatus::detect();
    println!("driver status: {status:#?}");
    status
        .check(None)
        .expect("driver installed, loaded and current");

    // Capture from the virtual microphone, opened by UID.
    let mic = list_input_devices()
        .expect("device list")
        .into_iter()
        .find(|d| d.uid == OWN_MIC_UID)
        .expect("virtual microphone is listed");
    let clock = Arc::new(SystemClock::new());
    let mut source =
        open_with_clock(&mic, clock.clone()).expect("open capture on the virtual microphone");
    let info = source.start().expect("start capture");
    println!("capture: {} at {} Hz", info.device, info.rate.hz());

    // Sink: our driver's hidden sink, by UID.
    let mut sink = VirtualMicSink::open(&VirtualMicOptions::default())
        .expect("open the virtual microphone sink");
    let sink_info = sink.start().expect("start sink");
    println!(
        "sink: {} at {} Hz (uid {})",
        sink_info.device,
        sink_info.rate.hz(),
        sink.uid()
    );

    // Let the capture settle, discard whatever is there, then write the tone.
    let mut frames: Vec<AudioFrame> = Vec::new();
    source
        .read(&mut frames, Duration::from_millis(300))
        .expect("settle read");
    frames.clear();

    let samples = tone(1.0);
    let written_at = clock.now();
    for (i, chunk) in samples.chunks(960).enumerate() {
        let frame = AudioFrame::new(
            chunk.to_vec(),
            SampleRate::HZ_48000,
            MonoTime::ZERO,
            i as u64,
        )
        .unwrap();
        let outcome = sink.write(&frame);
        assert_eq!(outcome.dropped, 0, "sink dropped samples");
    }
    sink.end_stream();

    // Collect 2 s of capture: the tone, then silence. Each frame is kept with the
    // time at which it was read, because the capture stamps are back-dated by the
    // device buffer and can precede the write.
    let mut got: Vec<(MonoTime, MonoTime, Vec<f32>)> = Vec::new();
    let deadline = clock.now().saturating_add(Duration::from_millis(2_000));
    while clock.now() < deadline {
        frames.clear();
        source
            .read(&mut frames, Duration::from_millis(50))
            .expect("read");
        let read_at = clock.now();
        got.extend(
            frames
                .drain(..)
                .map(|f| (f.ts(), read_at, f.samples().to_vec())),
        );
    }
    assert!(
        sink.wait_drained(Duration::from_secs(2)),
        "sink did not drain"
    );
    source.stop();
    sink.stop();

    // Onset: the first captured sample above a fraction of the tone amplitude. Its
    // arrival time is the read time minus the samples that follow it in the frame
    // (they were captured after it); the stamp-based time is the frame's stamp plus
    // the sample's offset.
    let threshold = AMPLITUDE * 0.2;
    let rate = info.rate.hz() as usize;
    let mut flat: Vec<f32> = Vec::new();
    let mut onset_index = None;
    let (mut arrival, mut stamped) = (None, None);
    for (ts, read_at, data) in &got {
        if onset_index.is_none()
            && let Some(pos) = data.iter().position(|s| s.abs() > threshold)
        {
            let after = info.rate.duration_of(data.len() - pos);
            arrival = Some(read_at.saturating_since(written_at).saturating_sub(after));
            stamped = Some(
                ts.saturating_add(info.rate.duration_of(pos))
                    .saturating_since(written_at),
            );
            onset_index = Some(flat.len() + pos);
        }
        flat.extend_from_slice(data);
    }
    let start = onset_index.expect("the tone came out of the virtual microphone");
    let arrival = arrival.unwrap();
    println!("captured {} samples at {rate} Hz", flat.len());
    println!(
        "measured latency, first write to first tone sample out of the microphone: {arrival:?} by arrival \
         (read time minus the later samples of its frame), {:?} by capture stamp (back-dated, may read 0)",
        stamped.unwrap()
    );
    assert!(arrival < Duration::from_secs(1), "latency {arrival:?}");

    // Analyse the middle 0.6 s of the tone region.
    assert!(flat.len() >= start + rate, "capture shorter than the tone");
    let middle = &flat[start + rate / 5..start + rate * 4 / 5];
    let hz = dominant_hz(middle);
    let rms = (middle.iter().map(|s| s * s).sum::<f32>() / middle.len() as f32).sqrt();
    println!(
        "dominant frequency: {hz} Hz, RMS {rms:.3} (expected {TONE_HZ} Hz, RMS {:.3})",
        AMPLITUDE / 2f32.sqrt()
    );
    assert!(
        (hz - TONE_HZ).abs() <= 2.0,
        "dominant frequency {hz} Hz, expected {TONE_HZ}"
    );
    assert!(
        rms > AMPLITUDE / 2f32.sqrt() * 0.7,
        "tone level too low: {rms}"
    );
}
