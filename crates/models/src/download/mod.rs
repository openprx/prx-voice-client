//! Download engine: installs one model's files from a remote source with
//! resume, retry, verification and atomic promotion (design 4.2). This file
//! only wires the settings and the sub-modules together.
//!
//! - [`http`]: one ranged request streamed into a partial file.
//! - [`retry`]: backoff schedule and sleeper.
//! - [`file`]: the per-file download loop (pieces, retries, redirects).
//! - [`verify`]: SHA-256 check of a finished file.
//! - [`job`]: the per-model procedure (adopt, disk check, files, promote).

mod file;
mod http;
mod job;
pub mod retry;
mod verify;

use std::sync::Arc;
use std::time::Duration;

use crate::catalogue::ModelEntry;
use crate::net::{AllowAll, Endpoint, NetworkPolicy};
use crate::store::FreeSpaceFn;
use crate::types::FailReason;

pub use retry::{BackoffPolicy, Sleeper, real_sleeper};

pub(crate) const PIECE_SIZE: u64 = 8 * 1024 * 1024;

/// Tunables of the engine; the defaults are the values of the design.
#[derive(Clone)]
pub struct EngineSettings {
    pub backoff: BackoffPolicy,
    pub sleeper: Sleeper,
    pub piece_size: u64,
    pub connect_timeout: Duration,
    /// Longest time one request (an 8 MiB piece, headers and body) may take.
    /// The blocking client has no separate first-byte timeout.
    pub piece_timeout: Duration,
    pub user_agent: String,
    /// Overrides the store's free-space probe when set.
    pub free_space: Option<FreeSpaceFn>,
    pub policy: Arc<dyn NetworkPolicy>,
}

impl Default for EngineSettings {
    fn default() -> Self {
        EngineSettings {
            backoff: BackoffPolicy::default(),
            sleeper: real_sleeper(),
            piece_size: PIECE_SIZE,
            connect_timeout: Duration::from_secs(15),
            piece_timeout: Duration::from_secs(300),
            user_agent: format!("prx-voice-client/{}", env!("CARGO_PKG_VERSION")),
            free_space: None,
            policy: Arc::new(AllowAll),
        }
    }
}

/// What to install.
pub struct JobSpec<'a> {
    pub entry: &'a ModelEntry,
    pub catalogue_version: u64,
    pub endpoint: &'a Endpoint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobOutcome {
    /// Every file verified and the model promoted (or it already was).
    Installed,
    /// Stopped on request; the partial is kept.
    Paused,
    /// Stopped on request; the partial was deleted.
    Cancelled,
    Failed(FailReason),
}

pub struct DownloadEngine {
    pub(crate) settings: EngineSettings,
    pub(crate) http: http::HttpClient,
}

impl DownloadEngine {
    pub fn new(settings: EngineSettings) -> Result<DownloadEngine, String> {
        let http = http::HttpClient::new(
            &http::HttpSettings {
                connect_timeout: settings.connect_timeout,
                piece_timeout: settings.piece_timeout,
                user_agent: settings.user_agent.clone(),
            },
            settings.policy.clone(),
        )?;
        Ok(DownloadEngine { settings, http })
    }

    /// Single request to an endpoint root; true if anything answers. Used for
    /// the automatic retry of jobs that failed as offline.
    pub fn probe(&self, url: &str) -> bool {
        self.http.probe(url)
    }

    /// Needed free space for `remaining` bytes: 5 % more plus 512 MiB.
    pub fn space_needed(remaining: u64) -> u64 {
        remaining + remaining / 20 + 512 * 1024 * 1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_needed_formula() {
        assert_eq!(DownloadEngine::space_needed(0), 512 * 1024 * 1024);
        assert_eq!(DownloadEngine::space_needed(2000), 2100 + 512 * 1024 * 1024);
    }
}
