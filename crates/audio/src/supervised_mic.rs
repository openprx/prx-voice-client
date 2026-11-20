//! The physical microphone of a call as one `AudioSource`: the input policy chooses the device and moves capture when the device list changes.
//!
//! It wraps [`InputSupervisor`] (which never selects our virtual microphone)
//! so that a pump can read it like any other source. Device changes are
//! reported as pump events instead of ending the source: when the policy finds
//! no microphone the source stays open and delivers nothing until one appears.

use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use prxvc_media::{AudioFrame, SampleRate};
use prxvc_ports::{AudioSource, EndpointError, EndpointHealth, SourceInfo};
use uuid::Uuid;

use crate::call_pump::{EventQueue, PumpEvent};
use crate::input_policy::{InputDevice, SelectionReason};
use crate::input_supervisor::{AudioLoopEvent, InputBackend, InputSupervisor};

/// How long [`read`](AudioSource::read) sleeps between looks at the capture.
const IDLE_POLL: Duration = Duration::from_millis(2);

/// Most frames one `read` hands over, so that a source that never runs dry
/// cannot keep the caller inside one call.
const MAX_FRAMES_PER_READ: usize = 64;

/// Rate reported by `start` before the first frame; frames carry their own.
const NOMINAL_RATE: SampleRate = SampleRate::HZ_48000;

/// The call's microphone, chosen by the input policy.
pub struct SupervisedMic<B: InputBackend + Send> {
    supervisor: InputSupervisor<B>,
    changes: Receiver<AudioLoopEvent>,
    events: EventQueue,
    device: Option<InputDevice>,
    frames: u64,
    started: bool,
}

impl<B: InputBackend + Send> SupervisedMic<B> {
    /// A microphone source. `preference` is the configured device (UID or
    /// name; empty follows the policy). Device changes are pushed to `events`.
    pub fn new(backend: B, preference: &str, events: EventQueue) -> Self {
        let (tx, changes) = channel();
        Self {
            supervisor: InputSupervisor::new(backend, Uuid::nil(), preference, tx),
            changes,
            events,
            device: None,
            frames: 0,
            started: false,
        }
    }

    /// The device capture is on now.
    pub fn device(&self) -> Option<&InputDevice> {
        self.device.as_ref()
    }

    fn relay_changes(&mut self) {
        while let Ok(change) = self.changes.try_recv() {
            match change {
                AudioLoopEvent::InputOpened {
                    device,
                    reason,
                    previous,
                } => {
                    // Starting is reported by `start` itself; later moves are events.
                    if previous.is_some() || self.started {
                        self.events.push(PumpEvent::DeviceChanged {
                            device: device.name.clone(),
                        });
                    }
                    log_reason(&device, reason);
                    self.device = Some(device);
                }
                AudioLoopEvent::InputUnavailable { message } => {
                    self.device = None;
                    self.events.push(PumpEvent::DeviceUnavailable { message });
                }
            }
        }
    }
}

fn log_reason(device: &InputDevice, reason: SelectionReason) {
    tracing::info!(device = %device.name, uid = %device.uid, ?reason, "call microphone chosen");
}

impl<B: InputBackend + Send> AudioSource for SupervisedMic<B> {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        self.supervisor
            .start(Instant::now())
            .map_err(|e| EndpointError::Unavailable {
                reason: e.to_string(),
            })?;
        self.relay_changes();
        self.started = true;
        let device = self
            .device
            .as_ref()
            .map_or_else(String::new, |d| d.name.clone());
        Ok(SourceInfo {
            rate: NOMINAL_RATE,
            device,
        })
    }

    fn read(&mut self, out: &mut Vec<AudioFrame>, max_wait: Duration) -> Result<(), EndpointError> {
        if !self.started {
            return Err(EndpointError::NotStarted);
        }
        let deadline = Instant::now() + max_wait;
        loop {
            self.supervisor.tick(Instant::now());
            self.relay_changes();
            let before = out.len();
            while out.len() - before < MAX_FRAMES_PER_READ
                && let Some(frame) = self.supervisor.read_frame()
            {
                out.push(frame);
            }
            self.frames += (out.len() - before) as u64;
            let now = Instant::now();
            if out.len() > before || now >= deadline {
                return Ok(());
            }
            std::thread::sleep(IDLE_POLL.min(deadline - now));
        }
    }

    fn health(&self) -> EndpointHealth {
        EndpointHealth {
            running: self.started,
            frames: self.frames,
            ..EndpointHealth::default()
        }
    }

    fn stop(&mut self) {
        self.supervisor.stop();
        self.started = false;
    }
}

#[cfg(test)]
mod tests;
