//! What the server manager needs from the operating system, as small traits, so that every rule
//! of the manager is tested with a fake launcher, fake probe and fake clock.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

/// How to start the server: the arguments of `scripts/dev/llama-server.sh`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaunchSpec {
    pub binary: PathBuf,
    pub model: PathBuf,
    /// The model name the server answers to (`--alias`).
    pub alias: String,
    pub port: u16,
    /// Parallel slots, one per direction.
    pub parallel: u32,
    pub context: u32,
    /// Layers on the GPU.
    pub gpu_layers: u32,
    /// Where the server's output goes.
    pub log: PathBuf,
}

impl LaunchSpec {
    /// The command line after the binary.
    pub(crate) fn args(&self) -> Vec<String> {
        vec![
            "-m".into(),
            self.model.display().to_string(),
            "--alias".into(),
            self.alias.clone(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            self.port.to_string(),
            "--parallel".into(),
            self.parallel.to_string(),
            "-c".into(),
            self.context.to_string(),
            "-ngl".into(),
            self.gpu_layers.to_string(),
        ]
    }
}

/// A server process we started.
pub(crate) trait Child: Send {
    fn pid(&self) -> u32;
    /// A description of how the process ended, or `None` while it runs.
    fn try_exit(&mut self) -> Option<String>;
    /// Ask it to stop (SIGTERM).
    fn terminate(&mut self);
    /// Stop it now (SIGKILL).
    fn kill(&mut self);
}

/// Starts server processes.
pub(crate) trait Launcher: Send + Sync {
    fn spawn(&self, spec: &LaunchSpec) -> io::Result<Box<dyn Child>>;
}

/// What answers on the port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PortState {
    /// Nothing listens.
    Closed,
    /// A server that is healthy and serves the model alias.
    Compatible,
    /// Something listens that is not such a server (or is still loading: not healthy yet).
    Foreign,
}

/// Looks at the port.
pub(crate) trait Probe: Send + Sync {
    fn probe(&self, port: u16, alias: &str) -> PortState;
    /// Whether `/health` answers (a server of ours that is still loading says no).
    fn healthy(&self, port: u16) -> bool;
}

/// What the process table says about a pid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcInfo {
    pub command: String,
    /// Seconds since the process started.
    pub age_secs: u64,
}

/// The process table, for the sweep of a server left behind.
pub(crate) trait Processes: Send + Sync {
    fn describe(&self, pid: u32) -> Option<ProcInfo>;
    fn terminate(&self, pid: u32);
    fn kill(&self, pid: u32);
}

/// Time. The manager never reads the system clock directly.
pub(crate) trait Clock: Send + Sync {
    /// Monotonic time since an arbitrary start.
    fn now(&self) -> Duration;
    /// Wall-clock milliseconds since the Unix epoch (for the record file).
    fn unix_ms(&self) -> u64;
    /// Wait. A fake clock advances its time instead.
    fn sleep(&self, d: Duration);
}
