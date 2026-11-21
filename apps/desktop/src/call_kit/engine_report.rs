//! The translation engine's lines of the readiness report: which engine is selected, whether its
//! prerequisites are in place (the language packs of system translation, the model server of the
//! language model). Reading only: nothing is started, downloaded or prompted.

use prxvc_config::AppConfig;
use prxvc_media::Direction;
use prxvc_providers::MtSpec;

use super::model_server::manages;
use super::providers::probe_server;
use super::readiness::{Line, Status};
use super::server_locate::{find_binary, is_executable_file};

/// The line that names the selected engine.
pub(crate) fn selected_line(config: &AppConfig) -> Line {
    let (what, engine) =
        match prxvc_config::TranslationEngine::parse(&config.call.translation_engine) {
            Some(prxvc_config::TranslationEngine::System) => (
                "system translation (the operating system's own; no server to start)",
                "system",
            ),
            Some(prxvc_config::TranslationEngine::Llm) => (
                "language model (the application starts the local server itself)",
                "llm",
            ),
            None => (
                "not a known engine",
                config.call.translation_engine.as_str(),
            ),
        };
    Line::new(
        Status::Info,
        "translation engine",
        format!("{engine}: {what}"),
    )
}

/// The lines of one direction's translation provider.
pub(crate) fn lines_of(config: &AppConfig, direction: Direction, spec: &MtSpec) -> Vec<Line> {
    if spec.is_system() {
        return system_lines(direction, spec);
    }
    let url = &spec.base_url;
    match probe_server(spec) {
        Ok(()) => vec![Line::new(
            Status::Ok,
            "translation server",
            format!("{direction:?}: {url} answers"),
        )],
        Err(reason) if manages(spec.id.as_str(), url).is_some() => {
            let path_var = std::env::var("PATH").ok();
            match find_binary(
                &config.llm_server.binary,
                path_var.as_deref(),
                &is_executable_file,
            ) {
                Ok(binary) => vec![Line::new(
                    Status::Info,
                    "translation server",
                    format!(
                        "{direction:?}: {url} is not running ({reason}); the application starts it when the language model is the engine (llama-server at {})",
                        binary.display()
                    ),
                )],
                Err(searched) => vec![Line::new(
                    Status::Missing,
                    "translation server",
                    format!(
                        "{direction:?}: llama-server was not found (looked in {}): brew install llama.cpp, or set llm_server.binary",
                        searched
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )],
            }
        }
        Err(reason) => vec![Line::new(
            Status::Missing,
            "translation server",
            format!("{direction:?}: {url} does not answer ({reason})"),
        )],
    }
}

/// The language pack of the pair, from the operating system.
fn system_lines(direction: Direction, spec: &MtSpec) -> Vec<Line> {
    #[cfg(feature = "apple-translation")]
    {
        use prxvc_adapters::mt::apple::ffi::AppleFramework;
        use prxvc_adapters::mt::apple::language::tag;
        use prxvc_adapters::mt::apple::{Availability, SystemFaultKind, SystemTranslationApi};
        let pair = format!("{} to {}", spec.source, spec.target);
        let line = match AppleFramework.os_support() {
            Err(f) if f.kind == SystemFaultKind::OsTooOld => Line::new(
                Status::Missing,
                "system translation",
                format!(
                    "{direction:?}: needs macOS 26 or later; choose the language model in the settings"
                ),
            ),
            Err(f) => Line::new(
                Status::Missing,
                "system translation",
                format!("{direction:?}: {f}"),
            ),
            Ok(()) => match AppleFramework.availability(tag(spec.source), tag(spec.target)) {
                Ok(Availability::Installed) => Line::new(
                    Status::Ok,
                    "system translation",
                    format!("{direction:?}: language pack {pair} installed"),
                ),
                Ok(Availability::PackNotInstalled) => Line::new(
                    Status::Missing,
                    "system translation",
                    format!(
                        "{direction:?}: language pack {pair} not downloaded (System Settings, General, Language & Region, Translation Languages)"
                    ),
                ),
                Ok(Availability::Unsupported) => Line::new(
                    Status::Missing,
                    "system translation",
                    format!("{direction:?}: {pair} is not supported by the framework"),
                ),
                Err(f) => Line::new(
                    Status::Missing,
                    "system translation",
                    format!("{direction:?}: {f}"),
                ),
            },
        };
        vec![line]
    }
    #[cfg(not(feature = "apple-translation"))]
    {
        let _ = spec;
        vec![Line::new(
            Status::Missing,
            "system translation",
            format!(
                "{direction:?}: this build does not include it (build with the apple-translation feature)"
            ),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(engine: &str) -> AppConfig {
        let mut c = AppConfig::default();
        c.call.translation_engine = engine.to_owned();
        c
    }

    #[test]
    fn the_selected_engine_is_named_with_what_that_means() {
        let system = selected_line(&config("system"));
        assert_eq!(system.status, Status::Info);
        assert!(system.detail.starts_with("system:") && system.detail.contains("no server"));
        let llm = selected_line(&config("llm"));
        assert!(llm.detail.contains("starts the local server itself"));
        assert!(
            selected_line(&config("cloud"))
                .detail
                .contains("not a known engine")
        );
    }
}
