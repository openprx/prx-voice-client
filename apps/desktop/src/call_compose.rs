//! Composition of call mode: builds the coordinator thread with the real host, connects its commands and snapshots to the engine loop and the shared UI state, and keeps call mode and the assistant session apart.
//!
//! The UI sends call commands as strings through the existing command queue
//! (the `Call*` group, easy to convert when the typed-command refactor lands);
//! the engine loop hands them to [`CallControl::handle_command`], which turns
//! each into one typed [`CallCommand`]. Snapshots flow the other way: the
//! coordinator's sink maps each one into the UI's view model and stores it in
//! the shared state, which wakes the UI.
//!
//! Exclusion: both users of the microphone path claim it through the shared
//! [`MicOwnership`]. The coordinator claims it on start and refuses (with the
//! typed `AssistantActive` issue) when an assistant session holds it; the
//! engine loop claims it before it starts a session and refuses (with a
//! message) while call mode holds it.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_desktop_ui::SharedAppState;
use prxvc_desktop_ui::call_view::command;
use prxvc_ports::BundleId;
use prxvc_session::call::{
    CallCommand, CallHandle, CallSettings, LocalOutputMode, MicClaim, MicOwner, MicOwnership,
    SendError,
};
use prxvc_session::{DirSet, OutputPolicy, Passthrough};

use crate::event_map::push_error;

/// The call application used when neither the start command nor `call.target_app` names one.
/// The snapshot shows the application in use, so this is never silent.
const DEFAULT_CALL_APP: &str = "net.whatsapp.WhatsApp";

/// How long closing the application waits for call mode to stop and restore the microphone.
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

struct Inner {
    handle: Mutex<Option<CallHandle>>,
    ownership: MicOwnership,
    assistant: Mutex<Option<MicClaim>>,
    shared: SharedAppState,
    handoff: Mutex<Handoff>,
    /// The call commands sent to the coordinator, by name (tests read it).
    #[cfg(test)]
    sent: Mutex<Vec<&'static str>>,
}

/// "End the assistant and start": the start that follows the end of the assistant session.
///
/// The start request is remembered when it is made (so that a refused start can be repeated with
/// the same options). The button arms it; the start is sent when the assistant has let go of the
/// microphone claim, which the engine loop reports through [`CallControl::release_assistant`].
/// Nothing waits on a timer.
#[derive(Debug, Default)]
struct Handoff {
    /// `translate` of the last start request, if any.
    last_start: Option<bool>,
    /// A start is armed: its `translate` flag.
    armed: Option<bool>,
}

impl Handoff {
    /// A start was requested (and may be refused because the assistant holds the microphone).
    fn started(&mut self, translate: bool) {
        self.last_start = Some(translate);
        // Any new start or stop of the user's replaces a pending hand-off.
        self.armed = None;
    }

    /// The user pressed "End assistant and start"; `default` applies without an earlier start.
    fn arm(&mut self, default: bool) {
        self.armed = Some(self.last_start.unwrap_or(default));
    }

    /// The microphone claim is free: the armed start, once.
    fn released(&mut self) -> Option<bool> {
        self.armed.take()
    }

    /// Something else happened (a stop, or a new assistant session): the hand-off no longer applies.
    fn cancel(&mut self) {
        self.armed = None;
    }
}

/// The engine's and the application's handle on call mode. Clones share one coordinator.
#[derive(Clone)]
pub(crate) struct CallControl(Arc<Inner>);

/// A configured passthrough choice (`"original"` or `"silence"`); anything else is reported and means `original`.
fn passthrough(setting: &str, key: &str) -> Passthrough {
    match setting {
        "original" => Passthrough::Original,
        "silence" => Passthrough::Silence,
        other => {
            tracing::warn!(
                key,
                value = other,
                "unknown passthrough setting; using \"original\""
            );
            Passthrough::Original
        }
    }
}

fn local_output_mode(setting: &str) -> LocalOutputMode {
    match setting {
        "translation" => LocalOutputMode::Translation,
        "both" => LocalOutputMode::Both,
        "original" => LocalOutputMode::Original,
        other => {
            tracing::warn!(
                value = other,
                "unknown call.local_output_mode; using \"translation\""
            );
            LocalOutputMode::Translation
        }
    }
}

