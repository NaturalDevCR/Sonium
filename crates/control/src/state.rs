//! In-memory server state — clients, groups, streams.
//!
//! [`ServerState`] is the single source of truth for everything the web UI
//! and REST API read and modify.  It is stored in an `Arc<ServerState>` and
//! shared across all Tokio tasks.
//!
//! Every mutating operation emits a [`crate::ws::Event`] so connected browser
//! sessions stay in sync in real time.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::ws::EventBus;

// ── Client ────────────────────────────────────────────────────────────────

/// Runtime status of a connected client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientStatus {
    /// TCP connection is active and audio is flowing.
    Connected,
    /// TCP connection dropped; will be marked `Gone` after a timeout.
    Disconnected,
}

/// A client known to the server (either currently connected or recently seen).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Stable unique ID sent in the `Hello` message.
    pub id:         String,
    /// Human-readable hostname.
    pub hostname:   String,
    /// Client application name (e.g. `"Sonium"`, `"Snapclient"`).
    pub client_name: String,
    /// Operating system string.
    pub os:         String,
    /// CPU architecture.
    pub arch:       String,
    /// Remote socket address (IP:port of the TCP connection).
    pub remote_addr: String,
    /// Volume (0–100).
    pub volume:     u8,
    /// Whether the client is muted.
    pub muted:      bool,
    /// Extra latency offset in ms (for Bluetooth / HDMI compensation).
    pub latency_ms: i32,
    /// Group this client belongs to (empty string = default group).
    pub group_id:   String,
    /// Connection status.
    pub status:     ClientStatus,
    /// When the client last connected.
    pub connected_at: DateTime<Utc>,
    /// Protocol version reported in `Hello`.
    pub protocol_version: u32,
}

impl ClientInfo {
    pub fn is_connected(&self) -> bool {
        self.status == ClientStatus::Connected
    }
}

// ── Group ─────────────────────────────────────────────────────────────────

/// A group of clients that all play the same stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// Unique identifier (auto-generated UUID).
    pub id:         String,
    /// Display name shown in the web UI.
    pub name:       String,
    /// The stream this group is playing.
    pub stream_id:  String,
    /// Ordered list of client IDs in this group.
    pub client_ids: Vec<String>,
}

// ── Stream ────────────────────────────────────────────────────────────────

/// An active audio stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub id:     String,
    pub codec:  String,
    pub format: String,
    pub status: StreamStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamStatus {
    Playing,
    Idle,
    Error,
}

// ── ServerState ──────────────────────────────────────────────────────────

/// Thread-safe in-memory state shared between the audio server and the
/// control API.
pub struct ServerState {
    clients: RwLock<HashMap<String, ClientInfo>>,
    groups:  RwLock<HashMap<String, Group>>,
    streams: RwLock<HashMap<String, StreamInfo>>,
    events:  Arc<EventBus>,
    start_time: DateTime<Utc>,
}

impl ServerState {
    pub fn new(events: Arc<EventBus>) -> Self {
        let mut groups  = HashMap::new();
        let default_grp = Group {
            id:         "default".into(),
            name:       "Default".into(),
            stream_id:  "default".into(),
            client_ids: vec![],
        };
        groups.insert("default".into(), default_grp);

        let mut streams = HashMap::new();
        streams.insert("default".into(), StreamInfo {
            id:     "default".into(),
            codec:  "opus".into(),
            format: "48000Hz/16bit/2ch".into(),
            status: StreamStatus::Idle,
        });

        Self {
            clients:    RwLock::new(HashMap::new()),
            groups:     RwLock::new(groups),
            streams:    RwLock::new(streams),
            events,
            start_time: Utc::now(),
        }
    }

    // ── Client CRUD ───────────────────────────────────────────────────────

    /// Register a newly connected client.
    pub fn client_connected(
        &self,
        id:          impl Into<String>,
        hostname:    impl Into<String>,
        client_name: impl Into<String>,
        os:          impl Into<String>,
        arch:        impl Into<String>,
        addr:        SocketAddr,
        protocol_version: u32,
    ) {
        let id = id.into();
        let info = ClientInfo {
            id:           id.clone(),
            hostname:     hostname.into(),
            client_name:  client_name.into(),
            os:           os.into(),
            arch:         arch.into(),
            remote_addr:  addr.to_string(),
            volume:       100,
            muted:        false,
            latency_ms:   0,
            group_id:     "default".into(),
            status:       ClientStatus::Connected,
            connected_at: Utc::now(),
            protocol_version,
        };

        // Add to default group
        {
            let mut groups = self.groups.write();
            if let Some(grp) = groups.get_mut("default") {
                if !grp.client_ids.contains(&id) {
                    grp.client_ids.push(id.clone());
                }
            }
        }

        self.clients.write().insert(id.clone(), info.clone());
        self.events.emit(crate::ws::Event::ClientConnected { client: info });
    }

