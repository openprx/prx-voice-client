//! Where the translated speech goes in the harness: a WAV file, or the virtual microphone with a recorder listening to it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use prxvc_audio::cpal_input::open_with_clock;
use prxvc_audio::input_devices::list_input_devices;
use prxvc_audio::input_policy::OWN_MIC_UID;
use prxvc_media::{AudioFrame, SampleRate, SystemClock};
use prxvc_ports::{AudioSink, AudioSource, EndpointError, EndpointHealth, SinkInfo, WriteOutcome};

/// Write mono 16-bit PCM.
pub(crate) fn write_wav(path: &Path, samples: &[f32], rate: SampleRate) -> Result<(), String> {
    let data_len = u32::try_from(samples.len() * 2).map_err(|_| "recording too long")?;
    let mut v = Vec::with_capacity(44 + samples.len() * 2);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&rate.hz().to_le_bytes());
    v.extend_from_slice(&(rate.hz() * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(path, v).map_err(|e| format!("{}: {e}", path.display()))
}

/// Everything written to a [`WavSink`], readable by the harness after the pump ended.
pub(crate) type Captured = Arc<Mutex<Vec<f32>>>;

/// A sink that keeps the samples and drains at the pace of real time, like a device.
pub(crate) struct WavSink {
    rate: SampleRate,
    started: Instant,
    written: Duration,
    out: Captured,
}

impl WavSink {
    pub(crate) fn new(rate: SampleRate) -> (Self, Captured) {
        let out: Captured = Arc::default();
        (
            Self {
                rate,
                started: Instant::now(),
                written: Duration::ZERO,
                out: Arc::clone(&out),
            },
            out,
        )
    }
}

impl AudioSink for WavSink {
    fn start(&mut self) -> Result<SinkInfo, EndpointError> {
        self.started = Instant::now();
        Ok(SinkInfo {
            rate: self.rate,
            device: "wav file".into(),
        })
    }

    fn write(&mut self, frame: &AudioFrame) -> WriteOutcome {
        self.out.lock().extend_from_slice(frame.samples());
        self.written += frame.duration();
        WriteOutcome {
            accepted: frame.len(),
            dropped: 0,
        }
    }

    fn flush(&mut self) {}

    fn queued(&self) -> Duration {
        self.written.saturating_sub(self.started.elapsed())
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth {
            running: true,
            ..EndpointHealth::default()
        }
    }

    fn stop(&mut self) {}
}

/// Records the virtual microphone (by UID, never a system default) on a thread of its own.
pub(crate) struct Recorder {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Vec<f32>>>,
    rate: SampleRate,
}

impl Recorder {
    /// Start capturing the virtual microphone.
    pub(crate) fn start() -> Result<Self, String> {
        let mic = list_input_devices()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|d| d.uid == OWN_MIC_UID)
            .ok_or("the virtual microphone is not listed (driver missing or not loaded)")?;
        let clock = Arc::new(SystemClock::new());
        let mut source = open_with_clock(&mic, clock).map_err(|e| e.to_string())?;
        let info = source.start().map_err(|e| e.to_string())?;
        let rate = info.rate;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("harness-recorder".into())
            .spawn(move || {
                let mut all = Vec::new();
                let mut frames = Vec::new();
                while !flag.load(Ordering::Acquire) {
                    frames.clear();
                    if source.read(&mut frames, Duration::from_millis(50)).is_err() {
                        break;
                    }
                    for f in &frames {
                        all.extend_from_slice(f.samples());
                    }
                }
                source.stop();
                all
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            stop,
            thread: Some(thread),
            rate,
        })
    }

    /// Stop and return what was recorded and its rate.
    pub(crate) fn finish(mut self) -> (Vec<f32>, SampleRate) {
        self.stop.store(true, Ordering::Release);
        let samples = self
            .thread
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or_default();
        (samples, self.rate)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Where an output file of `id` goes.
pub(crate) fn path_of(dir: &Path, id: &str, suffix: &str) -> PathBuf {
    dir.join(format!("{}{suffix}.wav", id.replace('/', "_")))
}
