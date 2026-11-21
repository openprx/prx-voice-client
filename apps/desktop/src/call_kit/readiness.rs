//! What a first translated call needs from this machine, as report lines: native runtimes, consents, the driver, the audio devices, the models, the voices and the translation server.
//!
//! Reading only: nothing is created, installed or started, and no permission prompt is raised
//! (a permission is read, never requested). The consents are those of the process that reads
//! them, so they are meaningful from inside the application bundle (`--check`) and not from a
//! terminal.

use prxvc_audio::core_audio_defaults::{DefaultKind, default_entry};
use prxvc_audio::driver_status::{DriverStatus, DriverVariant};
use prxvc_audio::mic_consent::{self, MicPermission};
use prxvc_audio::tap_consent::{self, PermissionStatus};
use prxvc_config::AppConfig;
use prxvc_media::Direction;
use prxvc_session::call::{CallErrorCode, OutputRoute, ProviderIssue};

use super::problems::problem_fault;
use super::providers::ProviderHost;
use super::route::{classify, output_entry};

/// How a line reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// In place.
    Ok,
    /// Not in place: the detail says what to do.
    Missing,
    /// Worth knowing, nothing to do.
    Info,
}

/// One finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    pub status: Status,
    pub label: String,
    pub detail: String,
}

impl Line {
    pub(crate) fn new(status: Status, label: &str, detail: impl Into<String>) -> Self {
        Self {
            status,
            label: label.to_owned(),
            detail: detail.into(),
        }
    }
}

