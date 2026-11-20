//! Plain data of one application's audio activity: running state, input and output flags and the devices in use.

/// An audio device a process uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDevice {
    /// Core Audio object ID (valid until the device goes away).
    pub object_id: u32,
    /// Stable device UID; empty when unknown.
    pub uid: String,
    /// Human-readable device name; empty when unknown.
    pub name: String,
    /// Channels of the device's input streams.
    pub input_channels: u32,
    /// Channels of the device's output streams.
    pub output_channels: u32,
}

impl ProcessDevice {
    /// Whether the device can render audio (has output streams).
    pub fn is_output_capable(&self) -> bool {
        self.output_channels > 0
    }

    /// The name for messages: the name, or the UID when the name is empty.
    pub fn label(&self) -> &str {
        if self.name.is_empty() { &self.uid } else { &self.name }
    }
}

/// What one application does in the audio system, as read at one moment.
///
/// Several process objects of the same bundle ID (an application with helper
/// processes) are merged: the flags are OR-ed and the devices are united.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppAudioActivity {
    /// The application's bundle ID.
    pub bundle_id: String,
    /// The audio system knows a process of the application (it is running).
    pub present: bool,
    /// A pid of the application, when present.
    pub pid: Option<i32>,
    /// `kAudioProcessPropertyIsRunning`: the process runs audio in either direction.
    pub audio_running: bool,
    /// `kAudioProcessPropertyIsRunningInput`: the process records.
    pub input_running: bool,
    /// `kAudioProcessPropertyIsRunningOutput`: the process plays.
    pub output_running: bool,
    /// Devices the process uses for input, which for a voice-processing call
    /// unit also include its output device as the echo reference.
    pub input_devices: Vec<ProcessDevice>,
    /// Devices the process uses for output.
    pub output_devices: Vec<ProcessDevice>,
}

impl AppAudioActivity {
    /// The record of an application the audio system does not know.
    pub fn absent(bundle_id: impl Into<String>) -> Self {
        Self { bundle_id: bundle_id.into(), ..Self::default() }
    }

    /// Add another process object of the same application.
    pub fn merge(&mut self, other: Self) {
        self.present |= other.present;
        self.pid = self.pid.or(other.pid);
        self.audio_running |= other.audio_running;
        self.input_running |= other.input_running;
        self.output_running |= other.output_running;
        unite(&mut self.input_devices, other.input_devices);
        unite(&mut self.output_devices, other.output_devices);
    }
}

/// Append the devices of `more` that `devices` does not hold yet (by UID, or by
/// object ID when the UID is unknown).
fn unite(devices: &mut Vec<ProcessDevice>, more: Vec<ProcessDevice>) {
    for d in more {
        let known = devices.iter().any(|e| if d.uid.is_empty() { e.object_id == d.object_id } else { e.uid == d.uid });
        if !known {
            devices.push(d);
        }
    }
}
