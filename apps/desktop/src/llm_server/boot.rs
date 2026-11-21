//! Building the real manager for the application and the process-wide handle consumers use.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_config::AppConfig;
use prxvc_config::paths::AppPaths;
use prxvc_models::{Catalogue, ModelId};
use prxvc_providers::{DEFAULT_MT_PROVIDER, ModelSource};

use super::core::{Core, Demand, Deps, Settings};
use super::handle::LlmServer;
use super::locate::{find_binary, is_executable_file};
use super::problem::ServerProblem;
use super::real::{HttpProbe, RealLauncher, RealProcesses, SystemClock};
use super::record::{Sweep, sweep};
use crate::call_kit::models::StoreModels;

/// The model the server serves, and the name it answers to (`--alias`): the catalogue id of the
/// default translation model.
pub(crate) const MODEL_ID: &str = "gemma-4-e4b-it-qat-q4-0";

/// Whether `provider` at `base_url` is the server this manager runs: the built-in
/// `local-llama-server` on this computer.
pub(crate) fn manages(provider: &str, base_url: &str) -> Option<u16> {
    if provider != DEFAULT_MT_PROVIDER {
        return None;
    }
    let url = url::Url::parse(base_url).ok()?;
    let local = matches!(
        url.host_str(),
        Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
    );
    if local {
        url.port_or_known_default()
    } else {
        None
    }
}

/// The settings of the real server: the arguments of `scripts/dev/llama-server.sh` (two parallel
/// slots, 8192 tokens of context, all layers on the GPU), port 8080.
pub(crate) fn settings(config: &AppConfig, paths: &AppPaths) -> Settings {
    Settings {
        alias: MODEL_ID.to_owned(),
        port: 8080,
        parallel: 2,
        context: 8192,
        gpu_layers: 99,
        health_timeout: Duration::from_secs(120),
        idle: Duration::from_secs(u64::from(config.llm_server.idle_stop_s)),
        stop_grace: Duration::from_secs(10),
        max_restarts: 3,
        restart_window: Duration::from_secs(300),
        log: paths.logs_dir.join("llm-server.log"),
        record: paths.data_dir.join("llm-server").join("server.json"),
    }
}

/// Resolve the binary (config key, default directories, `PATH`) and the model file (the model
/// store). Reads the store on every start, so a model installed meanwhile is found.
fn resolver(configured: String, models_dir: std::path::PathBuf) -> super::core::Resolve {
    Box::new(move || {
        let path_var = std::env::var("PATH").ok();
        let binary = find_binary(&configured, path_var.as_deref(), &is_executable_file)?;
        let catalogue = Arc::new(Catalogue::embedded());
        let missing = |detail: String| ServerProblem::ModelMissing { detail };
        let store = StoreModels::open(models_dir.clone(), catalogue)
            .map_err(|e| missing(format!("the model store cannot be read ({e})")))?;
        let ready = store
            .ready(&ModelId::new(MODEL_ID))
            .map_err(|e| missing(e.to_string()))?;
        let file = ready
            .file("model")
            .ok_or_else(|| missing("the model has no model file".to_owned()))?
            .to_path_buf();
        Ok((binary, file))
    })
}

/// Sweep a server left behind by a crashed run, then start the manager.
pub(crate) fn start(config: &AppConfig, paths: &AppPaths) -> LlmServer {
    build(
        settings(config, paths),
        config.llm_server.binary.clone(),
        paths.models_dir.clone(),
    )
}

/// The manager over real processes with explicit settings (the live test uses another port).
pub(crate) fn build(
    settings: Settings,
    binary: String,
    models_dir: std::path::PathBuf,
) -> LlmServer {
    let swept = sweep(
        &settings.record,
        &RealProcesses,
        super::ports::Clock::unix_ms(&SystemClock::new()),
    );
    match swept {
        Sweep::Stopped => tracing::warn!("stopped a model server left behind by an earlier run"),
        Sweep::Stale | Sweep::Nothing => {}
    }
    let demand = Arc::new(Mutex::new(Demand::default()));
    let deps = Deps {
        launcher: Arc::new(RealLauncher),
        probe: Arc::new(HttpProbe),
        clock: Arc::new(SystemClock::new()),
        resolve: resolver(binary, models_dir),
    };
    let core = Core::new(deps, settings.clone(), Arc::clone(&demand));
    LlmServer::spawn(core, demand, &settings)
}

static GLOBAL: OnceLock<LlmServer> = OnceLock::new();

/// Start the manager once for this process and keep it.
pub(crate) fn install(config: &AppConfig, paths: &AppPaths) -> &'static LlmServer {
    GLOBAL.get_or_init(|| start(config, paths))
}

/// The process-wide manager; `None` before [`install`] (tests, harnesses: the server is then not
/// managed and consumers only probe it, as before).
pub(crate) fn global() -> Option<&'static LlmServer> {
    GLOBAL.get()
}

/// Stop the server we started, when the application closes.
pub(crate) fn shutdown() {
    if let Some(server) = GLOBAL.get() {
        server.shutdown();
    }
}