/// The coordinator's settings from the configuration.
pub(crate) fn settings_from(config: &prxvc_config::AppConfig) -> CallSettings {
    let call = &config.call;
    let target = if call.target_app.trim().is_empty() {
        DEFAULT_CALL_APP
    } else {
        call.target_app.trim()
    };
    CallSettings {
        outgoing: OutputPolicy {
            when_paused: passthrough(&call.outgoing_when_paused, "call.outgoing_when_paused"),
            on_failure: passthrough(&call.outgoing_on_failure, "call.outgoing_on_failure"),
        },
        incoming: OutputPolicy {
            when_paused: passthrough(&call.incoming_when_paused, "call.incoming_when_paused"),
            on_failure: passthrough(&call.incoming_on_failure, "call.incoming_on_failure"),
        },
        local_output_mode: local_output_mode(&call.local_output_mode),
        default_target: Some(BundleId::new(target)),
        stop_grace: Duration::from_millis(u64::from(call.stop_grace_ms)),
        ..CallSettings::default()
    }
}

/// The incoming mixer's settings: the output gain (compensating the call application's ducking), the
/// lowered original of the "both" mode and the jitter buffer.
pub(crate) fn incoming_mixer(config: &prxvc_config::AppConfig) -> prxvc_audio::mix::MixerConfig {
    prxvc_audio::mix::MixerConfig {
        translated_gain_db: config.call.output_gain_db,
        original_gain_db: config.call.original_gain_db,
        ..outgoing_mixer(config)
    }
}

/// The outgoing mixer's settings: the translated lane's jitter buffer and its adaptive pre-roll.
pub(crate) fn outgoing_mixer(config: &prxvc_config::AppConfig) -> prxvc_audio::mix::MixerConfig {
    prxvc_audio::mix::MixerConfig {
        jitter_buffer: Duration::from_millis(u64::from(config.pipeline.jitter_buffer_ms)),
        adaptive_preroll: config.pipeline.adaptive_preroll,
        preroll_max: Duration::from_millis(u64::from(config.pipeline.preroll_max_ms)),
        trim_silence: config.pipeline.trim_silence,
        silence_keep: Duration::from_millis(u64::from(config.pipeline.silence_keep_ms)),
        catch_up_max_speed: config.pipeline.catch_up_max_speed,
        ..prxvc_audio::mix::MixerConfig::default()
    }
}

impl CallControl {
    /// Start the coordinator thread (macOS) and connect it to `shared`. Elsewhere call mode is unavailable.
    pub(crate) fn start(shared: &SharedAppState, config: &prxvc_config::AppConfig) -> Self {
        let ownership = MicOwnership::new();
        let handle = spawn(shared, config, &ownership);
        Self(Arc::new(Inner {
            handle: Mutex::new(handle),
            ownership,
            assistant: Mutex::new(None),
            shared: shared.clone(),
            handoff: Mutex::new(Handoff::default()),
            #[cfg(test)]
            sent: Mutex::new(Vec::new()),
        }))
    }

    /// Run a `Call*` command from the UI; returns whether `cmd` was one.
    ///
    /// `CallEndAssistantAndStart` is not handled here: the engine loop ends the assistant session
    /// and calls [`arm_start_after_assistant`](Self::arm_start_after_assistant).
    pub(crate) fn handle_command(&self, cmd: &str) -> bool {
        let typed = match cmd {
            command::START => CallCommand::Start {
                target_app: None,
                translate: crate::call_kit::config::load_current().call.translate,
            },
            command::START_TRANSLATED => CallCommand::Start {
                target_app: None,
                translate: true,
            },
            command::START_AUDIO_ONLY => CallCommand::Start {
                target_app: None,
                translate: false,
            },
            command::STOP => CallCommand::Stop,
            command::PAUSE => CallCommand::Pause(DirSet::BOTH),
            command::RESUME => CallCommand::Resume,
            command::REAPPLY_MICROPHONE => CallCommand::ReapplyVirtualMicrophone,
            command::SAY_DISCLOSURE => CallCommand::SayDisclosure,
            _ => return false,
        };
        match &typed {
            CallCommand::Start { translate, .. } => self.0.handoff.lock().started(*translate),
            CallCommand::Stop => self.0.handoff.lock().cancel(),
            _ => {}
        }
        self.send(typed, cmd_name(cmd));
        true
    }

    fn send(&self, typed: CallCommand, name: &'static str) {
        #[cfg(test)]
        self.0.sent.lock().push(name);
        let result = match self.0.handle.lock().as_ref() {
            Some(handle) => handle.send(typed),
            None => Err(SendError::Ended),
        };
        if let Err(e) = result {
            tracing::warn!(cmd = name, error = %e, "call command not delivered");
            push_error(&self.0.shared, &format!("Call mode: {e}"));
        }
    }

