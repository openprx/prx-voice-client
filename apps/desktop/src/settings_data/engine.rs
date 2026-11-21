//! The translation engine probes: whether system translation can serve on this machine, and what
//! the local model server is doing. Read-only: nothing is started, downloaded or prompted.

use prxvc_config::{AppConfig, TranslationEngine};
use prxvc_desktop_ui::settings_data::{EngineData, ModelServerView, SystemEngineView};

use crate::llm_server::{ServerState, Snapshot};

/// The engines' status.
pub(crate) fn probe() -> EngineData {
    EngineData {
        system: system_view(),
        server: crate::llm_server::global()
            .map_or(ModelServerView::Stopped, |s| server_view(&s.snapshot())),
    }
}

/// The local model server's state as the settings show it (pure).
pub(crate) fn server_view(snapshot: &Snapshot) -> ModelServerView {
    match &snapshot.state {
        ServerState::Stopped => ModelServerView::Stopped,
        ServerState::Starting { waited } => ModelServerView::Starting {
            waited_s: waited.as_secs(),
        },
        ServerState::Ready { adopted } => ModelServerView::Ready {
            adopted: *adopted,
            address: format!("127.0.0.1:{}", snapshot.port),
        },
        ServerState::Stopping => ModelServerView::Stopping,
        ServerState::Failed(problem) => ModelServerView::Failed(problem.to_string()),
    }
}

/// Whether the server is changing, or about to: the probe polls while it is, so that "starting"
/// turns into "ready" on screen without the user asking.
pub(crate) fn in_motion(config: &AppConfig) -> bool {
    let Some(server) = crate::llm_server::global() else {
        return false;
    };
    let snapshot = server.snapshot();
    let wants_llm =
        TranslationEngine::parse(&config.call.translation_engine) == Some(TranslationEngine::Llm);
    matches!(
        snapshot.state,
        ServerState::Starting { .. } | ServerState::Stopping
    ) || wants_llm != snapshot.pinned
        || (snapshot.pinned && snapshot.state == ServerState::Stopped)
}

/// "Chinese → English" in the interface language.
#[cfg(feature = "apple-translation")]
fn pair_text(from: &str, to: &str) -> String {
    let lang = |code: &str| {
        let key = format!("settings.lang.{code}");
        let text = prxvc_i18n::t(&key);
        if text == key { code.to_owned() } else { text }
    };
    format!("{} \u{2192} {}", lang(from), lang(to))
}

/// System translation from the operating system's point of view.
fn system_view() -> SystemEngineView {
    #[cfg(feature = "apple-translation")]
    {
        use prxvc_adapters::mt::apple::ffi::AppleFramework;
        use prxvc_adapters::mt::apple::{
            Availability, SystemFaultKind, SystemTranslationApi, language::tag,
        };
        use prxvc_media::Lang;
        if let Err(fault) = AppleFramework.os_support() {
            return match fault.kind {
                SystemFaultKind::OsTooOld => SystemEngineView::OsTooOld,
                _ => SystemEngineView::Failed(fault.to_string()),
            };
        }
        for (from, to) in [(Lang::Zh, Lang::En), (Lang::En, Lang::Zh)] {
            let pair = pair_text(from.code(), to.code());
            match AppleFramework.availability(tag(from), tag(to)) {
                Ok(Availability::Installed) => {}
                Ok(Availability::PackNotInstalled) => return SystemEngineView::PackMissing(pair),
                Ok(Availability::Unsupported) => return SystemEngineView::Unsupported(pair),
                Err(fault) => return SystemEngineView::Failed(fault.to_string()),
            }
        }
        SystemEngineView::Ready
    }
    #[cfg(not(feature = "apple-translation"))]
    {
        SystemEngineView::NotBuilt
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::llm_server::ServerProblem;

    fn snap(state: ServerState) -> Snapshot {
        Snapshot {
            state,
            pinned: true,
            leases: 0,
            attempt_seen: 1,
            port: 8080,
        }
    }

    #[test]
    fn every_server_state_has_a_view() {
        assert_eq!(
            server_view(&snap(ServerState::Stopped)),
            ModelServerView::Stopped
        );
        assert_eq!(
            server_view(&snap(ServerState::Starting {
                waited: Duration::from_millis(4_900)
            })),
            ModelServerView::Starting { waited_s: 4 }
        );
        assert_eq!(
            server_view(&snap(ServerState::Ready { adopted: true })),
            ModelServerView::Ready {
                adopted: true,
                address: "127.0.0.1:8080".into()
            }
        );
        assert_eq!(
            server_view(&snap(ServerState::Stopping)),
            ModelServerView::Stopping
        );
        let ModelServerView::Failed(text) =
            server_view(&snap(ServerState::Failed(ServerProblem::PortTaken {
                port: 8080,
            })))
        else {
            panic!("expected a failure view");
        };
        assert!(text.contains("8080"));
    }

    #[cfg(not(feature = "apple-translation"))]
    #[test]
    fn a_build_without_the_bridge_says_so() {
        assert_eq!(probe().system, SystemEngineView::NotBuilt);
    }
}
