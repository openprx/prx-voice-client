//! The record of the server we started, so that a crash of the application never leaves a server
//! nobody owns: the next start finds the record and stops that process, but only if the process
//! is still the one we started.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::ports::{ProcInfo, Processes};

/// What identifies the process: its pid and when and with what it started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ServerRecord {
    pub pid: u32,
    pub started_unix_ms: u64,
    pub port: u16,
    pub alias: String,
    pub binary: PathBuf,
}

/// How far the start time of the live process may differ from the record's, in seconds.
const START_TOLERANCE_SECS: u64 = 10;

impl ServerRecord {
    pub(crate) fn read(path: &Path) -> Option<Self> {
        serde_json::from_slice(&fs::read(path).ok()?).ok()
    }

    pub(crate) fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
        )?;
        fs::rename(tmp, path)
    }

    pub(crate) fn remove(path: &Path) {
        let _ = fs::remove_file(path);
    }

    /// Whether `info` is the process this record describes: a llama-server of this binary, alias
    /// and port that started when the record says. A reused pid fails one of these.
    pub(crate) fn matches(&self, info: &ProcInfo, now_unix_ms: u64) -> bool {
        let binary_name = self
            .binary
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let started_secs_ago = now_unix_ms.saturating_sub(self.started_unix_ms) / 1000;
        !binary_name.is_empty()
            && info.command.contains(&binary_name)
            && info.command.contains(&self.alias)
            && info.command.contains(&format!("--port {}", self.port))
            && info.age_secs.abs_diff(started_secs_ago) <= START_TOLERANCE_SECS
    }
}

/// What the sweep did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sweep {
    /// No record.
    Nothing,
    /// The record was stale (the process is gone or is not ours); only the file was removed.
    Stale,
    /// A server of ours was left behind and was stopped.
    Stopped,
}

/// Stop a server left behind by an earlier run, and remove the record.
pub(crate) fn sweep(path: &Path, procs: &dyn Processes, now_unix_ms: u64) -> Sweep {
    let Some(record) = ServerRecord::read(path) else {
        let existed = path.exists();
        ServerRecord::remove(path);
        return if existed {
            Sweep::Stale
        } else {
            Sweep::Nothing
        };
    };
    let ours = procs
        .describe(record.pid)
        .is_some_and(|info| record.matches(&info, now_unix_ms));
    if ours {
        procs.terminate(record.pid);
        // Give it a moment, then make sure.
        for _ in 0..50 {
            if procs.describe(record.pid).is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if procs.describe(record.pid).is_some() {
            procs.kill(record.pid);
        }
    }
    ServerRecord::remove(path);
    if ours { Sweep::Stopped } else { Sweep::Stale }
}