    /// "End assistant and start" was pressed: remember to start call mode with the options of the
    /// refused start as soon as the assistant has let go of the microphone. The caller ends the
    /// assistant session; [`release_assistant`](Self::release_assistant) sends the start.
    pub(crate) fn arm_start_after_assistant(&self) {
        let default = crate::call_kit::config::load_current().call.translate;
        self.0.handoff.lock().arm(default);
    }

    /// An assistant session wants the microphone: refuse while call mode holds it.
    ///
    /// # Errors
    /// The message to show the user.
    pub(crate) fn claim_assistant(&self) -> Result<(), String> {
        let mut slot = self.0.assistant.lock();
        if slot.is_some() {
            return Ok(());
        }
        match self.0.ownership.claim(MicOwner::Assistant) {
            Ok(claim) => {
                *slot = Some(claim);
                // A new assistant session replaces a pending hand-off to call mode.
                self.0.handoff.lock().cancel();
                Ok(())
            }
            Err(_) => Err(prxvc_i18n::t("call.refused.call_running")),
        }
    }

    /// The assistant session ended: give the microphone back, and send the start that
    /// "End assistant and start" armed, if any. The claim is dropped before the start is sent, so
    /// the coordinator finds the microphone free.
    pub(crate) fn release_assistant(&self) {
        *self.0.assistant.lock() = None;
        let armed = self.0.handoff.lock().released();
        if let Some(translate) = armed {
            self.send(
                CallCommand::Start {
                    target_app: None,
                    translate,
                },
                "CallEndAssistantAndStart",
            );
        }
    }

    /// Whether an assistant session holds the microphone claim now.
    #[cfg(test)]
    pub(crate) fn assistant_holds_microphone(&self) -> bool {
        self.0.assistant.lock().is_some()
    }

    /// A control with no coordinator behind it (the host's tests): what would be sent is recorded.
    #[cfg(test)]
    pub(crate) fn without_coordinator(shared: &SharedAppState) -> Self {
        Self(Arc::new(Inner {
            handle: Mutex::new(None),
            ownership: MicOwnership::new(),
            assistant: Mutex::new(None),
            shared: shared.clone(),
            handoff: Mutex::new(Handoff::default()),
            sent: Mutex::new(Vec::new()),
        }))
    }

    /// Call mode takes the microphone (the test of an assistant that is refused while a call runs).
    #[cfg(test)]
    pub(crate) fn hold_for_call(&self) -> prxvc_session::call::MicClaim {
        self.0
            .ownership
            .claim(MicOwner::Call)
            .expect("the microphone is free")
    }

    /// The call commands sent so far, by name.
    #[cfg(test)]
    pub(crate) fn sent_commands(&self) -> Vec<&'static str> {
        self.0.sent.lock().clone()
    }

    /// Stop call mode (restoring the default input) and end the coordinator; the application is closing.
    pub(crate) fn shutdown(&self) {
        let handle = self.0.handle.lock().take();
        if let Some(mut handle) = handle
            && handle.shutdown(EXIT_TIMEOUT).is_err()
        {
            tracing::error!(
                "call mode did not stop in time; the routing journal stays for the next start"
            );
        }
    }
}

/// The static name of a `Call*` command, for the log.
fn cmd_name(cmd: &str) -> &'static str {
    [
        command::START,
        command::START_TRANSLATED,
        command::START_AUDIO_ONLY,
        command::STOP,
        command::PAUSE,
        command::RESUME,
        command::REAPPLY_MICROPHONE,
    ]
    .into_iter()
    .find(|c| *c == cmd)
    .unwrap_or("Call")
}

