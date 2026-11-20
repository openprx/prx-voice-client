//! Loopback test of the outgoing call path: a file stands in for the microphone, the real pump, mixer and `VirtualMicSink` carry it into the driver, and the virtual microphone is captured by UID.
//!
//! Needs the installed driver and no call application. It never opens the real
//! microphone and never touches the system default input or output: the source
//! is a generated WAV file read back through a file-backed `AudioSource`, the
//! sink is the driver's hidden sink, and the capture opens the virtual
//! microphone only. It is silent. Ignored by default; run with
//! `cargo test -p prxvc-audio --test live_call_audio_loop -- --ignored --nocapture`.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use prxvc_audio::call_pump::{EventQueue, Pump, PumpConfig, PumpRoute, PumpSpec};
use prxvc_audio::cpal_input::open_with_clock;
use prxvc_audio::driver_status::DriverStatus;
use prxvc_audio::input_devices::list_input_devices;
use prxvc_audio::input_policy::OWN_MIC_UID;
use prxvc_audio::mix::{MixMode, Mixer, MixerConfig};
use prxvc_audio::virtual_mic::{VirtualMicOptions, VirtualMicSink};
use prxvc_media::{AudioFrame, Clock, MonoTime, SampleRate, SystemClock};
use prxvc_ports::{AudioSource, EndpointError, EndpointHealth, SourceInfo};

const RATE: u32 = 48_000;
const TONE_HZ: f32 = 440.0;
const AMPLITUDE: f32 = 0.5;
const FRAME: usize = 480; // 10 ms

/// Write a 16-bit mono PCM WAV of a tone.
fn write_tone_wav(path: &Path, seconds: f32) {
    let samples: Vec<i16> = (0..(RATE as f32 * seconds) as usize)
        .map(|i| {
            let x = AMPLITUDE * (2.0 * std::f32::consts::PI * TONE_HZ * i as f32 / RATE as f32).sin();
            (x * f32::from(i16::MAX)) as i16
        })
        .collect();
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in &samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write the wav file");
}

/// Read the samples of a 16-bit mono PCM WAV written by [`write_tone_wav`].
fn read_wav(path: &Path) -> Vec<f32> {
    let bytes = std::fs::read(path).expect("read the wav file");
    assert_eq!(&bytes[..4], b"RIFF");
    bytes[44..]
        .chunks_exact(2)
        .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / f32::from(i16::MAX))
        .collect()
}

/// A file-backed stand-in for the microphone: frames of 10 ms handed out in real time.
struct FileSource {
    samples: Vec<f32>,
    clock: Arc<SystemClock>,
    began: Option<Instant>,
    next: usize,
    first_frame_at: Arc<parking_lot::Mutex<Option<Instant>>>,
}

impl AudioSource for FileSource {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        self.began = Some(Instant::now());
        Ok(SourceInfo {
            rate: SampleRate::HZ_48000,
            device: "wav file".to_owned(),
        })
    }

    fn read(&mut self, out: &mut Vec<AudioFrame>, max_wait: Duration) -> Result<(), EndpointError> {
        let began = self.began.ok_or(EndpointError::NotStarted)?;
        let deadline = Instant::now() + max_wait;
        loop {
            let due = (began.elapsed().as_secs_f64() * f64::from(RATE)) as usize / FRAME;
            let mut pushed = false;
            while self.next / FRAME <= due && self.next < self.samples.len() {
                let end = (self.next + FRAME).min(self.samples.len());
                let frame = AudioFrame::new(
                    self.samples[self.next..end].to_vec(),
                    SampleRate::HZ_48000,
                    self.clock.now(),
                    (self.next / FRAME) as u64,
                )
                .expect("frame");
                self.first_frame_at.lock().get_or_insert_with(Instant::now);
                out.push(frame);
                self.next = end;
                pushed = true;
            }
            if pushed || Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth {
            running: self.began.is_some(),
            frames: (self.next / FRAME) as u64,
            ..EndpointHealth::default()
        }
    }

    fn stop(&mut self) {
        self.began = None;
    }
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
        .expect("range is not empty")
}

