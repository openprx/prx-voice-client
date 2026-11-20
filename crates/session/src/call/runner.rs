//! The coordinator's thread: a command channel in, snapshots out through the coordinator's sink, a bounded join on shutdown.
//!
//! One thread owns the [`Coordinator`] (and with it the host, the paths and
//! both direction machines) from creation to the end: other threads only send
//! [`CallCommand`]s through a bounded channel and read published snapshots. The
//! coordinator is built on its own thread from a factory, because the host may
//! hold system handles that must not move between threads.

use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::coordinator::{Coordinator, CoordinatorDeps};
use super::snapshot::CallCommand;

/// Commands that can wait in the channel; a sender that finds it full is told so.
const COMMAND_CAPACITY: usize = 64;

/// Time between ticks while no command arrives.
pub const TICK_INTERVAL: Duration = Duration::from_millis(250);

enum Input {
    Command(CallCommand),
    Shutdown,
}

/// A command could not be queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// The channel is full: the coordinator is busy (a start waiting for a prompt, for example).
    #[error("the call coordinator is busy")]
    Busy,
    /// The coordinator has ended.
    #[error("the call coordinator has ended")]
    Ended,
}

/// The coordinator thread did not end in time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the call coordinator thread did not end within the time allowed")]
pub struct ShutdownTimeout;

/// The handle on a running coordinator thread.
pub struct CallHandle {
    tx: SyncSender<Input>,
    thread: Option<JoinHandle<()>>,
}

impl CallHandle {
    /// Start the thread. `build` runs on it and makes the coordinator's dependencies.
    ///
    /// # Errors
    /// The operating-system error when the thread cannot be started.
    pub fn spawn<F>(build: F) -> std::io::Result<Self>
    where
        F: FnOnce() -> CoordinatorDeps + Send + 'static,
    {
        let (tx, rx) = sync_channel(COMMAND_CAPACITY);
        let thread = std::thread::Builder::new()
            .name("prxvc-call-coordinator".into())
            .spawn(move || run(Coordinator::new(build()), &rx))?;
        Ok(Self {
            tx,
            thread: Some(thread),
        })
    }

    /// Queue a command; never blocks.
    ///
    /// # Errors
    /// [`SendError`] when the channel is full or the coordinator has ended.
    pub fn send(&self, command: CallCommand) -> Result<(), SendError> {
        self.tx.try_send(Input::Command(command)).map_err(|e| match e {
            TrySendError::Full(_) => SendError::Busy,
            TrySendError::Disconnected(_) => SendError::Ended,
        })
    }

    /// Ask the coordinator to stop call mode and end, and wait up to `timeout` for the thread.
    ///
    /// # Errors
    /// [`ShutdownTimeout`] when the thread is still running; it is then detached.
    pub fn shutdown(&mut self, timeout: Duration) -> Result<(), ShutdownTimeout> {
        // A full channel still ends when the sender drops, so a failed send is not an error here.
        let _ = self.tx.try_send(Input::Shutdown);
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        let deadline = Instant::now() + timeout;
        while !thread.is_finished() {
            if Instant::now() >= deadline {
                tracing::error!("call coordinator thread did not end in time; detached");
                return Err(ShutdownTimeout);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if thread.join().is_err() {
            tracing::error!("call coordinator thread panicked");
        }
        Ok(())
    }
}

impl Drop for CallHandle {
    fn drop(&mut self) {
        // A timeout is logged by `shutdown`; a drop has nobody to tell.
        let _ = self.shutdown(Duration::from_secs(5));
    }
}

fn run(mut coordinator: Coordinator, rx: &Receiver<Input>) {
    loop {
        match rx.recv_timeout(TICK_INTERVAL) {
            Ok(Input::Command(command)) => coordinator.handle(command),
            Ok(Input::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                coordinator.shutdown();
                return;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        coordinator.tick();
    }
}
