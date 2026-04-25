//! WebSocket event broadcaster.
//!
//! [`EventBus`] distributes [`Event`]s to all connected browser sessions.
//! The server emits events from the [`crate::state::ServerState`] mutation
//! methods; connected WebSocket handlers subscribe and forward them as JSON.

use serde::Serialize;
use tokio::sync::broadcast;
use crate::state::{ClientInfo, Group, StreamStatus};

/// All events that can be pushed to connected web UI clients.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    ClientConnected    { client: ClientInfo },
    ClientDisconnected { client_id: String },
    VolumeChanged      { client_id: String, volume: u8, muted: bool },
    LatencyChanged     { client_id: String, latency_ms: i32 },
    ClientGroupChanged { client_id: String, group_id: String },
    GroupCreated       { group: Group },
    GroupDeleted       { group_id: String },
    GroupStreamChanged { group_id: String, stream_id: String },
    StreamStatus       { stream_id: String, status: StreamStatus },
    /// Emitted periodically so the UI can show server uptime / clock.
    Heartbeat          { uptime_s: i64 },
}

/// Broadcast channel wrapper for [`Event`]s.
///
/// Clone cheaply — all clones share the same underlying channel.
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    /// Emit an event to all active subscribers.
    pub fn emit(&self, event: Event) {
        // Ignore if no subscribers (normal at startup)
        let _ = self.sender.send(event);
    }

    /// Subscribe to receive future events.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self { Self::new() }
}
