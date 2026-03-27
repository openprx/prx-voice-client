//! Command and event bus using tokio channels.
//!
//! Provides a unidirectional command channel (UI/FFI -> runtime) and a
//! broadcast event channel (runtime -> multiple UI/FFI subscribers).

use prxvc_protocol::{Command, Event};
use tokio::sync::{broadcast, mpsc};

use crate::PrxvcError;

// ---------------------------------------------------------------------------
// Command channel
// ---------------------------------------------------------------------------

/// Command sender — UI/FFI sends commands through this.
#[derive(Debug, Clone)]
pub struct CommandSender {
    tx: mpsc::Sender<Command>,
}

impl CommandSender {
    /// Send a command into the runtime pipeline.
    pub async fn send(&self, cmd: Command) -> Result<(), PrxvcError> {
        self.tx
            .send(cmd)
            .await
            .map_err(|e| PrxvcError::Internal(format!("command channel closed: {e}")))
    }
}

/// Command receiver — runtime consumes commands from this.
#[derive(Debug)]
pub struct CommandReceiver {
    rx: mpsc::Receiver<Command>,
}

impl CommandReceiver {
    /// Receive the next command, returning `None` when all senders are dropped.
    pub async fn recv(&mut self) -> Option<Command> {
        self.rx.recv().await
    }
}

/// Create a command channel pair with the given buffer size.
pub fn command_channel(buffer: usize) -> (CommandSender, CommandReceiver) {
    let (tx, rx) = mpsc::channel(buffer);
    (CommandSender { tx }, CommandReceiver { rx })
}

// ---------------------------------------------------------------------------
// Event channel
// ---------------------------------------------------------------------------

/// Event broadcaster — runtime broadcasts events through this.
#[derive(Debug, Clone)]
pub struct EventBroadcaster {
    tx: broadcast::Sender<Event>,
}

impl EventBroadcaster {
    /// Broadcast an event to all current subscribers.
    ///
    /// Returns the number of receivers that received the event. If there are
    /// no active subscribers the event is silently dropped and `0` is returned.
    pub fn broadcast(&self, event: Event) -> Result<usize, PrxvcError> {
        match self.tx.send(event) {
            Ok(n) => Ok(n),
            // No active receivers — not an error, just nobody listening yet.
            Err(_) if self.tx.receiver_count() == 0 => Ok(0),
            Err(e) => Err(PrxvcError::Internal(format!("event broadcast failed: {e}"))),
        }
    }

    /// Create a new subscriber that will receive future events.
    pub fn subscribe(&self) -> EventSubscriber {
        EventSubscriber {
            rx: self.tx.subscribe(),
        }
    }
}

/// Event subscriber — UI/FFI subscribes to events through this.
#[derive(Debug)]
pub struct EventSubscriber {
    rx: broadcast::Receiver<Event>,
}

impl EventSubscriber {
    /// Receive the next event from the broadcast channel.
    pub async fn recv(&mut self) -> Result<Event, PrxvcError> {
        self.rx
            .recv()
            .await
            .map_err(|e| PrxvcError::Internal(format!("event receive failed: {e}")))
    }
}

/// Create an event broadcast channel with the given buffer size.
pub fn event_channel(buffer: usize) -> (EventBroadcaster, EventSubscriber) {
    let (tx, _rx) = broadcast::channel(buffer);
    let subscriber = EventSubscriber { rx: tx.subscribe() };
    (EventBroadcaster { tx }, subscriber)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_command_channel_send_recv() {
        let (tx, mut rx) = command_channel(16);
        tx.send(Command::InitializeApp).await.expect("send failed");
        let cmd = rx.recv().await.expect("recv returned None");
        assert_eq!(cmd, Command::InitializeApp);
    }

    #[tokio::test]
    async fn test_event_broadcast_multiple_subscribers() {
        let (broadcaster, mut sub1) = event_channel(16);
        let mut sub2 = broadcaster.subscribe();

        let count = broadcaster
            .broadcast(Event::AppReady)
            .expect("broadcast failed");
        assert_eq!(count, 2);

        let e1 = sub1.recv().await.expect("sub1 recv failed");
        let e2 = sub2.recv().await.expect("sub2 recv failed");
        assert_eq!(e1, Event::AppReady);
        assert_eq!(e2, Event::AppReady);
    }

    #[tokio::test]
    async fn test_event_subscriber_from_broadcaster() {
        let (broadcaster, _initial_sub) = event_channel(16);
        let mut sub = broadcaster.subscribe();

        broadcaster
            .broadcast(Event::AppReady)
            .expect("broadcast failed");
        let event = sub.recv().await.expect("recv failed");
        assert_eq!(event, Event::AppReady);
    }
}
