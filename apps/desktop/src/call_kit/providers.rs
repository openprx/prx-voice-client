//! Preparing the translation providers of a call from the registry: resolve what the directions need, report every problem, build, and free everything when the call ends.
//!
//! Nothing here starts a pipeline: it returns the providers and the parameters of each translated
//! direction ([`DirectionBuild`]), which the coordinator hands to its pipelines. The providers
//! are owned by the pipelines from then on; when the last of them is dropped the registry's
//! shared engines are freed ([`ProviderHost::release`] reports the count).

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use prxvc_config::AppConfig;
use prxvc_config::paths::AppPaths;
use prxvc_media::Direction;
use prxvc_models::Catalogue;
use prxvc_ports::{TraceSink, VadProvider};
use prxvc_providers::{
    DirectionPlan, EnvCredentials, LiveEngines, Locality, MtSpec, ProviderRegistry, VoiceDirs,
};
use prxvc_session::ProviderRole;
use prxvc_session::call::{
    CallErrorCode, DirectionBuild, HostFault, PreparedProviders, ProvidersFailure,
};
use prxvc_session::pipeline::PipelineProviders;

use super::config::{ConfigSource, paths_for};
use super::models::StoreModels;
use super::params::pipeline_params;
use super::problems::{build_fault, problem_fault, role_of, unreachable_fault};

/// How long the translation server may take to accept a connection.
const SERVER_PROBE: Duration = Duration::from_secs(1);

/// Builds the providers of a call and keeps the registry that owns their engines.
pub(crate) struct ProviderHost {
    source: ConfigSource,
    directions: Vec<Direction>,
    registry: Option<Arc<ProviderRegistry>>,
    /// Directories to use instead of the platform's (tests).
    paths: Option<AppPaths>,
}

impl ProviderHost {
    /// A host that translates `directions` with the configuration `source` gives.
    pub(crate) fn new(source: ConfigSource, directions: Vec<Direction>) -> Self {
        Self {
            source,
            directions,
            registry: None,
            paths: None,
        }
    }

    /// Use these directories instead of the platform's.
    #[must_use]
    pub(crate) fn with_paths(mut self, paths: AppPaths) -> Self {
        self.paths = Some(paths);
        self
    }

    /// The registry of the configuration `config`, over the model store of its directories.
    pub(crate) fn registry_for(
        config: &AppConfig,
        paths: Option<&AppPaths>,
    ) -> Result<Arc<ProviderRegistry>, HostFault> {
        fn setting(detail: String) -> HostFault {
            HostFault::new(
                CallErrorCode::Provider(prxvc_session::call::ProviderIssue::Setting),
                detail,
            )
        }
        let paths = match paths {
            Some(p) => p.clone(),
            None => paths_for(config).map_err(setting)?,
        };
        let catalogue = Arc::new(Catalogue::embedded());
        let models = StoreModels::open(paths.models_dir.clone(), Arc::clone(&catalogue))
            .map_err(|e| setting(format!("the model store cannot be read: {e}")))?;
        Ok(Arc::new(ProviderRegistry::new(
            config,
            catalogue,
            Arc::new(models),
            Arc::new(EnvCredentials::from_process()),
            VoiceDirs::from_paths(&paths),
        )))
    }

    /// Resolve and build the providers of the directions to translate.
    ///
    /// # Errors
    /// Every problem found, each as a typed fault; nothing stays loaded.
    pub(crate) fn prepare(
        &mut self,
        trace: Arc<dyn TraceSink>,
    ) -> Result<PreparedProviders, ProvidersFailure> {
        let config = (self.source)();
        let registry = Self::registry_for(&config, self.paths.as_ref())
            .map_err(|fault| ProvidersFailure::single(ProviderRole::Asr, fault))?;
        let plan = registry
            .resolve_directions(&self.directions)
            .map_err(|problems| {
                let role = problems.first().map_or(ProviderRole::Asr, role_of);
                ProvidersFailure {
                    role,
                    faults: problems.iter().map(problem_fault).collect(),
                }
            })?;
        for warning in &plan.warnings {
            tracing::warn!(%warning, "a fallback provider cannot be used");
        }
        check_servers(&plan.directions)?;
        let built = build_directions(&registry, &config, &plan);
        // Keep the registry only when something was built, so that `release` can report the engines.
        self.registry = Some(Arc::clone(&registry));
        let directions = built?;
        Ok(PreparedProviders { directions, trace })
    }

    /// Let go of the registry after the call ended and report which engines are still alive.
    pub(crate) fn release(&mut self) -> LiveEngines {
        let live = self
            .registry
            .take()
            .map(|r| r.live_engines())
            .unwrap_or_default();
        if live != LiveEngines::default() {
            tracing::warn!(?live, "engines are still alive after the call ended");
        } else {
            tracing::info!("every provider engine was freed");
        }
        live
    }
}