    /// Mark a client as disconnected (keeps history in the registry).
    pub fn client_disconnected(&self, id: &str) {
        let mut clients = self.clients.write();
        if let Some(c) = clients.get_mut(id) {
            c.status = ClientStatus::Disconnected;
            self.events.emit(crate::ws::Event::ClientDisconnected { client_id: id.into() });
        }
    }

    /// Update volume and/or mute for a client, push event, return the new state.
    pub fn set_volume(&self, client_id: &str, volume: u8, muted: bool)
        -> Option<(u8, bool)>
    {
        let mut clients = self.clients.write();
        let c = clients.get_mut(client_id)?;
        c.volume = volume;
        c.muted  = muted;
        self.events.emit(crate::ws::Event::VolumeChanged {
            client_id: client_id.into(),
            volume,
            muted,
        });
        Some((volume, muted))
    }

    /// Update the latency offset for a client.
    pub fn set_latency(&self, client_id: &str, latency_ms: i32) -> bool {
        let mut clients = self.clients.write();
        if let Some(c) = clients.get_mut(client_id) {
            c.latency_ms = latency_ms;
            self.events.emit(crate::ws::Event::LatencyChanged {
                client_id: client_id.into(),
                latency_ms,
            });
            true
        } else {
            false
        }
    }

    /// Move a client to a different group.
    pub fn set_client_group(&self, client_id: &str, group_id: &str) -> bool {
        let mut clients = self.clients.write();
        let mut groups  = self.groups.write();

        let client = match clients.get_mut(client_id) {
            Some(c) => c,
            None    => return false,
        };
        if !groups.contains_key(group_id) { return false; }

        // Remove from old group
        if let Some(old_grp) = groups.get_mut(&client.group_id) {
            old_grp.client_ids.retain(|id| id != client_id);
        }
        // Add to new group
        if let Some(new_grp) = groups.get_mut(group_id) {
            if !new_grp.client_ids.contains(&client_id.to_string()) {
                new_grp.client_ids.push(client_id.into());
            }
        }
        client.group_id = group_id.into();
        self.events.emit(crate::ws::Event::ClientGroupChanged {
            client_id: client_id.into(),
            group_id:  group_id.into(),
        });
        true
    }

    // ── Group CRUD ────────────────────────────────────────────────────────

    /// Create a new group and return its generated ID.
    pub fn create_group(&self, name: impl Into<String>, stream_id: impl Into<String>) -> String {
        let id  = uuid::Uuid::new_v4().to_string();
        let grp = Group {
            id:         id.clone(),
            name:       name.into(),
            stream_id:  stream_id.into(),
            client_ids: vec![],
        };
        self.groups.write().insert(id.clone(), grp.clone());
        self.events.emit(crate::ws::Event::GroupCreated { group: grp });
        id
    }

    /// Delete a group; clients in the group are moved to "default".
    pub fn delete_group(&self, group_id: &str) -> bool {
        if group_id == "default" { return false; }
        let mut groups  = self.groups.write();
        let mut clients = self.clients.write();

        if let Some(grp) = groups.remove(group_id) {
            for cid in &grp.client_ids {
                if let Some(c) = clients.get_mut(cid) {
                    c.group_id = "default".into();
                    if let Some(default) = groups.get_mut("default") {
                        default.client_ids.push(cid.clone());
                    }
                }
            }
            self.events.emit(crate::ws::Event::GroupDeleted { group_id: group_id.into() });
            true
        } else {
            false
        }
    }

    /// Change which stream a group is playing.
    pub fn set_group_stream(&self, group_id: &str, stream_id: &str) -> bool {
        let mut groups = self.groups.write();
        if !self.streams.read().contains_key(stream_id) { return false; }
        if let Some(g) = groups.get_mut(group_id) {
            g.stream_id = stream_id.into();
            self.events.emit(crate::ws::Event::GroupStreamChanged {
                group_id:  group_id.into(),
                stream_id: stream_id.into(),
            });
            true
        } else {
            false
        }
    }

