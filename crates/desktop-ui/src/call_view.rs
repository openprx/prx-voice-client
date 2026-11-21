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

/// What the user can do about an issue, right next to it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IssueAction {
    /// Nothing beyond reading it.
    #[default]
    None,
    /// End the assistant session that holds the microphone and start call mode again with the same
    /// options (`AssistantActive`).
    EndAssistantAndStart,
    /// Open the Permissions page of the settings (a missing or unavailable consent).
    OpenPermissions,
    /// Open the Speech pipeline page of the settings (a provider, model or translation server
    /// problem).
    OpenSpeechSettings,
    /// Open the Audio page of the settings (headphones, devices, the driver).
    OpenAudioSettings,
    /// Open the Assistant page of the settings (the dialogue backend, the assistant's voice).
    OpenAssistantSettings,
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
    /// What the user can do about it.
    pub action: IssueAction,
}

/// Where a committed clause is in its life, as the captions show it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClauseStatus {
    /// Committed, text not final.
    #[default]
    Committed,
    /// Source text final.
    Recognized,
    /// Being translated.
    Translating,
    /// Translated.
    Translated,
    /// Being synthesised.
    Synthesizing,
    /// Playing.
    Playing,
    /// Played.
    Played,
    /// Cancelled.
    Cancelled,
    /// Skipped to keep up.
    Skipped,
    /// A stage failed.
    Failed,
}

impl ClauseStatus {
    /// The i18n key of the status.
    pub fn key(self) -> &'static str {
        match self {
            Self::Committed => "call.clause.committed",
            Self::Recognized => "call.clause.recognized",
            Self::Translating => "call.clause.translating",
            Self::Translated => "call.clause.translated",
            Self::Synthesizing => "call.clause.synthesizing",
            Self::Playing => "call.clause.playing",
            Self::Played => "call.clause.played",
            Self::Cancelled => "call.clause.cancelled",
            Self::Skipped => "call.clause.skipped",
            Self::Failed => "call.clause.failed",
        }
    }
}

/// Why a clause was skipped, as the captions say it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SkipView {
    /// Not skipped.
    #[default]
    None,
    /// Dropped to keep up with the speaker (stale or backlog or a full queue): "not spoken".
    NotSpoken,
    /// Already in the listener's language: not translated, the original voice passes through.
    AlreadyInTarget,
}

/// A way a finished translation looks wrong (reported, the clause was spoken all the same).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptionFlag {
    /// Mostly in the wrong script for the target language.
    WrongLanguage,
    /// Far longer than a translation can be.
    TooLong,
    /// Stopped at the provider's limit.
    Truncated,
    /// Reads as a refusal.
    Refusal,
    /// Reads as an answer, not a translation.
    AnswerLike,
    /// A thinking tag was left in.
    ThinkResidue,
}

impl CaptionFlag {
    /// Every kind, in the order the counters list them.
    pub const ALL: [Self; 6] = [
        Self::WrongLanguage,
        Self::TooLong,
        Self::Truncated,
        Self::Refusal,
        Self::AnswerLike,
        Self::ThinkResidue,
    ];

    /// The i18n key of the flag's short name.
    pub fn key(self) -> &'static str {
        match self {
            Self::WrongLanguage => "callwin.flag.wrong_language",
            Self::TooLong => "callwin.flag.too_long",
            Self::Truncated => "callwin.flag.truncated",
            Self::Refusal => "callwin.flag.refusal",
            Self::AnswerLike => "callwin.flag.answer_like",
            Self::ThinkResidue => "callwin.flag.think_residue",
        }
    }
}

/// One committed clause of the captions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptionView {
    /// The utterance (segment) the clause belongs to, counted from 1 in its direction.
    pub utterance: u64,
    /// The clause's index inside its utterance.
    pub index: u16,
    /// What was said.
    pub source: String,
    /// Its translation so far.
    pub translation: String,
    /// Where it is.
    pub status: ClauseStatus,
    /// Why it was skipped, when it was.
    pub skip: SkipView,
    /// How its translation looked wrong, when it did.
    pub flags: Vec<CaptionFlag>,
    /// The first translation came back in the wrong language and it was translated again.
    pub retried: bool,
}

/// The clause being spoken, not yet committed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartialView {
    /// The utterance (segment) it belongs to.
    pub utterance: u64,
    /// The recogniser's current reading.
    pub text: String,
    /// Bytes at the start of `text` that will not change.
    pub stable_len: usize,
}

