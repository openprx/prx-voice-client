//! The typed reasons the local model server is not available, each with what to do.

use std::path::PathBuf;

/// Why the server could not be provided.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ServerProblem {
    /// No `llama-server` binary was found.
    #[error(
        "llama-server was not found (looked in {}). Install it (brew install llama.cpp) or set `llm_server.binary` in the config file.",
        searched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    )]
    BinaryMissing { searched: Vec<PathBuf> },
    /// The model file is not in the model store.
    #[error(
        "the language model is not installed: {detail}. Install it with scripts/dev/adopt-models.sh or the model manager."
    )]
    ModelMissing { detail: String },
    /// Something other than a compatible server answers on the port.
    #[error(
        "port {port} is used by another program that is not a compatible llama-server; free the port or use another address"
    )]
    PortTaken { port: u16 },
    /// The process could not be started.
    #[error("the model server could not be started: {detail}")]
    SpawnFailed { detail: String },
    /// The process ended while it started.
    #[error("the model server stopped while starting ({exit}); see {}", log.display())]
    ExitedWhileStarting { exit: String, log: PathBuf },
    /// The server did not become healthy in time.
    #[error("the model server did not answer within {seconds} s; see {}", log.display())]
    HealthTimeout { seconds: u64, log: PathBuf },
    /// The server crashed again and again.
    #[error("the model server crashed {restarts} times; see {}", log.display())]
    Crashed { restarts: u32, log: PathBuf },
}