    // ── Stream management ─────────────────────────────────────────────────

    pub fn set_stream_status(&self, stream_id: &str, status: StreamStatus) {
        let mut streams = self.streams.write();
        if let Some(s) = streams.get_mut(stream_id) {
            s.status = status.clone();
            self.events.emit(crate::ws::Event::StreamStatus {
                stream_id: stream_id.into(),
                status,
            });
        }
    }

    // ── Read access ───────────────────────────────────────────────────────

    pub fn all_clients(&self) -> Vec<ClientInfo> {
        self.clients.read().values().cloned().collect()
    }

    pub fn connected_clients(&self) -> Vec<ClientInfo> {
        self.clients.read().values()
            .filter(|c| c.is_connected())
            .cloned()
            .collect()
    }

    pub fn get_client(&self, id: &str) -> Option<ClientInfo> {
        self.clients.read().get(id).cloned()
    }

    pub fn all_groups(&self) -> Vec<Group> {
        self.groups.read().values().cloned().collect()
    }

    pub fn get_group(&self, id: &str) -> Option<Group> {
        self.groups.read().get(id).cloned()
    }

    pub fn all_streams(&self) -> Vec<StreamInfo> {
        self.streams.read().values().cloned().collect()
    }

    pub fn uptime_secs(&self) -> i64 {
        (Utc::now() - self.start_time).num_seconds()
    }

    pub fn events(&self) -> Arc<EventBus> {
        self.events.clone()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Arc<ServerState> {
        Arc::new(ServerState::new(Arc::new(EventBus::new())))
    }

    fn addr() -> SocketAddr { "127.0.0.1:50000".parse().unwrap() }

    #[test]
    fn client_connect_appears_in_list() {
        let s = state();
        s.client_connected("pi-1", "pi", "Sonium", "linux", "aarch64", addr(), 2);
        let clients = s.all_clients();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].id, "pi-1");
        assert!(clients[0].is_connected());
    }

    #[test]
    fn client_added_to_default_group() {
        let s = state();
        s.client_connected("pi-1", "pi", "Sonium", "linux", "aarch64", addr(), 2);
        let grp = s.get_group("default").unwrap();
        assert!(grp.client_ids.contains(&"pi-1".to_string()));
    }

    #[test]
    fn client_disconnect_changes_status() {
        let s = state();
        s.client_connected("pi-1", "pi", "Sonium", "linux", "aarch64", addr(), 2);
        s.client_disconnected("pi-1");
        assert!(!s.get_client("pi-1").unwrap().is_connected());
    }

    #[test]
    fn set_volume_updates_client() {
        let s = state();
        s.client_connected("pi-1", "pi", "Sonium", "linux", "aarch64", addr(), 2);
        s.set_volume("pi-1", 50, true);
        let c = s.get_client("pi-1").unwrap();
        assert_eq!(c.volume, 50);
        assert!(c.muted);
    }

    #[test]
    fn set_volume_unknown_client_returns_none() {
        let s = state();
        assert!(s.set_volume("ghost", 50, false).is_none());
    }

    #[test]
    fn create_and_delete_group() {
        let s  = state();
        let id = s.create_group("Kitchen", "default");
        assert!(s.get_group(&id).is_some());
        assert!(s.delete_group(&id));
        assert!(s.get_group(&id).is_none());
    }

    #[test]
    fn cannot_delete_default_group() {
        let s = state();
        assert!(!s.delete_group("default"));
    }

    #[test]
    fn move_client_between_groups() {
        let s   = state();
        let gid = s.create_group("Bedroom", "default");
        s.client_connected("pi-1", "pi", "Sonium", "linux", "aarch64", addr(), 2);
        assert!(s.set_client_group("pi-1", &gid));
        assert_eq!(s.get_client("pi-1").unwrap().group_id, gid);
        // removed from default
        assert!(!s.get_group("default").unwrap().client_ids.contains(&"pi-1".to_string()));
    }

    #[test]
    fn set_group_stream_unknown_stream_fails() {
        let s = state();
        assert!(!s.set_group_stream("default", "nonexistent"));
    }
}
