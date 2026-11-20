//! Playback endpoint: implements the `AudioSink` port over the converter and ring of `output`, with the device opened by a pluggable function.
//!
//! The sink owns the non-real-time half of playback: it converts and
//! resamples frames, queues them in the [`PlaybackBuffer`] and keeps the
//! counters. The device callback only drains the ring (`cpal_output_rt.rs`).
//! The device itself is opened by the function the sink is built with, so the
//! same endpoint serves the default output, a named device and the virtual
//! microphone, and tests run it with no hardware.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use prxvc_media::{AudioFrame, SampleRate};
use prxvc_ports::{AudioSink, EndpointError, EndpointHealth, SinkInfo, WriteOutcome};

use crate::AudioError;
use crate::cpal_stream::{OpenedStream, StreamHandle};
use crate::output::{OutputCore, OutputStats, Queued};
use crate::playback_buffer::PlaybackBuffer;
use crate::stream_sink::StreamSink;

/// Opens the device stream: given the ring to drain and the run flag the
/// callback observes, starts playing and returns the stream.
pub type SinkOpener = Box<
    dyn FnMut(&Arc<PlaybackBuffer>, &Arc<AtomicBool>) -> Result<OpenedStream, EndpointError> + Send,
>;

/// An [`AudioSink`] that plays through a ring drained by a device callback.
pub struct PlaybackSink {
    core: OutputCore,
    rate: SampleRate,
    opener: SinkOpener,
    stream: Option<StreamHandle>,
    running: Arc<AtomicBool>,
    started: bool,
    frames: u64,
    /// Interleaved device samples of frames that could not be converted.
    lost_in_conversion: u64,
    last_error: Option<String>,
}

impl PlaybackSink {
    /// A sink for a device with the given format, buffering at most
    /// `buffer_seconds` of audio, opening the device with `opener`.
    ///
    /// # Errors
    /// The device format is zero.
    pub fn new(
        device_rate: u32,
        device_channels: u16,
        buffer_seconds: u32,
        opener: SinkOpener,
    ) -> Result<Self, AudioError> {
        let core = OutputCore::new(device_rate, device_channels, buffer_seconds)?;
        let rate = SampleRate::new(device_rate)
            .map_err(|e| AudioError::InitializationFailed(e.to_string()))?;
        Ok(Self {
            core,
            rate,
            opener,
            stream: None,
            running: Arc::new(AtomicBool::new(false)),
            started: false,
            frames: 0,
            lost_in_conversion: 0,
            last_error: None,
        })
    }

    /// The sample rate negotiated with the output device.
    pub fn sample_rate(&self) -> u32 {
        self.core.device_rate()
    }

    /// The channel count negotiated with the output device.
    pub fn channels(&self) -> u16 {
        self.core.device_channels()
    }

    /// Shared handle to the playback queue.
    pub fn buffer(&self) -> &Arc<PlaybackBuffer> {
        self.core.buffer()
    }

    fn fail(&mut self, error: &EndpointError) {
        self.last_error = Some(error.to_string());
    }
}

impl AudioSink for PlaybackSink {
    fn start(&mut self) -> Result<SinkInfo, EndpointError> {
        if self.started {
            let e = EndpointError::Internal("sink is already started".to_owned());
            self.fail(&e);
            return Err(e);
        }
        self.running.store(true, Ordering::Release);
        match (self.opener)(self.core.buffer(), &self.running) {
            Ok(opened) => {
                self.stream = Some(opened.handle);
                self.started = true;
                self.last_error = None;
                Ok(SinkInfo {
                    rate: self.rate,
                    device: opened.device,
                })
            }
            Err(e) => {
                self.running.store(false, Ordering::Release);
                self.fail(&e);
                Err(e)
            }
        }
    }

    /// Frames of any rate are accepted and converted to the device rate; the
    /// outcome counts the frame's own samples, split in the proportion the
    /// queue accepted of the converted audio.
    fn write(&mut self, frame: &AudioFrame) -> WriteOutcome {
        let len = frame.len();
        if !self.started {
            self.fail(&EndpointError::NotStarted);
            self.lost_in_conversion += len as u64;
            return WriteOutcome {
                accepted: 0,
                dropped: len,
            };
        }
        match self.core.write(frame) {
            Ok(Queued { produced, accepted }) => {
                self.frames += 1;
                if produced == 0 || accepted == produced {
                    WriteOutcome {
                        accepted: len,
                        dropped: 0,
                    }
                } else {
                    let lost = produced - accepted;
                    let dropped = (len as u128 * lost as u128 / produced as u128) as usize;
                    WriteOutcome {
                        accepted: len - dropped,
                        dropped,
                    }
                }
            }
            Err(e) => {
                self.fail(&EndpointError::Internal(e.to_string()));
                self.lost_in_conversion += len as u64;
                WriteOutcome {
                    accepted: 0,
                    dropped: len,
                }
            }
        }
    }

    fn flush(&mut self) {
        self.core.flush();
    }

    fn queued(&self) -> Duration {
        let channels = usize::from(self.core.device_channels());
        self.rate
            .duration_of(self.core.buffer().buffered_samples() / channels)
    }

    fn health(&self) -> EndpointHealth {
        let stats = self.core.buffer().stats();
        EndpointHealth {
            running: self.started,
            frames: self.frames,
            dropped_samples: stats.dropped_samples + self.lost_in_conversion,
            underruns: stats.underruns,
            last_error: self.last_error.clone(),
        }
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        // Dropping the stream stops the device.
        self.stream = None;
        self.core.flush();
        self.started = false;
    }
}

impl StreamSink for PlaybackSink {
    fn end_stream(&mut self) {
        if let Err(e) = self.core.end_stream() {
            tracing::warn!("resampler tail lost at end of stream: {e}");
        }
    }

    fn is_drained(&self) -> bool {
        self.core.buffer().is_drained()
    }

    fn wait_drained(&self, timeout: Duration) -> bool {
        self.core.buffer().wait_drained(timeout)
    }

    fn stats(&self) -> OutputStats {
        self.core.buffer().stats()
    }
}

impl Drop for PlaybackSink {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
    }
}

#[cfg(test)]
impl PlaybackSink {
    /// A started sink with a device format and no device or stream behind
    /// it: the test plays the part of the device callback by pulling from
    /// [`buffer`](Self::buffer).
    pub(crate) fn detached(device_rate: u32, device_channels: u16) -> Self {
        let mut sink = Self::new(
            device_rate,
            device_channels,
            crate::output::DEFAULT_BUFFER_SECONDS,
            Box::new(|_, _| {
                Ok(OpenedStream {
                    handle: Box::new(()),
                    device: "detached".to_owned(),
                })
            }),
        )
        .expect("valid device format");
        sink.start().expect("detached sink starts");
        sink
    }
}

#[cfg(test)]
mod tests;
