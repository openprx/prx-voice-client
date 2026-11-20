//! A WAV file as a microphone: frames at the pace of real time, with silence before and after the speech.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use prxvc_adapters::tts::qwentts::load_reference_wav;
use prxvc_media::{AudioFrame, Clock, Lang, MonoTime, SampleRate, SystemClock};
use prxvc_ports::{AudioSource, EndpointError, EndpointHealth, SourceInfo};

/// Length of a delivered frame.
const FRAME: Duration = Duration::from_millis(20);

/// A decoded recording.
#[derive(Clone)]
pub(crate) struct Recording {
    pub samples: Vec<f32>,
    pub rate: SampleRate,
}

impl Recording {
    /// Read a WAV file (mono or first channel, any rate the reader supports).
    pub(crate) fn read(path: &Path) -> Result<Self, String> {
        let r = load_reference_wav(path, "harness", Lang::En, None)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            samples: r.samples,
            rate: r.rate,
        })
    }

    pub(crate) fn seconds(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.rate.hz())
    }
}

/// Delivers `lead` of silence, the recording, then `tail` of silence, each 20 ms frame no earlier
/// than its time in real time. After the tail it delivers nothing.
pub(crate) struct PacedFileSource {
    samples: Vec<f32>,
    rate: SampleRate,
    clock: Arc<SystemClock>,
    started: Option<(Instant, MonoTime)>,
    next: usize,
    per_frame: usize,
    lead: usize,
    total: usize,
    /// Where the pump learns when the feeding began (pipeline clock).
    pub(crate) began: Arc<parking_lot::Mutex<Option<MonoTime>>>,
    pub(crate) finished: Arc<std::sync::atomic::AtomicBool>,
}

impl PacedFileSource {
    pub(crate) fn new(
        rec: &Recording,
        lead: Duration,
        tail: Duration,
        clock: Arc<SystemClock>,
    ) -> Self {
        let rate = rec.rate;
        let (lead_n, tail_n) = (rate.samples_in(lead), rate.samples_in(tail));
        let mut samples = vec![0.0; lead_n];
        samples.extend_from_slice(&rec.samples);
        samples.extend(std::iter::repeat_n(0.0, tail_n));
        let total = samples.len();
        Self {
            samples,
            rate,
            clock,
            started: None,
            next: 0,
            per_frame: rate.samples_in(FRAME).max(1),
            lead: lead_n,
            total,
            began: Arc::default(),
            finished: Arc::default(),
        }
    }

    /// Samples of lead silence.
    pub(crate) fn lead_seconds(&self) -> f64 {
        self.lead as f64 / f64::from(self.rate.hz())
    }
}

impl AudioSource for PacedFileSource {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        let now = self.clock.now();
        self.started = Some((Instant::now(), now));
        *self.began.lock() = Some(now);
        Ok(SourceInfo {
            rate: self.rate,
            device: "wav file".into(),
        })
    }

    fn read(&mut self, out: &mut Vec<AudioFrame>, max_wait: Duration) -> Result<(), EndpointError> {
        let Some((began, _)) = self.started else {
            return Err(EndpointError::NotStarted);
        };
        if self.next >= self.total {
            self.finished
                .store(true, std::sync::atomic::Ordering::Release);
            std::thread::sleep(max_wait.min(Duration::from_millis(5)));
            return Ok(());
        }
        let due_at = |index: usize| FRAME * u32::try_from(index).unwrap_or(u32::MAX);
        let frame_index = self.next / self.per_frame;
        let wait = due_at(frame_index).saturating_sub(began.elapsed());
        if wait > max_wait {
            std::thread::sleep(max_wait);
            return Ok(());
        }
        std::thread::sleep(wait);
        while self.next < self.total && due_at(self.next / self.per_frame) <= began.elapsed() {
            let end = (self.next + self.per_frame).min(self.total);
            let ts = MonoTime::from_nanos(
                self.started.map_or(0, |(_, t)| t.as_nanos())
                    + u64::try_from(due_at(self.next / self.per_frame).as_nanos()).unwrap_or(0),
            );
            let frame = AudioFrame::new(
                self.samples[self.next..end].to_vec(),
                self.rate,
                ts,
                (self.next / self.per_frame) as u64,
            )
            .map_err(|e| EndpointError::Internal(e.to_string()))?;
            out.push(frame);
            self.next = end;
        }
        Ok(())
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth {
            running: self.started.is_some(),
            frames: (self.next / self.per_frame) as u64,
            ..EndpointHealth::default()
        }
    }

    fn stop(&mut self) {
        self.started = None;
    }
}