#[test]
#[ignore = "needs the PRX Voice Microphone driver installed and loaded"]
fn live_call_audio_loop() {
    let status = DriverStatus::detect();
    println!("driver status: {status:#?}");
    status.check(None).expect("driver installed, loaded and current");

    // The stand-in microphone: a tone in a WAV file, read back from disk.
    let dir = tempfile::tempdir().expect("temp dir");
    let wav = dir.path().join("tone.wav");
    write_tone_wav(&wav, 1.0);
    let samples = read_wav(&wav);
    println!("source file: {} ({} samples at {RATE} Hz)", wav.display(), samples.len());

    // Capture from the virtual microphone, opened by UID (never the default input).
    let mic = list_input_devices()
        .expect("device list")
        .into_iter()
        .find(|d| d.uid == OWN_MIC_UID)
        .expect("the virtual microphone is listed");
    let clock = Arc::new(SystemClock::new());
    let mut capture = open_with_clock(&mic, clock.clone()).expect("open capture");
    let info = capture.start().expect("start capture");
    println!("capture: {} at {} Hz", info.device, info.rate.hz());
    let mut frames: Vec<AudioFrame> = Vec::new();
    capture.read(&mut frames, Duration::from_millis(300)).expect("settle read");
    frames.clear();

    // The outgoing path: file source, real mixer (original lane), real virtual microphone sink.
    let sink = VirtualMicSink::open(&VirtualMicOptions::default()).expect("open the sink");
    let first_frame_at = Arc::new(parking_lot::Mutex::new(None));
    let source = FileSource {
        samples,
        clock: clock.clone(),
        began: None,
        next: 0,
        first_frame_at: Arc::clone(&first_frame_at),
    };
    let mixer = Box::new(Mixer::new(MixerConfig {
        mode: MixMode::Original,
        ..MixerConfig::default()
    }));
    let mut pump = Pump::start(PumpSpec {
        config: PumpConfig::new("outgoing"),
        source: Box::new(source),
        route: PumpRoute::Mix {
            mixer,
            sink: Box::new(sink),
        },
        events: EventQueue::new(),
    })
    .expect("start the pump");
    let started_at = Instant::now();

    // Collect 2.5 s of capture, each frame with the time it was read.
    let mut got: Vec<(Instant, MonoTime, Vec<f32>)> = Vec::new();
    while started_at.elapsed() < Duration::from_millis(2_500) {
        frames.clear();
        capture.read(&mut frames, Duration::from_millis(50)).expect("read");
        let read_at = Instant::now();
        got.extend(frames.drain(..).map(|f| (read_at, f.ts(), f.samples().to_vec())));
    }
    let stats = pump.stats();
    pump.stop(Duration::from_secs(2)).expect("the pump stops");
    capture.stop();
    println!("pump stats: {stats:#?}");

    // Onset: the first captured sample above a fraction of the tone amplitude, dated by
    // arrival (read time minus the samples that follow it in its frame).
    let threshold = AMPLITUDE * 0.2;
    let rate = info.rate.hz() as usize;
    let mut flat: Vec<f32> = Vec::new();
    let mut onset = None;
    let mut arrival = None;
    for (read_at, _ts, data) in &got {
        if onset.is_none()
            && let Some(pos) = data.iter().position(|s| s.abs() > threshold)
        {
            let after = info.rate.duration_of(data.len() - pos);
            arrival = Some(*read_at - after);
            onset = Some(flat.len() + pos);
        }
        flat.extend_from_slice(data);
    }
    let start = onset.expect("the tone came out of the virtual microphone");
    let first = first_frame_at.lock().expect("the source delivered frames");
    let latency = arrival.expect("arrival").saturating_duration_since(first);
    println!("captured {} samples at {rate} Hz", flat.len());
    println!("end-to-end latency, file source's first frame to first tone sample out of the microphone: {latency:?}");
    assert!(latency < Duration::from_millis(500), "latency {latency:?}");

    assert!(flat.len() >= start + rate * 3 / 4, "capture shorter than the tone");
    let middle = &flat[start + rate / 10..start + rate * 7 / 10];
    let hz = dominant_hz(middle);
    let rms = (middle.iter().map(|s| s * s).sum::<f32>() / middle.len() as f32).sqrt();
    let expected_rms = AMPLITUDE / 2f32.sqrt();
    println!("dominant frequency: {hz} Hz (expected {TONE_HZ}), RMS {rms:.3} (expected {expected_rms:.3})");
    assert!((hz - TONE_HZ).abs() <= 2.0, "dominant frequency {hz} Hz, expected {TONE_HZ}");
    assert!(rms > expected_rms * 0.7 && rms < expected_rms * 1.3, "tone level {rms}");

    // The pump saw the same signal and dropped nothing.
    assert_eq!(stats.sink_dropped_samples, 0, "the sink dropped samples");
    assert!(stats.frames >= 90, "frames read: {}", stats.frames);
    assert!(stats.level.peak > AMPLITUDE * 0.8, "metered peak {}", stats.level.peak);
}
