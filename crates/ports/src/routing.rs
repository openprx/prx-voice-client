//! System audio routing port: the operating-system actions a call needs (default device, call activity, change events).

use std::fmt;

/// The unique identifier of an audio device.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceUid(String);

impl DeviceUid {
    /// A device identifier from its text.
    pub fn new(uid: impl Into<String>) -> Self {
        Self(uid.into())
    }

    /// The identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceUid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The bundle identifier of an application.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BundleId(String);

impl BundleId {
    /// A bundle identifier from its text.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BundleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A routing failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RoutingError {
    /// The operating system refused or could not be asked.
    #[error("routing unavailable: {reason}")]
    Unavailable {
        /// Why.
        reason: String,
    },
    /// No device has this identifier.
    #[error("no audio device `{0}`")]
    UnknownDevice(DeviceUid),
    /// An unexpected failure.
    #[error("routing internal error: {0}")]
    Internal(String),
}

/// What an application is doing with audio devices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallActivity {
    /// The application has an input stream running.
    pub input_running: bool,
    /// The application has an output stream running.
    pub output_running: bool,
    /// Input devices the application uses.
    pub input_devices: Vec<DeviceUid>,
    /// Output devices the application uses.
    pub output_devices: Vec<DeviceUid>,
}

/// A change in the system the controller must react to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoutingChange {
    /// The default input device changed.
    DefaultInputChanged(DeviceUid),
    /// The default output device changed.
    DefaultOutputChanged(DeviceUid),
    /// A device appeared or disappeared.
    DeviceListChanged,
    /// The machine is going to sleep.
    Sleep,
    /// The machine woke up.
    Wake,
}

/// Operating-system audio routing (implemented by `prxvc-audio` on macOS).
pub trait SystemAudioRouting: Send {
    /// The current default input device.
    fn default_input(&self) -> Result<DeviceUid, RoutingError>;

    /// Make `uid` the default input device.
    fn set_default_input(&self, uid: &DeviceUid) -> Result<(), RoutingError>;

    /// Whether a device with this identifier exists.
    fn device_exists(&self, uid: &DeviceUid) -> bool;

    /// What `app` is doing with audio devices.
    fn call_activity(&self, app: &BundleId) -> Result<CallActivity, RoutingError>;

    /// Changes since the last call, oldest first.
    fn poll_changes(&mut self) -> Vec<RoutingChange>;
}
