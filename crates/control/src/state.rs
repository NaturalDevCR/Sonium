//! In-memory server state — clients, groups, streams.
//!
//! [`ServerState`] is the single source of truth for everything the web UI
//! and REST API read and modify.  It is stored in an `Arc<ServerState>` and
//! shared across all Tokio tasks.
//!
//! Every mutating operation emits a [`crate::ws::Event`] so connected browser
//! sessions stay in sync in real time, and (when a `PersistenceStore` is
//! provided) also saves to `sonium-state.json` so the state survives restarts.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::persistence::{PersistedClient, PersistedGroup, PersistenceStore};
use crate::ws::EventBus;
use sonium_protocol::messages::EqBand;

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
    pub id:           String,
    /// Human-readable hostname.
    pub hostname:     String,
    /// Client application name (e.g. `"Sonium"`, `"Snapclient"`).
    pub client_name:  String,
    /// Operating system string.
    pub os:           String,
    /// CPU architecture.
    pub arch:         String,
    /// Remote socket address (IP:port of the TCP connection).
    pub remote_addr:  String,
    /// Volume (0–100).
    pub volume:       u8,
    /// Whether the client is muted.
    pub muted:        bool,
    /// Extra latency offset in ms (for Bluetooth / HDMI compensation).
    pub latency_ms:   i32,
    /// Group this client belongs to (empty string = default group).
    pub group_id:     String,
    /// Connection status.
    pub status:       ClientStatus,
    /// When the client last connected.
    pub connected_at: DateTime<Utc>,
    /// Protocol version reported in `Hello`.
    pub protocol_version: u32,
    /// Optional operator-assigned display name (shown instead of hostname).
    #[serde(default)]
    pub display_name: Option<String>,
    /// Per-client EQ bands (empty = flat).
    #[serde(default)]
    pub eq_bands: Vec<EqBand>,
}

impl ClientInfo {
    pub fn is_connected(&self) -> bool {
        self.status == ClientStatus::Connected
    }

    /// The name to display in the UI — prefers `display_name` over `hostname`.
    pub fn label(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.hostname)
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
    pub id:           String,
    pub display_name: Option<String>,
    pub codec:        String,
    pub format:       String,
    pub status:       StreamStatus,
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
    clients:           RwLock<HashMap<String, ClientInfo>>,
    groups:            RwLock<HashMap<String, Group>>,
    streams:           RwLock<HashMap<String, StreamInfo>>,
    events:            Arc<EventBus>,
    start_time:        DateTime<Utc>,
    persistence:       Option<Arc<PersistenceStore>>,
    /// Snapshot loaded at startup; used to restore per-client settings on reconnect.
    saved_clients:     Vec<PersistedClient>,
}

