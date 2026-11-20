//! Microphone capture supervisor: keeps one capture open on the device the input policy chooses and moves it when the device list changes.
//!
//! Runs on the audio-loop thread (not a real-time thread): it may allocate and
//! enumerate devices. It polls the device list about once a second, which
//! covers hot-plug, a changed system default and a vanished device alike, and
//! reports every change as an [`AudioLoopEvent`]. It never falls back to the
//! virtual microphone: when the policy finds nothing usable, capture stops and
//! an [`AudioLoopEvent::InputUnavailable`] is sent until a device appears.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::input_policy::{InputDevice, InputSelector, SelectionReason, SelectionUpdate};
use crate::pipeline::AudioInput;
use crate::{AudioError, AudioInputFrame};

/// How often the device list is re-read while capturing.
pub const DEVICE_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// A change in the microphone capture, for the application's event path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioLoopEvent {
    /// Capture is now running on `device`.
    InputOpened {
        /// The device in use.
        device: InputDevice,
        /// Why it was chosen.
        reason: SelectionReason,
        /// The device capture moved away from, if this is a switch.
        previous: Option<InputDevice>,
    },
    /// No microphone is being captured. Capture resumes when one appears.
    InputUnavailable {
        /// Human-readable cause.
        message: String,
    },
}

/// Lists devices and opens captures; replaced by a fake in tests.
pub trait InputBackend {
    /// The current input devices.
    ///
    /// # Errors
    /// A platform failure reading the device list.
    fn list(&mut self) -> Result<Vec<InputDevice>, AudioError>;
    /// Open (but do not start) a capture on `device`.
    ///
    /// # Errors
    /// The device cannot be opened.
    fn open(&mut self, session_id: Uuid, device: &InputDevice) -> Result<Box<dyn AudioInput>, AudioError>;
}

/// The real backend: Core Audio / cpal enumeration and cpal capture.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlatformInputBackend;

impl InputBackend for PlatformInputBackend {
    fn list(&mut self) -> Result<Vec<InputDevice>, AudioError> {
        crate::input_devices::list_input_devices()
    }

    fn open(&mut self, session_id: Uuid, device: &InputDevice) -> Result<Box<dyn AudioInput>, AudioError> {
        Ok(Box::new(crate::cpal_input::CpalAudioInput::open(session_id, device)?))
    }
}

/// Owns the current capture and the policy state.
pub struct InputSupervisor<B: InputBackend> {
    backend: B,
    selector: InputSelector,
    session_id: Uuid,
    events: Sender<AudioLoopEvent>,
    mic: Option<Box<dyn AudioInput>>,
    last_poll: Instant,
}

impl<B: InputBackend> InputSupervisor<B> {
    /// A supervisor with no capture open. `preference` is the configured
    /// device UID or name; empty follows the policy's default rules.
    pub fn new(backend: B, session_id: Uuid, preference: &str, events: Sender<AudioLoopEvent>) -> Self {
        Self {
            backend,
            selector: InputSelector::new(preference),
            session_id,
            events,
            mic: None,
            last_poll: Instant::now(),
        }
    }

    /// Choose a device and start capture. Called once at session start.
    ///
    /// # Errors
    /// The policy found no usable device ([`AudioError::InputPolicy`]) or the
    /// chosen device could not be opened or started.
    pub fn start(&mut self, now: Instant) -> Result<(), AudioError> {
        self.last_poll = now;
        let devices = self.backend.list()?;
        match self.selector.update(&devices) {
            SelectionUpdate::Changed { from, to } => {
                self.switch(from, to.device, to.reason).inspect_err(|_| self.selector.forget_current())
            }
            SelectionUpdate::Lost(e) => Err(AudioError::InputPolicy(e)),
            SelectionUpdate::Unchanged => Ok(()),
        }
    }

    /// Re-read the device list when the poll interval has elapsed and move the
    /// capture if the policy now chooses differently.
    ///
    /// Returns `true` when capture moved (or stopped), so the caller can reset
    /// anything fed by the old stream.
    pub fn tick(&mut self, now: Instant) -> bool {
        if now.duration_since(self.last_poll) < DEVICE_POLL_INTERVAL {
            return false;
        }
        self.last_poll = now;
        let devices = match self.backend.list() {
            Ok(d) => d,
            Err(e) => {
                // Keep the current capture; the next poll tries again.
                tracing::warn!(error = %e, "input device enumeration failed");
                return false;
            }
        };
        match self.selector.update(&devices) {
            SelectionUpdate::Unchanged => false,
            SelectionUpdate::Changed { from, to } => {
                if let Err(e) = self.switch(from, to.device, to.reason) {
                    self.selector.forget_current();
                    self.report_unavailable(e.to_string());
                }
                true
            }
            SelectionUpdate::Lost(e) => {
                self.close();
                self.report_unavailable(e.to_string());
                true
            }
        }
    }

    /// Next captured frame, if capture is running.
    pub fn read_frame(&mut self) -> Option<AudioInputFrame> {
        self.mic.as_mut()?.read_frame().ok().flatten()
    }

    /// Stop capture.
    pub fn stop(&mut self) {
        self.close();
    }

    fn close(&mut self) {
        let Some(mut mic) = self.mic.take() else { return };
        if let Err(e) = mic.stop() {
            tracing::warn!(error = %e, "stopping the microphone failed");
        }
    }

    fn switch(
        &mut self,
        previous: Option<InputDevice>,
        device: InputDevice,
        reason: SelectionReason,
    ) -> Result<(), AudioError> {
        self.close();
        let mut mic = self.backend.open(self.session_id, &device)?;
        mic.start()?;
        self.mic = Some(mic);
        self.send(AudioLoopEvent::InputOpened { device, reason, previous });
        Ok(())
    }

    fn report_unavailable(&self, message: String) {
        self.send(AudioLoopEvent::InputUnavailable { message });
    }

    fn send(&self, event: AudioLoopEvent) {
        // The receiver is gone only when the application is shutting down.
        let _ = self.events.send(event);
    }
}

#[cfg(test)]
mod tests;
