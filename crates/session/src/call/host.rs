//! What the call coordinator needs from the machine it runs on: checks, endpoint paths, routing and call detection, behind two small traits.
//!
//! The coordinator is layer L2 and names no concrete adapter (CS-1.2.3). The
//! composition root implements [`CallHost`] over the real endpoints and the
//! routing controller; tests implement it with scripted fakes that log every
//! call. Failures cross as [`HostFault`]s (a typed code plus English detail).
//!
//! Blocking: [`CallHost::ensure_microphone_consent`] and
//! [`CallHost::ensure_capture_consent`] may wait for the user to answer a
//! system prompt (the adapter bounds the wait). Everything else returns
//! within milliseconds.

use std::time::Duration;

use prxvc_media::MonoTime;
use prxvc_ports::BundleId;

use super::error::HostFault;
use crate::fault::ProviderRole;
use crate::output::OutputSource;

/// A reading of one path's counters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathStats {
    /// The path runs.
    pub running: bool,
    /// The source failed and the path ended by itself.
    pub failed: bool,
    /// The device captured from.
    pub source_device: String,
    /// The device played to, when the path plays anything.
    pub sink_device: Option<String>,
    /// Frames read from the source.
    pub frames: u64,
    /// Peak of the last full metering window, linear (1.0 is full scale).
    pub peak: f32,
    /// RMS of the last full metering window, linear.
    pub rms: f32,
    /// Capture stamp of the last frame.
    pub last_frame_ts: Option<MonoTime>,
    /// Samples lost on capture (overruns).
    pub overruns: u64,
    /// Samples refused by the sink or dropped by the mixer.
    pub drops: u64,
    /// Times the sink or the mixer ran dry.
    pub underruns: u64,
    /// Audio queued in the sink.
    pub queued: Duration,
}

/// Something a running path reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathEvent {
    /// The path ended: its endpoint is gone. The coordinator recovers or reports.
    Lost(HostFault),
    /// A problem that does not end the path (capture consent revoked, restart failed).
    Fault(HostFault),
    /// The tap delivers only silence while the application plays.
    NoSignal(String),
    /// Audio is back after [`PathEvent::NoSignal`].
    SignalRestored,
    /// The call application quit.
    TargetGone,
    /// The call application is back.
    TargetBack,
    /// The tap stopped delivering buffers.
    Stalled {
        /// Milliseconds without a buffer.
        idle_ms: u64,
    },
    /// The microphone in use changed.
    DeviceChanged(String),
    /// No microphone can be captured now.
    DeviceUnavailable(String),
}

/// One audio path of a running call (a pump with its endpoints).
pub trait AudioPath: Send {
    /// What the listener of this path should hear now; the path executes it
    /// (a crossfade) or ignores it when it plays nothing.
    fn set_listener_hears(&mut self, hears: OutputSource);

    /// Counters now.
    fn stats(&self) -> PathStats;

    /// Events since the last call, oldest first.
    fn take_events(&mut self) -> Vec<PathEvent>;

    /// Stop the path and release its endpoints, waiting a bounded time.
    ///
    /// # Errors
    /// A [`HostFault`] with `StopLeak` when the path did not end in time.
    fn stop(&mut self) -> Result<(), HostFault>;
}

/// What entering call mode did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnteredRouting {
    /// The default input before call mode; `None` when it was already ours.
    pub saved_input: Option<String>,
    /// The physical microphone the policy chose.
    pub physical_input: String,
}

/// What leaving call mode did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeftRouting {
    /// The device now the default input, when it was restored.
    pub restored_to: Option<String>,
    /// The default input was no longer ours and was left alone.
    pub left_alone: bool,
    /// The remembered device is gone and the policy's choice was used.
    pub previous_gone: bool,
    /// Call mode was not on.
    pub was_not_in_call_mode: bool,
}

/// A change of the system the routing controller reports while call mode is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoutingNotice {
    /// The default input is no longer ours.
    DefaultInputDisplaced {
        /// The default input now.
        to: String,
    },
    /// The user selected our microphone again.
    OwnMicrophoneReselected,
    /// What leaving call mode would restore changed.
    RestoreTargetChanged {
        /// The device that would be restored now.
        target: Option<String>,
    },
    /// Our virtual microphone is gone.
    VirtualMicrophoneLost,
    /// The default output changed.
    DefaultOutputChanged {
        /// The default output now.
        to: String,
    },
    /// The machine is going to sleep.
    SystemSleeping,
    /// The machine woke.
    SystemWoke {
        /// Our microphone is present and still the default input.
        default_input_intact: bool,
    },
}

