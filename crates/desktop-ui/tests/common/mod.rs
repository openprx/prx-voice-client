//! Helpers shared by the egui_kittest integration tests.

use std::sync::{Mutex, MutexGuard};

use egui_kittest::{Harness, HarnessBuilder};
use prxvc_desktop_ui::{fonts, theme};

/// The i18n locale is global; tests that change it must not interleave.
static LOCALE_LOCK: Mutex<()> = Mutex::new(());

pub fn lock_locale() -> MutexGuard<'static, ()> {
    LOCALE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A harness builder with the bundled fonts and the RF theme installed as the
/// application has them, and the given system scheme.
///
/// No system CJK face is passed, so the run is deterministic: the embedded
/// Noto Sans SC subset covers the UI's Chinese strings.
pub fn builder<S>(size: egui::Vec2, scheme: egui::Theme) -> HarnessBuilder<S> {
    Harness::builder()
        .with_size(size)
        .with_theme(scheme)
        .with_context_setup(|ctx| {
            fonts::install(ctx, None);
            theme::install(ctx);
        })
}

// ---- the settings window ---------------------------------------------------------------

use prxvc_desktop_ui::settings_command::SettingsCommand;
use prxvc_desktop_ui::settings_data::*;
use prxvc_desktop_ui::settings_window::{self, BodyArgs, SettingsViewState};
use prxvc_desktop_ui::{NetworkStatus, SettingsSection, UiSettings, tool_window};

/// Everything the settings window body reads and changes.
#[allow(dead_code)]
pub struct WindowState {
    pub settings: UiSettings,
    pub data: SettingsData,
    pub commands: Vec<SettingsCommand>,
    pub view: SettingsViewState,
    pub pending_theme: Option<String>,
    pub network: NetworkStatus,
}

#[allow(dead_code)]
impl WindowState {
    pub fn new(section: SettingsSection) -> Self {
        let mut view = SettingsViewState::default();
        view.section = section;
        Self {
            settings: UiSettings::default(),
            data: rich_data(),
            commands: Vec::new(),
            view,
            pending_theme: None,
            network: NetworkStatus::Connected,
        }
    }

    /// The commands issued so far, without the refresh a freshly focused window asks for.
    pub fn commands_without_refresh(&self) -> Vec<SettingsCommand> {
        self.commands
            .iter()
            .filter(|c| **c != SettingsCommand::Refresh)
            .cloned()
            .collect()
    }
}

/// A harness that draws the settings window body in the padded frame of the window,
/// `width` points wide and tall enough that no page is scrolled.
#[allow(dead_code)]
pub fn window_harness(
    width: f32,
    scheme: egui::Theme,
    state: WindowState,
) -> Harness<'static, WindowState> {
    builder(egui::vec2(width, 2600.0), scheme).build_ui_state(
        |ui, st: &mut WindowState| {
            let tk = theme::tokens_for(ui.ctx());
            tool_window::window_frame(&tk).show(ui, |ui| {
                settings_window::body(
                    ui,
                    &tk,
                    BodyArgs {
                        settings: &mut st.settings,
                        data: &st.data,
                        commands: &mut st.commands,
                        view: &mut st.view,
                        current_theme: theme::RF_THEME_NAME,
                        pending_theme: &mut st.pending_theme,
                        network: &st.network,
                    },
                );
            });
        },
        state,
    )
}

fn device(uid: &str, name: &str, transport: TransportView, is_default: bool) -> DeviceView {
    DeviceView {
        uid: uid.into(),
        name: name.into(),
        transport,
        is_default,
    }
}

/// Data with something in every list, so that every page draws its fullest form.
#[allow(dead_code)]
pub fn rich_data() -> SettingsData {
    SettingsData {
        loaded: true,
        audio: AudioData {
            inputs: vec![
                device("builtin-mic", "MacBook Pro Microphone", TransportView::BuiltIn, true),
                device("usb-mic", "USB Studio Mic", TransportView::Usb, false),
            ],
            outputs: vec![
                device("builtin-out", "MacBook Pro Speakers", TransportView::BuiltIn, true),
                device("bt-out", "Headphones", TransportView::Bluetooth, false),
            ],
            input_choice: Some(InputChoiceView::Chosen {
                name: "MacBook Pro Microphone".into(),
                reason: InputReason::SystemDefault,
            }),
            list_error: None,
        },
        permissions: PermissionsData {
            microphone: PermissionView::Allowed,
            call_audio: PermissionView::NotAsked,
            outside_bundle: false,
            is_macos: true,
            microphone_request: None,
            call_audio_request: None,
            driver: DriverView {
                state: DriverStateView::Installed,
                bundle_available: true,
                installed_version: Some("1.2.0".into()),
                installed_variant: Some("usb".into()),
                operation: DriverOperationView::Idle,
            },
            local_endpoints: vec![("Local llama-server".into(), "127.0.0.1:8080".into())],
        },
        pipeline: PipelineData::Resolved {
            rows: vec![
                PipelineRow {
                    role: PipelineRole::Vad,
                    key: String::new(),
                    provider: "silero-vad".into(),
                    target: "silero-vad-6.2".into(),
                    data: DataLeavingView::Nothing,
                    locality: LocalityView::ThisComputer,
                    host: None,
                    fallback: None,
                },
                PipelineRow {
                    role: PipelineRole::Translation,
                    key: "zh>en".into(),
                    provider: "Local llama-server".into(),
                    target: "http://127.0.0.1:8080/v1".into(),
                    data: DataLeavingView::Text,
                    locality: LocalityView::Remote,
                    host: Some("api.example.com".into()),
                    fallback: Some("gemma-4-e4b".into()),
                },
            ],
            leaves_this_computer: true,
            warnings: vec![],
        },
        voice_profile: Some(VoiceProfileView {
            id: "me".into(),
            language: Some("zh".into()),
            prepared_for: vec!["0123456789ab".into()],
        }),
        voice_presets: vec!["warm-en".into(), "calm-zh".into()],
        about: AboutData {
            version: "0.1.0".into(),
            data_dir: "/Users/someone/Library/Application Support/org.openprx.prx-voice-client".into(),
            models_dir: "/Users/someone/Library/Application Support/org.openprx.prx-voice-client/models".into(),
            config_file: "/Users/someone/Library/Application Support/org.openprx.prx-voice-client/config.toml".into(),
            log_file: Some("/Users/someone/Library/Logs/org.openprx.prx-voice-client/prx-voice.log".into()),
            notices_file: Some("/Applications/PRX Voice.app/Contents/Resources/THIRD_PARTY_NOTICES.md".into()),
        },
        notices: vec![],
    }
}
