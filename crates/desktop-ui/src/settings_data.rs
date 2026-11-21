//! Plain view models the app fills for the settings window: devices, permission and driver status, the speech pipeline, the voice profile and facts for About.
//!
//! The UI crate never calls the audio system, the operating system or the provider registry. The
//! app probes them on its own threads and publishes a [`SettingsData`] through the
//! [`SettingsFeed`](crate::settings_feed::SettingsFeed); everything here is `Clone` plain data so
//! that a snapshot is copied once per frame.

/// How a device is connected, as far as the list shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportView {
    /// Built into the computer.
    BuiltIn,
    /// USB.
    Usb,
    /// Bluetooth.
    Bluetooth,
    /// A software device.
    Virtual,
    /// A combination of devices.
    Aggregate,
    /// Any other known connection.
    Other,
    /// The platform does not say.
    #[default]
    Unknown,
}

/// One audio device of the system.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceView {
    /// Stable identifier; the value stored in the configuration.
    pub uid: String,
    /// Display name.
    pub name: String,
    /// Connection type.
    pub transport: TransportView,
    /// Whether it is the system default for its direction.
    pub is_default: bool,
}

/// Why the input policy chose a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputReason {
    /// The configured device.
    Configured,
    /// The configured device is not connected; the default stands in.
    ConfiguredMissing,
    /// "System default" and the default is a normal microphone.
    SystemDefault,
    /// "System default" but the default is virtual, aggregate or ours, so another device was chosen.
    DefaultAvoided,
}

/// What the input policy decided, or why it could not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputChoiceView {
    /// A device is chosen.
    Chosen {
        /// Name of the device.
        name: String,
        /// Why.
        reason: InputReason,
    },
    /// The configured device is our own virtual microphone, which cannot be an input.
    OwnVirtualDevice,
    /// No physical input exists.
    NoUsableDevice,
}

/// The audio devices and the input policy's verdict.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudioData {
    /// Input devices, our own virtual microphone excluded.
    pub inputs: Vec<DeviceView>,
    /// Output devices that are not hidden, our own virtual devices excluded.
    pub outputs: Vec<DeviceView>,
    /// What the policy picks for the configured input.
    pub input_choice: Option<InputChoiceView>,
    /// The microphone "System default" would use, by the same policy.
    pub default_input: Option<String>,
    /// The name of the system's default output device.
    pub default_output: Option<String>,
    /// The device list could not be read.
    pub list_error: Option<String>,
}

/// What the installed driver needs, in the words of the settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DriverStateView {
    /// Not probed yet.
    #[default]
    Unknown,
    /// This platform has no driver.
    Unsupported,
    /// No driver is installed.
    NotInstalled,
    /// Installed and current.
    Installed,
    /// Installed, an older version than this app carries.
    UpdateAvailable,
    /// Installed with a different transport variant than this app carries.
    WrongVariant,
    /// Something sits at the driver's path but is not readable.
    NeedsRepair,
    /// Installed and newer than this app's copy.
    InstalledIsNewer,
}

/// A driver operation the user started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverOperationKind {
    /// Install or update.
    Install,
    /// Remove.
    Uninstall,
}

/// Where the last driver operation stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DriverOperationView {
    /// Nothing started.
    #[default]
    Idle,
    /// Waiting for the administrator prompt or running.
    Working(DriverOperationKind),
    /// Finished.
    Succeeded(DriverOperationKind),
    /// The user dismissed the administrator prompt.
    Cancelled(DriverOperationKind),
    /// Failed with a stable error code (see the `settings.driver.error.*` strings).
    Failed {
        /// Which operation.
        operation: DriverOperationKind,
        /// The typed error's code, for example `copy_failed`.
        code: String,
    },
}

/// The virtual microphone driver.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DriverView {
    /// What the installed driver needs.
    pub state: DriverStateView,
    /// This app carries a driver it can install.
    pub bundle_available: bool,
    /// Installed version, when readable.
    pub installed_version: Option<String>,
    /// Installed transport variant, when readable.
    pub installed_variant: Option<String>,
    /// The last operation.
    pub operation: DriverOperationView,
}

/// A permission as the operating system reports it, without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionView {
    /// Not probed yet.
    #[default]
    NotChecked,
    /// Allowed.
    Allowed,
    /// Refused or restricted.
    NotAllowed,
    /// Not asked yet.
    NotAsked,
    /// This platform needs no such permission.
    NotNeeded,
}

/// The result of a request the user triggered, shown under the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestOutcome {
    /// The system prompt was answered with yes.
    Granted,
    /// Refused, now or earlier; the user must change it in System Settings.
    Refused,
    /// This copy cannot show the prompt because it does not run from an application bundle.
    NotInBundle,
    /// The prompt was not answered in time.
    NoAnswer,
}