/// What the call application is doing with audio (PD-4.1.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ActivityJudgement {
    /// Not read yet.
    #[default]
    Unknown,
    /// Detection is not possible.
    Unavailable,
    /// The application is not running.
    NotRunning,
    /// The application runs and does not record.
    Idle,
    /// The application records from our microphone.
    ActiveOnOurMicrophone {
        /// It also plays audio (a call rather than a voice message).
        output_running: bool,
    },
    /// The application records from another microphone.
    ActiveOnOtherMicrophone {
        /// Names of the microphones it uses.
        devices: Vec<String>,
    },
}

/// Everything the coordinator needs from the machine, in start-up order.
pub trait CallHost: Send {
    /// Step 1: the process runs from the application bundle.
    fn in_app_bundle(&self) -> bool;

    /// Step 2: microphone consent; asks when undetermined.
    ///
    /// # Errors
    /// `MicrophoneConsentDenied` or `MicrophoneConsentUnavailable`.
    fn ensure_microphone_consent(&mut self) -> Result<(), HostFault>;

    /// Step 2: audio-capture consent; asks when undetermined.
    ///
    /// # Errors
    /// `CaptureConsentDenied` or `CaptureConsentUnavailable`.
    fn ensure_capture_consent(&mut self) -> Result<(), HostFault>;

    /// Step 3: the driver is installed, loaded, current and the variant `target` needs.
    ///
    /// # Errors
    /// One of the `Driver*` codes.
    fn check_driver(&mut self, target: &BundleId) -> Result<(), HostFault>;

    /// Step 4: a physical microphone can be used; returns its name.
    ///
    /// # Errors
    /// `NoMicrophone`.
    fn check_microphone(&mut self) -> Result<String, HostFault>;

    /// Steps 4 and 6: open the physical microphone and the virtual microphone
    /// sink and start the outgoing pump (original voice only), so that the
    /// virtual microphone carries a signal before the default input moves to it.
    ///
    /// # Errors
    /// `EndpointOpenFailed(LocalInput | RemoteOutput)` or a `Driver*` code.
    fn start_outgoing(&mut self) -> Result<Box<dyn AudioPath>, HostFault>;

    /// Steps 5 and 7: write the journal, switch the default input to the
    /// virtual microphone and read it back.
    ///
    /// # Errors
    /// `RoutingJournalPresent`, `RoutingSwitchFailed`, `RoutingUnavailable` or a
    /// `Driver*`/`NoMicrophone` code. A failed entry leaves the default input as it was.
    fn enter_call_mode(&mut self, target: &BundleId) -> Result<EnteredRouting, HostFault>;

    /// Step 8: start the tap of `target` and the incoming pump.
    ///
    /// # Errors
    /// `TargetNotRunning`, a consent code or `EndpointOpenFailed(RemoteInput)`.
    fn start_incoming(&mut self, target: &BundleId) -> Result<Box<dyn AudioPath>, HostFault>;

    /// Step 9: start reading the call application's activity. A failure is not blocking.
    ///
    /// # Errors
    /// `ActivityUnavailable`.
    fn start_activity_probe(&mut self, target: &BundleId) -> Result<(), HostFault>;

    /// The judgement of the activity probe now.
    fn call_activity(&mut self) -> ActivityJudgement;

    /// Stop reading the call application's activity.
    fn stop_activity_probe(&mut self);

    /// Changes of the system since the last call (about once a second).
    fn poll_routing(&mut self) -> Vec<RoutingNotice>;

    /// Make our microphone the default input again (an explicit user action).
    ///
    /// # Errors
    /// `RoutingSwitchFailed` or `VirtualMicrophoneLost`.
    fn reassert_routing(&mut self) -> Result<(), HostFault>;

    /// Put the default input back. Idempotent: not being in call mode is not an error.
    ///
    /// # Errors
    /// `RestoreFailed`; call mode and the journal then stay for the next start.
    fn leave_call_mode(&mut self) -> Result<LeftRouting, HostFault>;

    /// Prepare the translation providers. Only called when translation was requested.
    ///
    /// # Errors
    /// The role that failed and why.
    fn prepare_providers(&mut self) -> Result<(), (ProviderRole, HostFault)>;
}