/// The translation server of each direction must accept a connection.
fn check_servers(directions: &[DirectionPlan]) -> Result<(), ProvidersFailure> {
    let mut faults = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for d in directions {
        let spec = &d.mt.primary;
        if seen.contains(&spec.base_url.as_str()) {
            continue;
        }
        seen.push(&spec.base_url);
        if let Err(reason) = probe_server(spec) {
            faults.push(unreachable_fault(&spec.base_url, &reason));
        }
    }
    if faults.is_empty() {
        Ok(())
    } else {
        Err(ProvidersFailure {
            role: ProviderRole::Mt,
            faults,
        })
    }
}

/// Connect to a local or local-network server; a remote endpoint is judged by its first request.
pub(crate) fn probe_server(spec: &MtSpec) -> Result<(), String> {
    if spec.locality == Locality::Remote {
        return Ok(());
    }
    let url = url::Url::parse(&spec.base_url).map_err(|e| e.to_string())?;
    let host = url.host_str().ok_or("the address has no host")?;
    let port = url
        .port_or_known_default()
        .ok_or("the address has no port")?;
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("the address does not resolve")?;
    TcpStream::connect_timeout(&addr, SERVER_PROBE)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Build every provider the plan names; the first failures of a role are reported together.
pub(crate) fn build_directions(
    registry: &ProviderRegistry,
    config: &AppConfig,
    plan: &prxvc_providers::DirectionsPlan,
) -> Result<Vec<DirectionBuild>, ProvidersFailure> {
    let fail =
        |role, e: &prxvc_providers::BuildError| ProvidersFailure::single(role, build_fault(e));
    let vad: Arc<dyn VadProvider> = Arc::from(
        registry
            .build_vad(&plan.vad.primary)
            .map_err(|e| fail(ProviderRole::Vad, &e))?,
    );
    let mut out = Vec::new();
    for d in &plan.directions {
        let asr = registry
            .build_asr(&d.asr.primary)
            .map_err(|e| fail(ProviderRole::Asr, &e))?;
        let refine = match &d.refine {
            Some(r) => Some(
                registry
                    .build_asr_refine(&r.primary)
                    .map_err(|e| fail(ProviderRole::AsrRefine, &e))?,
            ),
            None => None,
        };
        let mt = registry
            .build_mt(&d.mt.primary)
            .map_err(|e| fail(ProviderRole::Mt, &e))?;
        let tts = registry
            .build_tts(&d.tts.primary)
            .map_err(|e| fail(ProviderRole::Tts, &e))?;
        let params = pipeline_params(
            config,
            d.direction,
            (d.source, d.target),
            d.tts.primary.voice.selection(),
            plan.recognition,
        );
        out.push(DirectionBuild {
            params,
            providers: PipelineProviders {
                vad: Arc::clone(&vad),
                asr: Arc::from(asr),
                refine: refine.map(Arc::from),
                mt: Arc::from(mt),
                tts: Arc::from(tts),
            },
            // Only the outgoing direction speaks to the other party.
            disclosure: (d.direction == Direction::Outgoing)
                .then(|| super::disclosure::disclosure_spec(config, d.target)),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call_kit::trace::NullTrace;
    use prxvc_session::call::{CallErrorCode, ProviderIssue};

    fn host(dir: &std::path::Path, directions: Vec<Direction>) -> ProviderHost {
        let mut config = AppConfig::default();
        // The owner's interface language is Chinese: the call languages derive to zh and en.
        config.ui.language = "zh-CN".into();
        let paths = AppPaths::from_roots(
            dir.join("config"),
            dir.join("data"),
            dir.join("data/models"),
            &config,
        );
        ProviderHost::new(Arc::new(move || config.clone()), directions).with_paths(paths)
    }

    fn codes(failure: &ProvidersFailure) -> Vec<CallErrorCode> {
        failure.faults.iter().map(|f| f.code).collect()
    }

    #[test]
    fn an_empty_installation_reports_every_problem_with_what_to_do() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let mut host = host(dir.path(), vec![Direction::Outgoing, Direction::Incoming]);
        let failure = match host.prepare(Arc::new(NullTrace)) {
            Ok(_) => panic!("nothing is installed, so nothing can be prepared"),
            Err(f) => f,
        };
        let codes = codes(&failure);
        assert!(
            codes.contains(&CallErrorCode::Provider(ProviderIssue::ModelNotInstalled)),
            "{codes:?}"
        );
        assert!(
            codes.contains(&CallErrorCode::Provider(ProviderIssue::VoiceProfileMissing)),
            "the owner's voice is reported with the models: {codes:?}"
        );
        assert!(
            codes.contains(&CallErrorCode::Provider(ProviderIssue::VoicePresetMissing)),
            "the incoming direction's preset voice is reported too: {codes:?}"
        );
        assert!(failure.faults.iter().all(|f| !f.detail.is_empty()));
        assert_eq!(host.release(), LiveEngines::default());
    }

    #[test]
    fn a_direction_that_does_not_translate_needs_none_of_its_roles() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let mut host = host(dir.path(), vec![Direction::Outgoing]);
        let failure = match host.prepare(Arc::new(NullTrace)) {
            Ok(_) => panic!("nothing is installed"),
            Err(f) => f,
        };
        assert!(
            !codes(&failure).contains(&CallErrorCode::Provider(ProviderIssue::VoicePresetMissing)),
            "the preset voice belongs to the incoming direction only: {:?}",
            failure.faults
        );
    }
}