/// Counters of one direction since it started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CountersView {
    /// Results dropped because their clause was cancelled or finished.
    pub late_dropped: u64,
    /// Clauses skipped by the backlog rule.
    pub backlog_skips: u64,
    /// Clauses skipped because they were stale when new speech began.
    pub stale_skips: u64,
    /// Segments cut at the maximum length.
    pub forced_cuts: u64,
    /// Finals that disagreed with committed text.
    pub final_rewrites: u64,
    /// Translations flagged as not being one.
    pub mt_suspect: u64,
    /// Synthesis stopped by the runaway guard.
    pub runaway_synth: u64,
    /// Audio frames dropped before recognition.
    pub capture_overruns: u64,
    /// Times playback ran dry (sink and translated lane).
    pub underruns: u64,
    /// Clauses that fell back to the streaming text.
    pub refine_fallbacks: u64,
    /// Recognised units that were only filler and were neither translated nor spoken.
    pub filler_dropped: u64,
    /// Translated pieces with nothing to say aloud that never reached the synthesiser.
    pub unspeakable_dropped: u64,
    /// Clauses already in the listener's language: not translated, the original passed through.
    pub in_target_passed: u64,
    /// Translations asked for again after a reply in the wrong language.
    pub mt_retries: u64,
    /// Finished translations flagged, by kind, in the order of [`CaptionFlag::ALL`].
    pub flags: [u64; 6],
}

impl CountersView {
    /// The count of one flag kind.
    pub fn flag(&self, flag: CaptionFlag) -> u64 {
        let i = CaptionFlag::ALL.iter().position(|f| *f == flag).unwrap_or(0);
        self.flags[i]
    }

    /// All flagged translations.
    pub fn flags_total(&self) -> u64 {
        self.flags.iter().sum()
    }
}

/// The last finished clause's latency figures.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LatencyView {
    /// Speech detected until the first translated sample plays, milliseconds.
    pub speech_start_to_first_audio_ms: Option<f64>,
    /// The committing pause detected until the first translated sample plays, milliseconds.
    pub pause_to_first_audio_ms: Option<f64>,
    /// Every interval of the clause: name and milliseconds.
    pub stages: Vec<(String, f64)>,
}

/// One audio path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaneView {
    /// The direction translates (a pipeline runs).
    pub translating: bool,
    /// "zh" and "en" of the languages spoken and heard; empty when not translating.
    pub languages: Option<(String, String)>,
    /// The clause being spoken.
    pub partial: Option<PartialView>,
    /// The latest committed clauses, oldest first.
    pub clauses: Vec<CaptionView>,
    /// Counters; `None` when not translating.
    pub counters: Option<CountersView>,
    /// Latency of the last finished clause.
    pub latency: Option<LatencyView>,
    /// Peak of the translated speech as played, linear.
    pub translated_peak: f32,
    /// RMS of the translated speech as played, linear.
    pub translated_rms: f32,
    /// The echo gate is closed: "paused while you listen".
    pub input_gated: bool,
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

/// Where the spoken notice to the other party stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoticeState {
    /// Not part of this call.
    #[default]
    Off,
    /// Being prepared.
    Preparing,
    /// Ready, plays before the first translated sentence.
    Ready,
    /// Playing now.
    Playing,
    /// Played to the end.
    Played,
    /// Could not be prepared.
    Failed,
}

/// The spoken notice to the other party as the window shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoticeView {
    /// The setting is on for this call.
    pub enabled: bool,
    /// Where it stands.
    pub state: NoticeState,
    /// Times it was played to the end.
    pub plays: u32,
    /// Seconds into the call when it was last played.
    pub last_played_s: Option<u32>,
}

/// Everything the control shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallPanel {
    /// Translation was requested for this call.
    pub translate_enabled: bool,
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
    /// The spoken notice to the other party.
    pub notice: NoticeView,
    /// A graceful stop is running: the last sentences are being played before call mode ends.
    /// The seconds left until the limit. A second Stop ends it at once. The window shows the
    /// state "finishing" (`callwin.chip.finishing`) instead of "translating", the hint
    /// `callwin.finishing.hint` and the Stop button as "Stop now" (`callwin.stop_now`).
    pub finishing_s: Option<u32>,
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
    /// Start call mode as `call.translate` says (audio only unless the setting is on).
    pub const START: &str = "CallStart";
    /// Start call mode with translation.
    pub const START_TRANSLATED: &str = "CallStartTranslated";
    /// Start call mode with the audio only (passthrough), whatever `call.translate` says.
    pub const START_AUDIO_ONLY: &str = "CallStartAudioOnly";
    /// Stop call mode.
    pub const STOP: &str = "CallStop";
    /// Pause both directions.
    pub const PAUSE: &str = "CallPause";
    /// Resume.
    pub const RESUME: &str = "CallResume";
    /// Make our microphone the default input again.
    pub const REAPPLY_MICROPHONE: &str = "CallReapplyMicrophone";
    /// Say the spoken notice to the other party again.
    pub const SAY_DISCLOSURE: &str = "CallSayDisclosure";
    /// End the assistant session and, once it has let go of the microphone, start call mode with
    /// the options of the start that was refused.
    pub const END_ASSISTANT_AND_START: &str = "CallEndAssistantAndStart";
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
