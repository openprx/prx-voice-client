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
        ..CallSettings::default()
    }
}

/// The incoming mixer's settings: the output gain (compensating the call application's ducking), the
/// lowered original of the "both" mode and the jitter buffer.
pub(crate) fn incoming_mixer(config: &prxvc_config::AppConfig) -> prxvc_audio::mix::MixerConfig {
    prxvc_audio::mix::MixerConfig {
        translated_gain_db: config.call.output_gain_db,
        original_gain_db: config.call.original_gain_db,
        jitter_buffer: Duration::from_millis(u64::from(config.pipeline.jitter_buffer_ms)),
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
        }))
    }

    /// Run a `Call*` command from the UI; returns whether `cmd` was one.
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
            _ => return false,
        };
        let result = match self.0.handle.lock().as_ref() {
            Some(handle) => handle.send(typed),
            None => Err(SendError::Ended),
        };
        if let Err(e) = result {
            tracing::warn!(cmd, error = %e, "call command not delivered");
            push_error(&self.0.shared, &format!("Call mode: {e}"));
        }
        true
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
                Ok(())
            }
            Err(_) => Err(prxvc_i18n::t("call.refused.call_running")),
        }
    }

    /// The assistant session ended: give the microphone back.
    pub(crate) fn release_assistant(&self) {
        *self.0.assistant.lock() = None;
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
    let host_config = HostConfig {
        input_device: config.audio.input_device.clone(),
        virtual_mic_device: config.audio.virtual_mic_device.clone(),
        output_device: config.audio.output_device.clone(),
        echo_mode: config.audio.echo_mode,
        echo_tail: std::time::Duration::from_millis(u64::from(config.audio.half_duplex_tail_ms)),
        incoming_mixer: incoming_mixer(config),
        data_dir: paths.data_dir.clone(),
        config_source: crate::call_kit::config::current_source(),
        translate: vec![prxvc_media::Direction::Outgoing, prxvc_media::Direction::Incoming],
        trace: Arc::new(crate::call_kit::trace::NullTrace),
    };
    let settings = settings_from(config);
    let (shared, ownership) = (shared.clone(), ownership.clone());
    let spawned = CallHandle::spawn(move || CoordinatorDeps {
        host: Box::new(RealCallHost::new(host_config)),
        clock: Arc::new(SystemClock::new()),
        ownership,
        sink: Box::new(move |snapshot| shared.set_call_panel(panel_of(snapshot))),
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
}
