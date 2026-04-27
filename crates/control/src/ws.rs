//! WebSocket event broadcaster.
//!
//! [`EventBus`] distributes [`Event`]s to all connected browser sessions.
//! The server emits events from the [`crate::state::ServerState`] mutation
//! methods; connected WebSocket handlers subscribe and forward them as JSON.

use crate::state::{ClientInfo, Group, StreamStatus};
use serde::Serialize;
use sonium_protocol::messages::EqBand;
use tokio::sync::broadcast;

/// All events that can be pushed to connected web UI clients.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    ClientConnected {
        client: ClientInfo,
    },
    ClientDisconnected {
        client_id: String,
    },
    ClientDeleted {
        client_id: String,
    },
    ClientRenamed {
        client_id: String,
        display_name: String,
    },
    VolumeChanged {
        client_id: String,
        volume: u8,
        muted: bool,
    },
    LatencyChanged {
        client_id: String,
        latency_ms: i32,
    },
    ClientGroupChanged {
        client_id: String,
        group_id: String,
    },
    GroupCreated {
        group: Group,
    },
    GroupDeleted {
        group_id: String,
    },
    GroupRenamed {
        group_id: String,
        name: String,
    },
    GroupStreamChanged {
        group_id: String,
        stream_id: String,
    },
    StreamStatus {
        stream_id: String,
        status: StreamStatus,
    },
    StreamRestarted {
        stream_id: String,
    },
    StreamRemoved {
        stream_id: String,
    },
    /// Emitted periodically so the UI can show server uptime / clock.
    Heartbeat {
        uptime_s: i64,
    },
    /// Emitted ~10 times/s so the UI can show a per-stream VU meter.
    StreamLevel {
        stream_id: String,
        rms_db: f32,
    },
    /// Emitted when the operator changes a client's EQ bands.
    EqChanged {
        client_id: String,
        eq_bands: Vec<EqBand>,
    },
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
    fn default() -> Self {
        Self::new()
    }
}