#[cfg(target_os = "macos")]
fn spawn(
    shared: &SharedAppState,
    config: &prxvc_config::AppConfig,
    ownership: &MicOwnership,
) -> Option<CallHandle> {
    use prxvc_media::SystemClock;
    use prxvc_session::call::CoordinatorDeps;

    use crate::call_host::{HostConfig, RealCallHost};
    use crate::call_panel::panel_of;

    let Some(paths) = crate::config_load::app_paths() else {
        tracing::warn!("call mode is unavailable: the data directory is unknown");
        return None;
    };
    let trace = crate::call_kit::trace::TraceSlot::default();
    let host_config = HostConfig {
        input_device: config.audio.input_device.clone(),
        virtual_mic_device: config.audio.virtual_mic_device.clone(),
        output_device: config.audio.output_device.clone(),
        echo_mode: config.audio.echo_mode,
        echo_tail: std::time::Duration::from_millis(u64::from(config.audio.half_duplex_tail_ms)),
        incoming_mixer: incoming_mixer(config),
        outgoing_mixer: outgoing_mixer(config),
        data_dir: paths.data_dir.clone(),
        config_source: crate::call_kit::config::current_source(),
        translate: vec![
            prxvc_media::Direction::Outgoing,
            prxvc_media::Direction::Incoming,
        ],
        trace: trace.clone(),
    };
    let settings = settings_from(config);
    let (shared, ownership) = (shared.clone(), ownership.clone());
    let spawned = CallHandle::spawn(move || CoordinatorDeps {
        host: Box::new(RealCallHost::new(host_config)),
        clock: Arc::new(SystemClock::new()),
        ownership,
        sink: Box::new(move |snapshot| {
            let mut panel = panel_of(snapshot, &trace);
            crate::call_engine::annotate(&mut panel);
            shared.set_call_panel(panel);
        }),
        settings,
    });
    match spawned {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::error!("could not start the call coordinator thread: {e}");
            None
        }
    }
}

