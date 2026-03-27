//! Application runtime — orchestrates the command/event loop.
//!
//! The [`AppRuntime`] owns the command and event channels, manages the
//! application lifecycle state, and dispatches incoming commands to the
//! appropriate subsystems.

use crate::bus::{
    CommandReceiver, CommandSender, EventBroadcaster, EventSubscriber, command_channel,
    event_channel,
};
use crate::{AppContext, AppState, PrxvcError};
use prxvc_protocol::{Command, Event};

/// Application runtime — orchestrates the command/event loop.
pub struct AppRuntime {
    /// Current lifecycle state.
    state: AppState,
    /// Shared application context.
    _context: AppContext,
    /// Sender side of the command channel (cloned out to callers).
    command_tx: CommandSender,
    /// Receiver side of the command channel (consumed by the runtime loop).
    command_rx: CommandReceiver,
    /// Broadcaster for events emitted by the runtime.
    event_tx: EventBroadcaster,
}

impl AppRuntime {
    /// Create a new runtime with the given application context.
    ///
    /// The runtime starts in the [`AppState::Booting`] state. Command and
    /// event channels are created with a buffer of 256 each.
    pub fn new(context: AppContext) -> Self {
        let (command_tx, command_rx) = command_channel(256);
        let (event_tx, _initial_sub) = event_channel(256);

        Self {
            state: AppState::Booting,
            _context: context,
            command_tx,
            command_rx,
            event_tx,
        }
    }

    /// Clone the command sender so that UI/FFI layers can dispatch commands.
    pub fn command_sender(&self) -> CommandSender {
        self.command_tx.clone()
    }

    /// Create a new event subscriber from the internal broadcaster.
    pub fn event_subscriber(&self) -> EventSubscriber {
        self.event_tx.subscribe()
    }

    /// Return the current application lifecycle state.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Start the runtime, transitioning to [`AppState::Ready`] and
    /// broadcasting an [`Event::AppReady`] event.
    pub fn start(&mut self) -> Result<(), PrxvcError> {
        self.state = AppState::Ready;
        let _count = self.event_tx.broadcast(Event::AppReady)?;
        Ok(())
    }

    /// Shut down the runtime, resetting the state to [`AppState::Booting`].
    pub fn shutdown(&mut self) -> Result<(), PrxvcError> {
        self.state = AppState::Booting;
        Ok(())
    }

    /// Process a single command, updating state and broadcasting events.
    pub fn process_command(&mut self, command: Command) -> Result<(), PrxvcError> {
        match command {
            Command::InitializeApp => {
                self.state = AppState::Ready;
                let _count = self.event_tx.broadcast(Event::AppReady)?;
            }
            Command::ShutdownApp => {
                self.shutdown()?;
            }
            other => {
                tracing::debug!(?other, "unhandled command, broadcasting as error event");
                let _count = self.event_tx.broadcast(Event::ErrorOccurred {
                    code: "core.unhandled_command".to_owned(),
                    message: format!("command not yet implemented: {other:?}"),
                    recoverable: true,
                })?;
            }
        }
        Ok(())
    }

    /// Receive the next command from the channel.
    ///
    /// Returns `None` when all command senders have been dropped.
    pub async fn recv_command(&mut self) -> Option<Command> {
        self.command_rx.recv().await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_context() -> AppContext {
        AppContext {
            config_path: PathBuf::from("/tmp/test-config"),
            data_path: PathBuf::from("/tmp/test-data"),
            is_debug: true,
        }
    }

    #[test]
    fn test_runtime_new_starts_booting() {
        let rt = AppRuntime::new(test_context());
        assert_eq!(*rt.state(), AppState::Booting);
    }

    #[test]
    fn test_runtime_start_transitions_to_ready() {
        let mut rt = AppRuntime::new(test_context());
        rt.start().expect("start failed");
        assert_eq!(*rt.state(), AppState::Ready);
    }

    #[test]
    fn test_runtime_shutdown() {
        let mut rt = AppRuntime::new(test_context());
        rt.start().expect("start failed");
        rt.shutdown().expect("shutdown failed");
        assert_eq!(*rt.state(), AppState::Booting);
    }

    #[tokio::test]
    async fn test_runtime_process_initialize_command() {
        let mut rt = AppRuntime::new(test_context());
        let mut sub = rt.event_subscriber();

        rt.process_command(Command::InitializeApp)
            .expect("process_command failed");
        assert_eq!(*rt.state(), AppState::Ready);

        let event = sub.recv().await.expect("recv failed");
        assert_eq!(event, Event::AppReady);
    }
}
