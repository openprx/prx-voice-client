//! View model of call mode for the temporary debug control: plain data the engine fills and the debug view draws.
//!
//! The UI crate does not depend on the call coordinator. The application maps
//! the coordinator's snapshot into a [`CallPanel`]: enums for the machine
//! state and for what each side hears, a stable i18n key and the English
//! detail for every issue, and numbers for the levels and counters.

/// The phase of the call machine, as the control shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CallPhase {
    /// No call mode.
    #[default]
    Idle,
    /// Start-up checks and endpoints are running.
    Initializing,
    /// Both paths run with passthrough.
    AudioReady,
    /// Both directions translate.
    Translating,
    /// Some direction is paused.
    Paused,
    /// Reduced or failed.
    Degraded,
    /// Endpoints are lost or the system sleeps.
    Recovering,
    /// Shutting down.
    Stopping,
}

impl CallPhase {
    /// The i18n key of the state's name.
    pub fn key(self) -> &'static str {
        match self {
            Self::Idle => "call.state.idle",
            Self::Initializing => "call.state.initializing",
            Self::AudioReady => "call.state.audio_ready",
            Self::Translating => "call.state.translating",
            Self::Paused => "call.state.paused",
            Self::Degraded => "call.state.degraded",
            Self::Recovering => "call.state.recovering",
            Self::Stopping => "call.state.stopping",
        }
    }
}

/// What a listener hears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Heard {
    /// The translation.
    Translation,
    /// The original voice.
    #[default]
    Original,
    /// Nothing.
    Nothing,
}

/// What the call detection concluded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActivityView {
    /// Not read yet.
    #[default]
    Unknown,
    /// Detection is not available.
    Unavailable,
    /// The application is not running.
    NotRunning,
    /// Running, not recording.
    Idle,
    /// A call uses our microphone.
    OnOurMicrophone,
    /// A call uses another microphone.
    OnOtherMicrophone,
}

/// How serious an issue is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Call mode cannot run.
    #[default]
    Blocking,
    /// Reduced operation or something to do.
    Degraded,
    /// Worth showing.
    Info,
}

/// One condition to show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueView {
    /// The i18n key of the message (`call.issue.<code>`).
    pub key: String,
    /// How serious.
    pub severity: IssueSeverity,
    /// English detail from the coordinator, shown in small type.
    pub detail: String,
}

/// One audio path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaneView {
    /// The path runs.
    pub running: bool,
    /// What its listener hears.
    pub hears: Heard,
    /// The device captured from.
    pub device: String,
    /// Frames read.
    pub frames: u64,
    /// RMS of the last window, linear.
    pub rms: f32,
    /// Peak of the last window, linear.
    pub peak: f32,
    /// Samples lost on capture.
    pub overruns: u64,
    /// Samples dropped on the way out.
    pub drops: u64,
    /// Times the output ran dry.
    pub underruns: u64,
}

/// Everything the control shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallPanel {
    /// The call machine's phase.
    pub phase: CallPhase,
    /// Call mode is on (the default input was switched).
    pub in_call_mode: bool,
    /// The remote party's side (outgoing).
    pub outgoing: LaneView,
    /// The local user's side (incoming).
    pub incoming: LaneView,
    /// The physical microphone.
    pub microphone: String,
    /// The virtual microphone sink.
    pub virtual_sink: String,
    /// The call application.
    pub call_app: String,
    /// The call detection's conclusion.
    pub activity: ActivityView,
    /// The microphones a call on another microphone uses.
    pub activity_devices: String,
    /// The default input is the PRX Voice Microphone.
    pub default_input_ours: bool,
    /// The default input that replaced ours.
    pub displaced_to: String,
    /// Conditions that hold now.
    pub issues: Vec<IssueView>,
    /// The last failure.
    pub last_error: Option<IssueView>,
}

impl CallPanel {
    /// Start is possible.
    pub fn can_start(&self) -> bool {
        self.phase == CallPhase::Idle
    }

    /// Stop is possible.
    pub fn can_stop(&self) -> bool {
        !matches!(self.phase, CallPhase::Idle | CallPhase::Stopping)
    }

    /// Pause is possible (the machine accepts it only while translating).
    pub fn can_pause(&self) -> bool {
        matches!(self.phase, CallPhase::Translating | CallPhase::Degraded)
    }

    /// Resume is possible.
    pub fn can_resume(&self) -> bool {
        self.phase == CallPhase::Paused
    }

    /// "Use PRX Voice Microphone again" is possible.
    pub fn can_reapply(&self) -> bool {
        self.in_call_mode && !self.default_input_ours
    }
}

/// The commands the control pushes, in the string form of the engine's command queue.
pub mod command {
    /// Start call mode (audio only).
    pub const START: &str = "CallStart";
    /// Stop call mode.
    pub const STOP: &str = "CallStop";
    /// Pause both directions.
    pub const PAUSE: &str = "CallPause";
    /// Resume.
    pub const RESUME: &str = "CallResume";
    /// Make our microphone the default input again.
    pub const REAPPLY_MICROPHONE: &str = "CallReapplyMicrophone";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_follow_the_phase() {
        let mut p = CallPanel::default();
        assert!(p.can_start() && !p.can_stop() && !p.can_pause() && !p.can_resume());
        p.phase = CallPhase::AudioReady;
        assert!(!p.can_start() && p.can_stop() && !p.can_pause());
        p.phase = CallPhase::Translating;
        assert!(p.can_pause());
        p.phase = CallPhase::Paused;
        assert!(p.can_resume() && !p.can_pause());
        p.phase = CallPhase::Stopping;
        assert!(!p.can_stop() && !p.can_start());
    }

    #[test]
    fn reapply_is_offered_only_after_a_displacement() {
        let mut p = CallPanel {
            in_call_mode: true,
            default_input_ours: true,
            ..CallPanel::default()
        };
        assert!(!p.can_reapply());
        p.default_input_ours = false;
        assert!(p.can_reapply());
        p.in_call_mode = false;
        assert!(!p.can_reapply());
    }

    #[test]
    fn every_phase_has_a_translated_name() {
        for phase in [
            CallPhase::Idle,
            CallPhase::Initializing,
            CallPhase::AudioReady,
            CallPhase::Translating,
            CallPhase::Paused,
            CallPhase::Degraded,
            CallPhase::Recovering,
            CallPhase::Stopping,
        ] {
            assert_ne!(prxvc_i18n::t(phase.key()), phase.key());
        }
    }
}
