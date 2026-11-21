//! What the settings window shows that only the app can find out: devices, permissions, the driver, the speech pipeline, the voice profile and facts for About.
//!
//! [`collect`] runs every probe and returns one [`SettingsData`] (the host publishes it through the
//! feed, on a thread of its own). Every probe is a read-only query: none shows a prompt, installs
//! anything or changes a setting. The files of this module are one probe each:
//!
//! * `audio`: input and output devices and the input policy's verdict;
//! * `permissions`: microphone and call-audio status, the requests the user triggers, the
//!   System Settings panes;
//! * `pipeline`: the provider registry's `resolve_call()` and `egress_summary` as view models, and
//!   the assistant's backends and chain;
//! * `engine`: whether system translation can serve and what the local model server is doing;
//! * `voice`: the installed profile and the preset voices;
//! * `about`: version, directories, notices, local endpoints.

mod about;
mod audio;
mod engine;
mod permissions;
mod pipeline;
mod voice;

use prxvc_config::AppConfig;
use prxvc_config::paths::AppPaths;
use prxvc_desktop_ui::settings_data::{PermissionsData, SettingsData};

pub(crate) use engine::in_motion as engine_in_motion;
pub(crate) use permissions::{request_call_audio, request_microphone, system_pane_url};

/// Run every probe for `config`. `previous` supplies what a probe cannot know: the outcome of
/// a permission request the user made, which stays until the permission is granted, and the
/// notices about the settings file.
pub(crate) fn collect(config: &AppConfig, previous: &SettingsData) -> SettingsData {
    let paths = AppPaths::resolve(config).ok();
    let resources = crate::bundle::resources_dir(crate::bundle::current());
    let microphone = permissions::microphone_status();
    let call_audio = permissions::call_audio_status();
    let keep = |status: prxvc_desktop_ui::settings_data::PermissionView,
                outcome: Option<prxvc_desktop_ui::settings_data::RequestOutcome>| {
        if status == prxvc_desktop_ui::settings_data::PermissionView::Allowed {
            None
        } else {
            outcome
        }
    };
    let catalogue_presets = prxvc_models::Catalogue::embedded()
        .presets
        .iter()
        .map(|p| p.id.clone())
        .collect::<Vec<_>>();
    SettingsData {
        loaded: true,
        audio: audio::probe(&config.audio.input_device),
        permissions: PermissionsData {
            microphone,
            call_audio,
            outside_bundle: crate::bundle::current().is_outside_bundle(),
            is_macos: cfg!(target_os = "macos"),
            microphone_request: keep(microphone, previous.permissions.microphone_request),
            call_audio_request: keep(call_audio, previous.permissions.call_audio_request),
            driver: crate::driver_install::driver_view(),
            local_endpoints: about::local_endpoints(config),
        },
        pipeline: pipeline::probe(config),
        assistant: pipeline::probe_assistant(config),
        engine: engine::probe(),
        voice_profile: paths
            .as_ref()
            .and_then(|p| voice::profile(&p.voices_dir, &config.voice.self_profile_id)),
        voice_presets: match &paths {
            Some(p) => voice::presets(catalogue_presets, &p.data_dir.join("voice-presets")),
            None => catalogue_presets,
        },
        about: about::about(paths.as_ref(), resources.as_deref()),
        notices: previous.notices.clone(),
    }
}