impl ServerState {
    pub fn new(
        events:        Arc<EventBus>,
        persistence:   Option<Arc<PersistenceStore>>,
        saved_clients: Vec<PersistedClient>,
    ) -> Self {
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
            id:           "default".into(),
            display_name: None,
            codec:        "opus".into(),
            format:       "48000Hz/16bit/2ch".into(),
            status:       StreamStatus::Idle,
        });

        Self {
            clients:       RwLock::new(HashMap::new()),
            groups:        RwLock::new(groups),
            streams:       RwLock::new(streams),
            events,
            start_time:    Utc::now(),
            persistence,
            saved_clients,
        }
    }

    /// Restore persisted groups (call before accepting any client connections).
    pub fn restore_groups(&self, persisted: Vec<PersistedGroup>) {
        let mut groups = self.groups.write();
        for pg in persisted {
            groups.entry(pg.id.clone()).or_insert_with(|| Group {
                id:         pg.id,
                name:       pg.name,
                stream_id:  pg.stream_id,
                client_ids: vec![],
            });
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    fn persist(&self) {
        let Some(store) = &self.persistence else { return };
        let groups: Vec<PersistedGroup> = self.groups.read().values().map(|g| PersistedGroup {
            id:        g.id.clone(),
            name:      g.name.clone(),
            stream_id: g.stream_id.clone(),
        }).collect();
        let clients: Vec<PersistedClient> = self.clients.read().values().map(|c| PersistedClient {
            id:           c.id.clone(),
            hostname:     c.hostname.clone(),
            display_name: c.display_name.clone(),
            volume:       c.volume,
            muted:        c.muted,
            latency_ms:   c.latency_ms,
            group_id:     c.group_id.clone(),
            eq_bands:     c.eq_bands.clone(),
            last_seen:    Utc::now(),
        }).collect();
        store.save(&groups, &clients);
    }

    // ── Client CRUD ───────────────────────────────────────────────────────

    /// Register a newly connected client, restoring persisted settings if available.
    #[allow(clippy::too_many_arguments)]
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
        let id       = id.into();
        let hostname = hostname.into();

        // Restore settings from the last persisted snapshot for this client ID.
        let saved = self.saved_clients.iter().find(|c| c.id == id);

        let (volume, muted, latency_ms, group_id, display_name, eq_bands): (u8, bool, i32, String, Option<String>, Vec<EqBand>) = if let Some(s) = saved {
            (s.volume, s.muted, s.latency_ms, s.group_id.clone(), s.display_name.clone(), s.eq_bands.clone())
        } else {
            (100, false, 0, "default".into(), None, vec![])
        };

        let info = ClientInfo {
            id:           id.clone(),
            hostname:     hostname.clone(),
            client_name:  client_name.into(),
            os:           os.into(),
            arch:         arch.into(),
            remote_addr:  addr.to_string(),
            volume,
            muted,
            latency_ms,
            group_id:     group_id.clone(),
            status:       ClientStatus::Connected,
            connected_at: Utc::now(),
            protocol_version,
            display_name,
            eq_bands,
        };

        // Place into the correct group (restored or default).
        {
            let mut groups = self.groups.write();
            // Remove from any group that already lists this client (stale from previous session).
            for g in groups.values_mut() {
                g.client_ids.retain(|cid| cid != &id);
            }
            let target = if groups.contains_key(&group_id) { group_id.clone() } else { "default".into() };
            if let Some(grp) = groups.get_mut(&target) {
                grp.client_ids.push(id.clone());
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

    /// Permanently remove a disconnected client from the registry.
    /// Returns `false` if the client is not found or is still connected.
    pub fn delete_client(&self, client_id: &str) -> bool {
        let mut clients = self.clients.write();
        match clients.get(client_id) {
            None => return false,
            Some(c) if c.is_connected() => return false,
            _ => {}
        }
        let info = clients.remove(client_id).unwrap();
        // Remove from its group.
        if let Some(g) = self.groups.write().get_mut(&info.group_id) {
            g.client_ids.retain(|id| id != client_id);
        }
        self.events.emit(crate::ws::Event::ClientDeleted { client_id: client_id.into() });
        self.persist();
        true
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
        drop(clients);
        self.persist();
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
            drop(clients);
            self.persist();
            true
        } else {
            false
        }
    }

    /// Update the EQ bands for a client and push to connected sessions.
    pub fn set_eq(&self, client_id: &str, eq_bands: Vec<EqBand>) -> bool {
        let mut clients = self.clients.write();
        if let Some(c) = clients.get_mut(client_id) {
            c.eq_bands = eq_bands.clone();
            self.events.emit(crate::ws::Event::EqChanged {
                client_id: client_id.into(),
                eq_bands,
            });
            drop(clients);
            self.persist();
            true
        } else {
            false
        }
    }

    /// Read the EQ bands for a client.
    pub fn get_eq(&self, client_id: &str) -> Option<Vec<EqBand>> {
        self.clients.read().get(client_id).map(|c| c.eq_bands.clone())
    }

    /// Set an operator-assigned display name for a client.
    pub fn set_client_name(&self, client_id: &str, display_name: Option<String>) -> bool {
        let mut clients = self.clients.write();
        if let Some(c) = clients.get_mut(client_id) {
            c.display_name = display_name.clone();
            self.events.emit(crate::ws::Event::ClientRenamed {
                client_id:    client_id.into(),
                display_name: display_name.unwrap_or_default(),
            });
            drop(clients);
            self.persist();
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
        drop(clients);
        drop(groups);
        self.persist();
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
        self.persist();
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
            drop(groups);
            drop(clients);
            self.persist();
            true
        } else {
            false
        }
    }

    /// Rename a group.  Returns `false` if the group is not found.
    pub fn rename_group(&self, group_id: &str, new_name: impl Into<String>) -> bool {
        let mut groups = self.groups.write();
        if let Some(g) = groups.get_mut(group_id) {
            let name = new_name.into();
            g.name = name.clone();
            self.events.emit(crate::ws::Event::GroupRenamed {
                group_id: group_id.into(),
                name,
            });
            drop(groups);
            self.persist();
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
            drop(groups);
            self.persist();
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

    /// Fast accessor for volume/mute — avoids cloning the full `ClientInfo`.
    pub fn get_volume(&self, client_id: &str) -> Option<(u8, bool)> {
        let clients = self.clients.read();
        clients.get(client_id).map(|c| (c.volume, c.muted))
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

    /// Returns the stream_id currently assigned to a client's group.
    pub fn client_stream_id(&self, client_id: &str) -> Option<String> {
        let group_id  = self.clients.read().get(client_id)?.group_id.clone();
        let stream_id = self.groups.read().get(&group_id)?.stream_id.clone();
        Some(stream_id)
    }

    /// Register a new stream in the state (idempotent — updates status if already exists).
    pub fn register_stream(
        &self,
        id:           impl Into<String>,
        display_name: Option<String>,
        codec:        impl Into<String>,
        format:       impl Into<String>,
    ) {
        let id = id.into();
        let codec = codec.into();
        let format = format.into();
        let mut streams = self.streams.write();
        streams
            .entry(id.clone())
            .and_modify(|stream| {
                stream.display_name = display_name.clone();
                stream.codec = codec.clone();
                stream.format = format.clone();
            })
            .or_insert_with(|| StreamInfo {
                id: id.clone(),
                display_name,
                codec,
                format,
                status: StreamStatus::Idle,
            });
    }

    pub fn unregister_stream(&self, id: &str) {
        self.streams.write().remove(id);
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
        Arc::new(ServerState::new(Arc::new(EventBus::new()), None, vec![]))
    }

    fn addr() -> SocketAddr { "127.0.0.1:50000".parse().unwrap() }

    fn connect(s: &ServerState, id: &str) {
        s.client_connected(id, "pi", "Sonium", "linux", "aarch64", addr(), 2);
    }

    #[test]
    fn client_connect_appears_in_list() {
        let s = state();
        connect(&s, "pi-1");
        let clients = s.all_clients();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].id, "pi-1");
        assert!(clients[0].is_connected());
    }

    #[test]
    fn client_added_to_default_group() {
        let s = state();
        connect(&s, "pi-1");
        let grp = s.get_group("default").unwrap();
        assert!(grp.client_ids.contains(&"pi-1".to_string()));
    }

    #[test]
    fn client_disconnect_changes_status() {
        let s = state();
        connect(&s, "pi-1");
        s.client_disconnected("pi-1");
        assert!(!s.get_client("pi-1").unwrap().is_connected());
    }

    #[test]
    fn set_volume_updates_client() {
        let s = state();
        connect(&s, "pi-1");
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
        connect(&s, "pi-1");
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

    #[test]
    fn rename_group_works() {
        let s = state();
        assert!(s.rename_group("default", "Living Room"));
        assert_eq!(s.get_group("default").unwrap().name, "Living Room");
    }

    #[test]
    fn rename_group_unknown_returns_false() {
        let s = state();
        assert!(!s.rename_group("ghost", "Anything"));
    }

    #[test]
    fn delete_disconnected_client() {
        let s = state();
        connect(&s, "pi-1");
        s.client_disconnected("pi-1");
        assert!(s.delete_client("pi-1"));
        assert!(s.get_client("pi-1").is_none());
        assert!(!s.get_group("default").unwrap().client_ids.contains(&"pi-1".to_string()));
    }

    #[test]
    fn cannot_delete_connected_client() {
        let s = state();
        connect(&s, "pi-1");
        assert!(!s.delete_client("pi-1"));
    }

    #[test]
    fn set_client_name() {
        let s = state();
        connect(&s, "pi-1");
        assert!(s.set_client_name("pi-1", Some("Living Room Speaker".into())));
        assert_eq!(s.get_client("pi-1").unwrap().display_name.as_deref(), Some("Living Room Speaker"));
    }

    #[test]
    fn client_restored_from_persisted_state() {
        let saved_clients = vec![PersistedClient {
            id:           "pi-1".into(),
            hostname:     "pi".into(),
            display_name: Some("Kitchen".into()),
            volume:       60,
            muted:        true,
            latency_ms:   50,
            group_id:     "default".into(),
            eq_bands:     vec![],
            last_seen:    Utc::now(),
        }];
        let s = Arc::new(ServerState::new(Arc::new(EventBus::new()), None, saved_clients));
        s.client_connected("pi-1", "pi", "Sonium", "linux", "aarch64", addr(), 2);
        let c = s.get_client("pi-1").unwrap();
        assert_eq!(c.volume, 60);
        assert!(c.muted);
        assert_eq!(c.latency_ms, 50);
        assert_eq!(c.display_name.as_deref(), Some("Kitchen"));
    }
}