/// Permissions and prerequisites.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PermissionsData {
    /// Microphone.
    pub microphone: PermissionView,
    /// System audio capture for call translation.
    pub call_audio: PermissionView,
    /// The app runs outside its application bundle, so permissions go to the launching application.
    pub outside_bundle: bool,
    /// This is macOS (the rows and System Settings panes exist).
    pub is_macos: bool,
    /// Last microphone request.
    pub microphone_request: Option<RequestOutcome>,
    /// Last call-audio request.
    pub call_audio_request: Option<RequestOutcome>,
    /// The virtual microphone driver.
    pub driver: DriverView,
    /// Provider endpoints that stay reachable in offline mode: `(display name, host)`.
    pub local_endpoints: Vec<(String, String)>,
}

/// The role of a pipeline row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineRole {
    /// Speech detection.
    Vad,
    /// Streaming recognition.
    Recognition,
    /// The refine pass of recognition.
    Refine,
    /// Translation.
    Translation,
    /// Speech synthesis.
    Voice,
    /// The assistant's dialogue backend.
    Dialogue,
}

/// What a provider receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataLeavingView {
    /// Nothing leaves this process.
    Nothing,
    /// Text.
    Text,
    /// Audio.
    Audio,
}

/// Where a provider runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalityView {
    /// This computer.
    ThisComputer,
    /// A private network.
    LocalNetwork,
    /// Elsewhere.
    Remote,
}

/// One resolved provider of the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineRow {
    /// Role.
    pub role: PipelineRole,
    /// Language key: `""` for detection, `zh`, or `zh>en`.
    pub key: String,
    /// Provider name.
    pub provider: String,
    /// A local model's id or an endpoint's address.
    pub target: String,
    /// What leaves the machine.
    pub data: DataLeavingView,
    /// Where it runs.
    pub locality: LocalityView,
    /// Host the data goes to, when it leaves the process.
    pub host: Option<String>,
    /// The configured fallback provider's name, when there is one.
    pub fallback: Option<String>,
}

/// One reason the pipeline cannot run, as plain data for a translated sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineProblem {
    /// The role and language key it concerns (`""`, `"zh"` or `"zh>en"`); `None` for the call as a whole.
    pub subject: Option<(PipelineRole, String)>,
    /// A fallback problem (non-fatal) rather than a primary one.
    pub fallback: bool,
    /// Stable code: the suffix of the `settings.pipeline.problem.*` strings.
    pub code: String,
    /// Values for the `{0}`, `{1}` placeholders of that string.
    pub args: Vec<String>,
}

/// The speech pipeline as the provider registry resolves it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PipelineData {
    /// Not resolved yet.
    #[default]
    NotLoaded,
    /// Every role resolved.
    Resolved {
        /// One row per role and key.
        rows: Vec<PipelineRow>,
        /// Something is sent beyond this computer.
        leaves_this_computer: bool,
        /// Fallback problems.
        warnings: Vec<PipelineProblem>,
    },
    /// At least one role cannot be served.
    Problems(Vec<PipelineProblem>),
}

/// An installed voice profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceProfileView {
    /// Profile id.
    pub id: String,
    /// Language of the reference recording, when recorded.
    pub language: Option<String>,
    /// Short fingerprints of the speech models the voice was prepared for.
    pub prepared_for: Vec<String>,
}

/// Facts shown in About.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AboutData {
    /// Version of the app.
    pub version: String,
    /// The data directory.
    pub data_dir: String,
    /// The directory of the model store.
    pub models_dir: String,
    /// The configuration file.
    pub config_file: String,
    /// The log file, when the app logs to a file.
    pub log_file: Option<String>,
    /// Where the third-party notices are, when found.
    pub notices_file: Option<String>,
}

/// Something the user should be told about the settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsNotice {
    /// The settings file could not be read; it was kept under another name.
    UnreadableFileKept {
        /// The new name of the file.
        kept_as: String,
    },
    /// A change could not be saved; the old value is in effect.
    SaveFailed {
        /// The error text.
        detail: String,
    },
}

/// Everything the app publishes for the settings window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SettingsData {
    /// The first probe has finished.
    pub loaded: bool,
    /// Devices.
    pub audio: AudioData,
    /// Permissions and prerequisites.
    pub permissions: PermissionsData,
    /// The speech pipeline.
    pub pipeline: PipelineData,
    /// The installed voice profile, when there is one.
    pub voice_profile: Option<VoiceProfileView>,
    /// Ids of the preset voices that can be chosen.
    pub voice_presets: Vec<String>,
    /// About.
    pub about: AboutData,
    /// Notices about the settings file.
    pub notices: Vec<SettingsNotice>,
}