/// Call mode needs macOS audio routing and process taps.
#[cfg(not(target_os = "macos"))]
fn spawn(
    _shared: &SharedAppState,
    _config: &prxvc_config::AppConfig,
    _ownership: &MicOwnership,
) -> Option<CallHandle> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(f: impl FnOnce(&mut prxvc_config::AppConfig)) -> prxvc_config::AppConfig {
        let mut c = prxvc_config::AppConfig::default();
        f(&mut c);
        c
    }

    #[test]
    fn default_settings_hear_the_original_and_target_the_default_application() {
        let s = settings_from(&prxvc_config::AppConfig::default());
        assert_eq!(s.outgoing, OutputPolicy::default());
        assert_eq!(s.incoming, OutputPolicy::default());
        assert_eq!(s.local_output_mode, LocalOutputMode::Translation);
        assert_eq!(s.default_target, Some(BundleId::new(DEFAULT_CALL_APP)));
    }

    #[test]
    fn configured_policies_and_target_are_read() {
        let s = settings_from(&config_with(|c| {
            c.call.outgoing_when_paused = "silence".into();
            c.call.incoming_on_failure = "silence".into();
            c.call.local_output_mode = "original".into();
            c.call.target_app = " org.telegram.desktop ".into();
        }));
        assert_eq!(s.outgoing.when_paused, Passthrough::Silence);
        assert_eq!(s.outgoing.on_failure, Passthrough::Original);
        assert_eq!(s.incoming.on_failure, Passthrough::Silence);
        assert_eq!(s.local_output_mode, LocalOutputMode::Original);
        assert_eq!(
            s.default_target,
            Some(BundleId::new("org.telegram.desktop"))
        );
    }

    #[test]
    fn an_unknown_setting_falls_back_loudly_to_the_documented_default() {
        let s = settings_from(&config_with(|c| c.call.outgoing_on_failure = "mute".into()));
        assert_eq!(s.outgoing.on_failure, Passthrough::Original);
    }

    #[test]
    fn the_incoming_mixer_takes_the_output_gain_the_lowered_original_and_the_jitter_buffer() {
        let mixer = incoming_mixer(&config_with(|c| {
            c.call.output_gain_db = 9.0;
            c.call.original_gain_db = -18.0;
            c.pipeline.jitter_buffer_ms = 120;
        }));
        assert_eq!(mixer.translated_gain_db, 9.0);
        assert_eq!(mixer.original_gain_db, -18.0);
        assert_eq!(mixer.jitter_buffer, Duration::from_millis(120));
        let default = incoming_mixer(&prxvc_config::AppConfig::default());
        assert!(default.adaptive_preroll);
        assert_eq!(default.preroll_max, Duration::from_millis(1500));
        let off = outgoing_mixer(&config_with(|c| {
            c.pipeline.adaptive_preroll = false;
            c.pipeline.preroll_max_ms = 900;
        }));
        assert!(!off.adaptive_preroll);
        let shaped = outgoing_mixer(&config_with(|c| {
            c.pipeline.trim_silence = false;
            c.pipeline.silence_keep_ms = 90;
            c.pipeline.catch_up_max_speed = 1.1;
        }));
        assert!(!shaped.trim_silence);
        assert_eq!(shaped.silence_keep, Duration::from_millis(90));
        assert_eq!(shaped.catch_up_max_speed, 1.1);
        assert!(default.trim_silence && default.catch_up_max_speed == 1.25);
        assert_eq!(off.preroll_max, Duration::from_millis(900));
        assert_eq!(default.translated_gain_db, 0.0, "no gain unless asked for");
        assert_eq!(default.original_gain_db, -12.0);
    }

    #[test]
    fn the_assistant_and_call_mode_exclude_each_other() {
        let shared = SharedAppState::new();
        let control = CallControl::start(&shared, &prxvc_config::AppConfig::default());
        assert!(control.claim_assistant().is_ok());
        assert!(
            control.claim_assistant().is_ok(),
            "claiming again while holding it is harmless"
        );
        // Call mode is refused while the assistant holds the microphone.
        assert_eq!(control.0.ownership.holder(), Some(MicOwner::Assistant));
        assert!(control.0.ownership.claim(MicOwner::Call).is_err());
        control.release_assistant();
        assert_eq!(control.0.ownership.holder(), None);
        // And the other way round.
        let call = control.0.ownership.claim(MicOwner::Call).expect("free");
        assert!(control.claim_assistant().is_err());
        drop(call);
        assert!(control.claim_assistant().is_ok());
        control.shutdown();
    }

    #[test]
    fn only_call_commands_are_taken() {
        let shared = SharedAppState::new();
        let control = CallControl::start(&shared, &prxvc_config::AppConfig::default());
        assert!(!control.handle_command("StartSession"));
        assert!(!control.handle_command("RefreshDriverStatus"));
        control.shutdown();
    }

    /// A control with no coordinator behind it: what would be sent is recorded in `sent`.
    fn control() -> CallControl {
        CallControl::without_coordinator(&SharedAppState::new())
    }

    fn sent(c: &CallControl) -> Vec<&'static str> {
        c.0.sent.lock().clone()
    }

    #[test]
    fn the_start_after_the_assistant_is_sent_when_its_claim_is_released_and_not_before() {
        let c = control();
        assert!(c.claim_assistant().is_ok());
        // The start the assistant refused, then the button.
        assert!(c.handle_command(command::START_TRANSLATED));
        c.arm_start_after_assistant();
        assert_eq!(
            sent(&c),
            vec![command::START_TRANSLATED],
            "the armed start waits: only the refused one was sent"
        );
        assert_eq!(c.0.ownership.holder(), Some(MicOwner::Assistant));
        c.release_assistant();
        assert_eq!(
            c.0.ownership.holder(),
            None,
            "the claim is free when the start goes"
        );
        assert_eq!(
            sent(&c),
            vec![command::START_TRANSLATED, "CallEndAssistantAndStart"]
        );
        // Releasing again does not start twice.
        c.release_assistant();
        assert_eq!(sent(&c).len(), 2);
    }

    #[test]
    fn the_hand_off_repeats_the_options_of_the_refused_start() {
        let mut h = Handoff::default();
        h.started(false);
        h.arm(true);
        assert_eq!(h.released(), Some(false), "audio only was asked for");
        let mut h = Handoff::default();
        h.started(true);
        h.arm(false);
        assert_eq!(h.released(), Some(true));
        let mut h = Handoff::default();
        h.arm(true);
        assert_eq!(
            h.released(),
            Some(true),
            "without an earlier start the setting applies"
        );
    }

    #[test]
    fn a_stop_a_new_start_or_a_new_assistant_session_cancel_the_hand_off() {
        let c = control();
        assert!(c.claim_assistant().is_ok());
        c.arm_start_after_assistant();
        assert!(c.handle_command(command::STOP));
        c.release_assistant();
        assert_eq!(
            sent(&c),
            vec![command::STOP],
            "nothing started after a stop"
        );

        let c = control();
        assert!(c.claim_assistant().is_ok());
        c.arm_start_after_assistant();
        c.release_assistant();
        assert_eq!(sent(&c), vec!["CallEndAssistantAndStart"]);

        let c = control();
        c.arm_start_after_assistant();
        assert!(c.claim_assistant().is_ok());
        c.release_assistant();
        assert!(
            sent(&c).is_empty(),
            "a new assistant session replaced the hand-off"
        );
    }

    #[test]
    fn end_assistant_and_start_is_not_a_command_of_the_coordinator() {
        let c = control();
        assert!(!c.handle_command(command::END_ASSISTANT_AND_START));
        assert!(sent(&c).is_empty());
    }
}