/// The lines as text, one finding per line.
pub(crate) fn render(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|l| {
            let tag = match l.status {
                Status::Ok => "ok     ",
                Status::Missing => "MISSING",
                Status::Info => "info   ",
            };
            format!("[{tag}] {}: {}", l.label, l.detail)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Consents, driver and devices.
pub(crate) fn machine_lines(config: &AppConfig) -> Vec<Line> {
    let mut lines = Vec::new();
    lines.push(match mic_consent::permission_status() {
        MicPermission::Granted => Line::new(Status::Ok, "microphone consent", "granted to this process"),
        MicPermission::Denied => Line::new(
            Status::Missing,
            "microphone consent",
            "denied: allow PRX Voice under System Settings > Privacy & Security > Microphone",
        ),
        MicPermission::Undetermined => Line::new(
            Status::Missing,
            "microphone consent",
            "not asked yet: the first call start shows the system prompt (run from the app bundle to see its state)",
        ),
    });
    lines.push(match tap_consent::permission_status() {
        PermissionStatus::Granted => Line::new(Status::Ok, "audio capture consent", "granted to this process"),
        PermissionStatus::Denied => Line::new(
            Status::Missing,
            "audio capture consent",
            "denied: allow PRX Voice under System Settings > Privacy & Security > Screen & System Audio Recording",
        ),
        PermissionStatus::Undetermined => Line::new(
            Status::Missing,
            "audio capture consent",
            "not asked yet: the first call start shows the system prompt",
        ),
        PermissionStatus::Unknown => Line::new(
            Status::Info,
            "audio capture consent",
            "the system gives no answer from this process",
        ),
    });
    let driver = DriverStatus::detect();
    lines.push(match driver.check(Some(DriverVariant::Usb)) {
        Ok(()) => Line::new(
            Status::Ok,
            "virtual microphone driver",
            format!(
                "installed and loaded, variant {:?}, version {}",
                driver.variant(),
                driver.installed_version().unwrap_or("?")
            ),
        ),
        Err(issue) => Line::new(
            Status::Missing,
            "virtual microphone driver",
            format!("{issue} (install it from the app or with driver/macos/install.sh)"),
        ),
    });
    lines.push(match default_entry(DefaultKind::Input) {
        Some(d) => Line::new(Status::Info, "default input", d.name),
        None => Line::new(Status::Missing, "default input", "no default input device"),
    });
    lines.push(match output_entry(&config.audio.output_device) {
        Some(d) => {
            let route = classify(d.transport, &d.name);
            match route {
                OutputRoute::Speakers => Line::new(
                    Status::Missing,
                    "local output",
                    format!("{} is the built-in speaker: use headphones, or the translation is heard by the microphone and translated again", d.name),
                ),
                OutputRoute::Headphones => Line::new(Status::Ok, "local output", format!("{} (headphones)", d.name)),
                OutputRoute::Unknown => Line::new(
                    Status::Info,
                    "local output",
                    format!("{} (route not known: headphones are assumed; with audio.echo_mode auto the half-duplex gate applies)", d.name),
                ),
            }
        }
        None => Line::new(Status::Missing, "local output", "no output device"),
    });
    lines
}

/// Models, voices, preset and the translation server, from resolving the call plan.
pub(crate) fn provider_lines(config: &AppConfig) -> Vec<Line> {
    let mut lines = Vec::new();
    let registry = match ProviderHost::registry_for(config, None) {
        Ok(r) => r,
        Err(fault) => {
            return vec![Line::new(Status::Missing, "providers", fault.detail)];
        }
    };
    match registry.resolve_directions(&[Direction::Outgoing, Direction::Incoming]) {
        Ok(plan) => {
            lines.push(Line::new(
                Status::Ok,
                "languages",
                format!(
                    "{} to {} outgoing, {} to {} incoming",
                    plan.directions[0].source,
                    plan.directions[0].target,
                    plan.directions[1].source,
                    plan.directions[1].target
                ),
            ));
            lines.push(super::engine_report::selected_line(config));
            lines.push(Line::new(
                Status::Ok,
                "voice activity",
                format!("{} ready", plan.vad.primary.id),
            ));
            for d in &plan.directions {
                lines.push(Line::new(
                    Status::Ok,
                    "recognition",
                    format!("{:?}: {} ready", d.direction, d.asr.primary.id),
                ));
                if let Some(r) = &d.refine {
                    lines.push(Line::new(
                        Status::Ok,
                        "refine pass",
                        format!("{:?}: {} ready", d.direction, r.primary.id),
                    ));
                }
                lines.push(Line::new(
                    Status::Ok,
                    "voice",
                    format!("{:?}: {:?}", d.direction, d.tts.primary.voice.selection()),
                ));
                lines.extend(super::engine_report::lines_of(config, d.direction, &d.mt.primary));
            }
        }
        Err(problems) => {
            for p in &problems {
                let fault = problem_fault(p);
                lines.push(Line::new(
                    Status::Missing,
                    label_of(fault.code),
                    fault.detail,
                ));
            }
        }
    }
    lines
}

fn label_of(code: CallErrorCode) -> &'static str {
    match code {
        CallErrorCode::Provider(ProviderIssue::ModelNotInstalled) => {
            "model not installed (scripts/dev/adopt-models.sh)"
        }
        CallErrorCode::Provider(ProviderIssue::ModelInstalling) => "model being installed",
        CallErrorCode::Provider(ProviderIssue::ModelDamaged) => "model damaged",
        CallErrorCode::Provider(ProviderIssue::RuntimeNotBuiltIn) => {
            "runtime not built in (build with the native features)"
        }
        CallErrorCode::Provider(ProviderIssue::VoiceProfileMissing) => {
            "voice profile missing (scripts/dev/install-voices.sh)"
        }
        CallErrorCode::Provider(ProviderIssue::VoiceNotPrepared) => {
            "voice not prepared (scripts/dev/install-voices.sh)"
        }
        CallErrorCode::Provider(ProviderIssue::VoicePresetMissing) => {
            "preset voice missing (scripts/dev/install-voices.sh)"
        }
        CallErrorCode::Provider(ProviderIssue::ServerUnreachable | ProviderIssue::ServerFailed) => {
            "translation server"
        }
        CallErrorCode::Provider(ProviderIssue::LanguagePackMissing) => {
            "system translation language pack missing (System Settings, General, Language & Region, Translation Languages)"
        }
        CallErrorCode::Provider(ProviderIssue::OsTooOld) => {
            "system translation needs macOS 26 (switch the engine to the language model)"
        }
        CallErrorCode::Provider(ProviderIssue::Offline) => "offline mode blocks a provider",
        CallErrorCode::Provider(ProviderIssue::Unsupported) => "provider cannot serve",
        CallErrorCode::Provider(ProviderIssue::Setting) => "setting",
        _ => "provider",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_render_with_their_status() {
        let text = render(&[
            Line::new(Status::Ok, "a", "fine"),
            Line::new(Status::Missing, "b", "do this"),
            Line::new(Status::Info, "c", "fyi"),
        ]);
        assert_eq!(
            text,
            "[ok     ] a: fine\n[MISSING] b: do this\n[info   ] c: fyi"
        );
    }
}
