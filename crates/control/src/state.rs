//! In-memory server state — clients, groups, streams.
//!
//! [`ServerState`] is the single source of truth for everything the web UI
//! and REST API read and modify.  It is stored in an `Arc<ServerState>` and
//! shared across all Tokio tasks.
//!
//! Every mutating operation emits a [`crate::ws::Event`] so connected browser
//! sessions stay in sync in real time, and (when a `PersistenceStore` is
//! provided) also saves to `sonium-state.json` so the state survives restarts.

use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crate::announcement_scheduler::{
    AnnouncementClient, AnnouncementScheduler, SchedulerAdmission, SchedulerConfig, SchedulerEvents,
};
use crate::announcements::{
    AnnouncementAdmission, AnnouncementError, AnnouncementIntent, AnnouncementLifecycle,
    AnnouncementLimits, AnnouncementRecord, AnnouncementTransition,
};
use crate::persistence::{PersistedClient, PersistedGroup, PersistedStream, PersistenceStore};
use crate::ws::EventBus;
use sonium_common::config::validate_client_id;
use sonium_common::SoniumError;
use sonium_protocol::messages::{AnnouncementControlV1, EqBand, HealthReport};
use sonium_transport::TransportMode;
use std::cmp::Reverse;
use tokio::sync::broadcast;

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
    pub id: String,
    /// Human-readable hostname.
    pub hostname: String,
    /// Client application name (e.g. `"Sonium"`, `"Snapclient"`).
    pub client_name: String,
    /// Operating system string.
    pub os: String,
    /// CPU architecture.
    pub arch: String,
    /// Remote socket address (IP:port of the TCP connection).
    pub remote_addr: String,
    /// Volume (0–100).
    pub volume: u8,
    /// Whether the client is muted.
    pub muted: bool,
    /// Extra latency offset in ms (for Bluetooth / HDMI compensation).
    pub latency_ms: i32,
    /// Group this client belongs to (empty string = default group).
    pub group_id: String,
    /// Connection status.
    pub status: ClientStatus,
    /// When the client last connected.
    pub connected_at: DateTime<Utc>,
    /// Protocol version reported in `Hello`.
    pub protocol_version: u32,
    /// Monotonic connection identity used to ignore stale session cleanup.
    #[serde(skip)]
    pub session_generation: u64,
    /// Optional operator-assigned display name (shown instead of hostname).
    #[serde(default)]
    pub display_name: Option<String>,
    /// Whether this client should send diagnostic health reports.
    #[serde(default)]
    pub observability_enabled: bool,
    /// Real-time health metrics.
    pub health: Option<HealthReport>,
    /// Last known NTP clock offset (ms) from health reports — used for group sync.
    #[serde(skip)]
    pub last_clock_offset_ms: Option<i32>,
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
    pub id: String,
    /// Display name shown in the web UI.
    pub name: String,
    /// The stream this group is playing.
    pub stream_id: String,
    /// Ordered list of client IDs in this group.
    pub client_ids: Vec<String>,
}

// ── Stream ───────────────────────────────────────────────────────────────

/// An active audio stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub id: String,
    pub display_name: Option<String>,
    pub codec: String,
    pub format: String,
    pub source: String,
    pub buffer_ms: u32,
    #[serde(default)]
    pub buffer_ms_overridden: bool,
    #[serde(default = "default_chunk_ms")]
    pub chunk_ms: u32,
    #[serde(default)]
    pub chunk_ms_overridden: bool,
    pub idle_timeout_ms: Option<u32>,
    pub silence_on_idle: bool,
    pub status: StreamStatus,
    /// Reopen progress while a file/FIFO source waits for its producer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<StreamRecovery>,
    /// Per-stream EQ bands (empty = flat).
    #[serde(default)]
    pub eq_bands: Vec<EqBand>,
    /// Whether the EQ is enabled for this stream.
    #[serde(default)]
    pub eq_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamStatus {
    Playing,
    Idle,
    Recovering,
    Error,
}

/// Context for a file/FIFO source that is waiting to be reopened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamRecovery {
    /// Number of consecutive reopen attempts since the reader last produced input.
    pub attempt: u32,
    /// Bounded delay before the next reopen attempt.
    pub retry_in_ms: u64,
}

fn default_chunk_ms() -> u32 {
    20
}

// ── Transport ─────────────────────────────────────────────────────────────

/// Runtime-mutable transport configuration held in [`ServerState`].
struct TransportState {
    mode: TransportMode,
    /// Server UDP port for RTP media (`0` = not configured).
    server_udp_port: u16,
}

type StreamStatusHook = dyn Fn(&str, StreamStatus, Option<StreamRecovery>) + Send + Sync + 'static;
type ClientRemovalHook = dyn Fn(&str, u64) + Send + Sync + 'static;

// ── ServerState ──────────────────────────────────────────────────────────

/// Thread-safe in-memory state shared between the audio server and the
/// control API.
pub struct ServerState {
    clients: RwLock<HashMap<String, ClientInfo>>,
    /// Serializes client replacement with deletion without holding either
    /// state lock across the other, avoiding the clients/groups lock inversion.
    client_lifecycle: Mutex<()>,
    max_known_clients: usize,
    client_removal_hook: RwLock<Option<Arc<ClientRemovalHook>>>,
    next_client_generation: AtomicU64,
    stream_status_hook: RwLock<Option<Arc<StreamStatusHook>>>,
    groups: RwLock<HashMap<String, Group>>,
    announcements: Mutex<AnnouncementScheduler>,
    announcement_controls: broadcast::Sender<AnnouncementControlV1>,
    streams: RwLock<HashMap<String, StreamInfo>>,
    events: Arc<EventBus>,
    start_time: DateTime<Utc>,
    persistence: Option<Arc<PersistenceStore>>,
    /// Snapshot of stream settings loaded at startup.
    saved_streams: Vec<PersistedStream>,
    /// Active media transport configuration (runtime-mutable via control API).
    transport: parking_lot::Mutex<TransportState>,
    /// IANA timezone identifier for log timestamps and web UI display.
    timezone: parking_lot::RwLock<Option<String>>,
}

impl ServerState {
    pub fn new(
        events: Arc<EventBus>,
        persistence: Option<Arc<PersistenceStore>>,
        saved_clients: Vec<PersistedClient>,
        saved_streams: Vec<PersistedStream>,
    ) -> Self {
        Self::new_with_known_client_limit(events, persistence, saved_clients, saved_streams, 256)
    }

    /// Construct state with an explicit bound for remembered, disconnected clients.
    pub fn new_with_known_client_limit(
        events: Arc<EventBus>,
        persistence: Option<Arc<PersistenceStore>>,
        saved_clients: Vec<PersistedClient>,
        saved_streams: Vec<PersistedStream>,
        max_known_clients: usize,
    ) -> Self {
        let mut saved_clients: Vec<PersistedClient> = saved_clients
            .into_iter()
            .filter(|client| validate_client_id(&client.id).is_ok())
            .collect();
        saved_clients.sort_by_key(|client| Reverse(client.last_seen));
        saved_clients.truncate(max_known_clients);
        let mut groups = HashMap::new();
        let default_grp = Group {
            id: "default".into(),
            name: "Default".into(),
            stream_id: "default".into(),
            client_ids: vec![],
        };
        groups.insert("default".into(), default_grp);

        let mut streams = HashMap::new();
        for ps in &saved_streams {
            streams.insert(
                ps.id.clone(),
                StreamInfo {
                    id: ps.id.clone(),
                    display_name: None, // No display_name in PersistedStream yet
                    codec: "Unknown".into(),
                    format: "Unknown".into(),
                    source: "Unknown".into(),
                    buffer_ms: 1000,
                    buffer_ms_overridden: false,
                    chunk_ms: 20,
                    chunk_ms_overridden: false,
                    idle_timeout_ms: None,
                    silence_on_idle: false,
                    status: StreamStatus::Idle,
                    recovery: None,
                    eq_bands: ps.eq_bands.clone(),
                    eq_enabled: ps.eq_enabled,
                },
            );
        }

        if !streams.contains_key("default") {
            streams.insert(
                "default".into(),
                StreamInfo {
                    id: "default".into(),
                    display_name: Some("Default Stream".into()),
                    codec: "Unknown".into(),
                    format: "Unknown".into(),
                    source: "Unknown".into(),
                    buffer_ms: 1000,
                    buffer_ms_overridden: false,
                    chunk_ms: 20,
                    chunk_ms_overridden: false,
                    idle_timeout_ms: None,
                    silence_on_idle: false,
                    status: StreamStatus::Idle,
                    recovery: None,
                    eq_bands: vec![],
                    eq_enabled: true,
                },
            );
        }

        let mut announcements = AnnouncementScheduler::new(
            AnnouncementLimits::default(),
            groups.keys().cloned(),
            SchedulerConfig::default(),
        );
        announcements.set_group_clients(
            "default",
            saved_clients
                .iter()
                .filter(|client| client.group_id == "default")
                .map(|client| AnnouncementClient {
                    client_id: client.id.clone(),
                    generation: 0,
                }),
            Utc::now().timestamp_millis(),
        );
        let (announcement_controls, _) = broadcast::channel(256);

        Self {
            clients: RwLock::new(
                saved_clients
                    .iter()
                    .map(|c| {
                        (
                            c.id.clone(),
                            ClientInfo {
                                id: c.id.clone(),
                                hostname: c.hostname.clone(),
                                client_name: "Sonium".into(),
                                os: "unknown".into(),
                                arch: "unknown".into(),
                                remote_addr: "".into(),
                                volume: c.volume,
                                muted: c.muted,
                                latency_ms: c.latency_ms,
                                group_id: c.group_id.clone(),
                                status: ClientStatus::Disconnected,
                                connected_at: c.last_seen,
                                protocol_version: 0,
                                session_generation: 0,
                                display_name: c.display_name.clone(),
                                observability_enabled: c.observability_enabled,
                                health: None,
                                last_clock_offset_ms: None,
                            },
                        )
                    })
                    .collect(),
            ),
            client_lifecycle: Mutex::new(()),
            max_known_clients,
            client_removal_hook: RwLock::new(None),
            next_client_generation: AtomicU64::new(1),
            stream_status_hook: RwLock::new(None),
            groups: RwLock::new(groups),
            announcements: Mutex::new(announcements),
            announcement_controls,
            streams: RwLock::new(streams),
            events,
            start_time: Utc::now(),
            persistence,
            saved_streams,
            transport: parking_lot::Mutex::new(TransportState {
                mode: TransportMode::Tcp,
                server_udp_port: 0,
            }),
            timezone: parking_lot::RwLock::new(None),
        }
    }

    /// Register cleanup for client-labelled resources owned by the server binary.
    pub fn set_client_removal_hook(&self, hook: Arc<ClientRemovalHook>) {
        *self.client_removal_hook.write() = Some(hook);
    }

    fn notify_client_removed(&self, id: &str, generation: u64) {
        if let Some(hook) = self.client_removal_hook.read().clone() {
            hook(id, generation);
        }
    }

    /// Register synchronization for stream-labelled resources owned by the
    /// server binary, such as Prometheus gauges.
    pub fn set_stream_status_hook(&self, hook: Arc<StreamStatusHook>) {
        *self.stream_status_hook.write() = Some(hook);
    }

    fn notify_stream_status(
        &self,
        id: &str,
        status: StreamStatus,
        recovery: Option<StreamRecovery>,
    ) {
        if let Some(hook) = self.stream_status_hook.read().clone() {
            hook(id, status, recovery);
        }
    }

    /// Set the timezone identifier.
    pub fn set_timezone(&self, tz: Option<String>) {
        *self.timezone.write() = tz;
    }

    /// Get the timezone identifier.
    pub fn timezone(&self) -> Option<String> {
        self.timezone.read().clone()
    }

    /// Restore persisted groups (call before accepting any client connections).
    pub fn restore_groups(&self, persisted: Vec<PersistedGroup>) {
        let mut groups = self.groups.write();
        for pg in persisted {
            self.announcements.lock().add_group(pg.id.clone());
            groups.entry(pg.id.clone()).or_insert_with(|| Group {
                id: pg.id,
                name: pg.name,
                stream_id: pg.stream_id,
                client_ids: vec![],
            });
        }
        for client in self.clients.read().values() {
            if let Some(group) = groups.get_mut(&client.group_id) {
                if !group.client_ids.contains(&client.id) {
                    group.client_ids.push(client.id.clone());
                }
            }
        }
        let group_ids: Vec<String> = groups.keys().cloned().collect();
        drop(groups);
        let now_ms = Utc::now().timestamp_millis();
        for group_id in group_ids {
            self.reconcile_announcement_group(&group_id, now_ms);
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    fn persist(&self) {
        let Some(store) = &self.persistence else {
            return;
        };
        let groups: Vec<PersistedGroup> = self
            .groups
            .read()
            .values()
            .map(|g| PersistedGroup {
                id: g.id.clone(),
                name: g.name.clone(),
                stream_id: g.stream_id.clone(),
            })
            .collect();
        let clients: Vec<PersistedClient> = self
            .clients
            .read()
            .values()
            .map(|c| PersistedClient {
                id: c.id.clone(),
                hostname: c.hostname.clone(),
                display_name: c.display_name.clone(),
                volume: c.volume,
                muted: c.muted,
                latency_ms: c.latency_ms,
                observability_enabled: c.observability_enabled,
                group_id: c.group_id.clone(),
                last_seen: Utc::now(),
            })
            .collect();
        let streams: Vec<PersistedStream> = self
            .streams
            .read()
            .values()
            .map(|s| PersistedStream {
                id: s.id.clone(),
                eq_bands: s.eq_bands.clone(),
                eq_enabled: s.eq_enabled,
            })
            .collect();

        store.save(&groups, &clients, &streams);
    }

    fn announcement_clients_in_group(&self, group_id: &str) -> Vec<AnnouncementClient> {
        self.clients
            .read()
            .values()
            .filter(|client| client.group_id == group_id)
            .map(|client| AnnouncementClient {
                client_id: client.id.clone(),
                generation: client.session_generation,
            })
            .collect()
    }

    fn reconcile_announcement_group(&self, group_id: &str, now_ms: i64) {
        let clients = self.announcement_clients_in_group(group_id);
        let events = self
            .announcements
            .lock()
            .set_group_clients(group_id, clients, now_ms);
        self.emit_announcement_events(events);
    }

    // ── Client CRUD ───────────────────────────────────────────────────────

    /// Register a newly connected client, restoring persisted settings if available.
    #[allow(clippy::too_many_arguments)]
    pub fn client_connected(
        &self,
        id: impl Into<String>,
        hostname: impl Into<String>,
        client_name: impl Into<String>,
        os: impl Into<String>,
        arch: impl Into<String>,
        addr: SocketAddr,
        protocol_version: u32,
    ) {
        let _ =
            self.try_client_connected(id, hostname, client_name, os, arch, addr, protocol_version);
    }

    /// Atomically admit a client without replacing an active session.
    #[allow(clippy::too_many_arguments)]
    pub fn try_client_connected(
        &self,
        id: impl Into<String>,
        hostname: impl Into<String>,
        client_name: impl Into<String>,
        os: impl Into<String>,
        arch: impl Into<String>,
        addr: SocketAddr,
        protocol_version: u32,
    ) -> Result<u64, SoniumError> {
        let id = id.into();
        validate_client_id(&id)?;
        let _lifecycle = self.client_lifecycle.lock();
        let hostname = hostname.into();

        let (info, evicted) = {
            let mut clients = self.clients.write();
            let existing = clients.get(&id).cloned();
            if existing.as_ref().is_some_and(ClientInfo::is_connected) {
                return Err(SoniumError::Protocol(
                    "client ID already has an active session".into(),
                ));
            }

            let evicted = if existing.is_none() && clients.len() >= self.max_known_clients {
                let lru_id = clients
                    .values()
                    .filter(|client| !client.is_connected())
                    .min_by_key(|client| client.connected_at)
                    .map(|client| client.id.clone())
                    .ok_or_else(|| {
                        SoniumError::Protocol(
                            "maximum known clients reached with no disconnected client to evict"
                                .into(),
                        )
                    })?;
                clients.remove(&lru_id)
            } else {
                None
            };

            // The live registry already contains every retained startup entry.
            // Once an entry is evicted or explicitly deleted, its old snapshot
            // must not remain as a second restoration source.
            let (volume, muted, latency_ms, group_id, display_name, observability_enabled) =
                if let Some(client) = existing {
                    (
                        client.volume,
                        client.muted,
                        client.latency_ms,
                        client.group_id,
                        client.display_name,
                        client.observability_enabled,
                    )
                } else {
                    (100, false, 0, "default".into(), None, false)
                };

            let session_generation = self.next_client_generation.fetch_add(1, Ordering::Relaxed);
            let info = ClientInfo {
                id: id.clone(),
                hostname: hostname.clone(),
                client_name: client_name.into(),
                os: os.into(),
                arch: arch.into(),
                remote_addr: addr.to_string(),
                volume,
                muted,
                latency_ms,
                group_id,
                status: ClientStatus::Connected,
                connected_at: Utc::now(),
                protocol_version,
                session_generation,
                display_name,
                observability_enabled,
                health: None,
                last_clock_offset_ms: None,
            };
            clients.insert(id.clone(), info.clone());
            (info, evicted)
        };

        // Place into the correct group (restored or default).
        {
            let mut groups = self.groups.write();
            if let Some(evicted) = &evicted {
                if let Some(group) = groups.get_mut(&evicted.group_id) {
                    group
                        .client_ids
                        .retain(|client_id| client_id != &evicted.id);
                }
            }
            // Remove from any group that already lists this client (stale from previous session).
            for g in groups.values_mut() {
                g.client_ids.retain(|cid| cid != &id);
            }
            let target = if groups.contains_key(&info.group_id) {
                info.group_id.clone()
            } else {
                "default".into()
            };
            if let Some(grp) = groups.get_mut(&target) {
                grp.client_ids.push(id.clone());
            }
        }

        let now_ms = Utc::now().timestamp_millis();
        self.reconcile_announcement_group(&info.group_id, now_ms);
        if let Some(evicted) = &evicted {
            if evicted.group_id != info.group_id {
                self.reconcile_announcement_group(&evicted.group_id, now_ms);
            }
        }

        if let Some(evicted) = evicted {
            self.events.emit(crate::ws::Event::ClientDeleted {
                client_id: evicted.id.clone(),
            });
            self.notify_client_removed(&evicted.id, evicted.session_generation);
        }
        let session_generation = info.session_generation;
        self.events
            .emit(crate::ws::Event::ClientConnected { client: info });
        self.persist();
        Ok(session_generation)
    }

    /// Mark a client as disconnected (keeps history in the registry).
    pub fn client_disconnected(&self, id: &str) {
        let generation = self
            .clients
            .read()
            .get(id)
            .map(|client| client.session_generation);
        if let Some(generation) = generation {
            self.client_disconnected_generation(id, generation);
        }
    }

    /// Mark only the matching admitted session as disconnected.
    ///
    /// The metric cleanup hook runs after releasing the state lock; its
    /// generation lets the metric registry ignore stale cleanup after a
    /// reconnect.
    pub fn client_disconnected_generation(&self, id: &str, generation: u64) -> bool {
        let disconnected = {
            let mut clients = self.clients.write();
            let Some(client) = clients.get_mut(id) else {
                return false;
            };
            if client.session_generation != generation || !client.is_connected() {
                return false;
            }
            client.status = ClientStatus::Disconnected;
            client.connected_at = Utc::now();
            true
        };
        if disconnected {
            self.events.emit(crate::ws::Event::ClientDisconnected {
                client_id: id.into(),
            });
            self.notify_client_removed(id, generation);
            self.persist();
        }
        disconnected
    }

    /// Permanently remove a disconnected client from the registry.
    /// Returns `false` if the client is not found or is still connected.
    pub fn delete_client(&self, client_id: &str) -> bool {
        let _lifecycle = self.client_lifecycle.lock();
        let info = {
            let mut clients = self.clients.write();
            match clients.get(client_id) {
                None => return false,
                Some(c) if c.is_connected() => return false,
                _ => {}
            }
            clients.remove(client_id).expect("client was checked above")
        };
        // Remove from its group.
        if let Some(g) = self.groups.write().get_mut(&info.group_id) {
            g.client_ids.retain(|id| id != client_id);
        }
        self.reconcile_announcement_group(&info.group_id, Utc::now().timestamp_millis());
        // Emit while reconnects are still excluded so an old ClientDeleted
        // event can never arrive after the replacement ClientConnected event.
        self.events.emit(crate::ws::Event::ClientDeleted {
            client_id: client_id.into(),
        });
        self.notify_client_removed(client_id, info.session_generation);
        self.persist();
        true
    }

    /// Update volume and/or mute for a client, push event, return the new state.
    pub fn set_volume(&self, client_id: &str, volume: u8, muted: bool) -> Option<(u8, bool)> {
        let mut clients = self.clients.write();
        let c = clients.get_mut(client_id)?;
        c.volume = volume;
        c.muted = muted;
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

    /// Enable or disable diagnostic health reporting for a client.
    pub fn set_client_observability(&self, client_id: &str, enabled: bool) -> bool {
        let mut clients = self.clients.write();
        if let Some(c) = clients.get_mut(client_id) {
            c.observability_enabled = enabled;
            if !enabled {
                c.health = None;
            }
            self.events
                .emit(crate::ws::Event::ClientObservabilityChanged {
                    client_id: client_id.into(),
                    enabled,
                });
            drop(clients);
            self.persist();
            true
        } else {
            false
        }
    }

    /// Update the EQ bands for a stream and push to connected sessions.
    pub fn set_eq(&self, stream_id: &str, eq_bands: Vec<EqBand>, enabled: bool) -> bool {
        let mut streams = self.streams.write();
        if let Some(s) = streams.get_mut(stream_id) {
            s.eq_bands = eq_bands.clone();
            s.eq_enabled = enabled;
            self.events.emit(crate::ws::Event::StreamEqChanged {
                stream_id: stream_id.into(),
                eq_bands,
                enabled,
            });
            drop(streams);
            self.persist();
            true
        } else {
            false
        }
    }

    /// Read the EQ bands for a stream.
    pub fn get_stream_eq(&self, stream_id: &str) -> Option<(Vec<EqBand>, bool)> {
        self.streams
            .read()
            .get(stream_id)
            .map(|s| (s.eq_bands.clone(), s.eq_enabled))
    }

    /// Set an operator-assigned display name for a client.
    pub fn set_client_name(&self, client_id: &str, display_name: Option<String>) -> bool {
        let mut clients = self.clients.write();
        if let Some(c) = clients.get_mut(client_id) {
            c.display_name = display_name.clone();
            self.events.emit(crate::ws::Event::ClientRenamed {
                client_id: client_id.into(),
                display_name: display_name.unwrap_or_default(),
            });
            drop(clients);
            self.persist();
            true
        } else {
            false
        }
    }

    /// Update health metrics for a client and push to UI.
    pub fn set_client_health(&self, client_id: &str, health: HealthReport) -> bool {
        let mut clients = self.clients.write();
        if let Some(c) = clients.get_mut(client_id) {
            c.health = Some(health.clone());
            self.events.emit(crate::ws::Event::ClientHealth {
                client_id: client_id.into(),
                health,
            });
            true
        } else {
            false
        }
    }

    /// Store the last clock offset (ms) from a health report — used for group sync.
    pub fn set_client_clock_offset(&self, client_id: &str, offset_ms: i32) -> bool {
        let mut clients = self.clients.write();
        if let Some(c) = clients.get_mut(client_id) {
            c.last_clock_offset_ms = Some(offset_ms);
            true
        } else {
            false
        }
    }

    /// Calculate the median clock offset (µs) for all connected clients in a group.
    /// Returns `None` if fewer than 2 connected clients have reported an offset
    /// (group sync is pointless with a single client).
    pub fn group_median_clock_offset_us(&self, group_id: &str) -> Option<i64> {
        let clients = self.clients.read();
        let groups = self.groups.read();
        let group = groups.get(group_id)?;
        let mut offsets: Vec<i64> = group
            .client_ids
            .iter()
            .filter_map(|cid| {
                let c = clients.get(cid)?;
                if c.is_connected() {
                    c.last_clock_offset_ms.map(|ms| ms as i64 * 1000)
                } else {
                    None
                }
            })
            .collect();
        if offsets.len() < 2 {
            return None;
        }
        offsets.sort_unstable();
        let mid = offsets.len() / 2;
        if offsets.len().is_multiple_of(2) {
            Some((offsets[mid - 1] + offsets[mid]) / 2)
        } else {
            Some(offsets[mid])
        }
    }

    /// Move a client to a different group.
    pub fn set_client_group(&self, client_id: &str, group_id: &str) -> bool {
        let mut clients = self.clients.write();
        let mut groups = self.groups.write();

        let client = match clients.get_mut(client_id) {
            Some(c) => c,
            None => return false,
        };
        if !groups.contains_key(group_id) {
            return false;
        }
        if client.group_id == group_id {
            return true;
        }
        let old_group_id = client.group_id.clone();

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
        drop(clients);
        drop(groups);
        let now_ms = Utc::now().timestamp_millis();
        self.reconcile_announcement_group(&old_group_id, now_ms);
        self.reconcile_announcement_group(group_id, now_ms);
        self.events.emit(crate::ws::Event::ClientGroupChanged {
            client_id: client_id.into(),
            group_id: group_id.into(),
        });
        self.persist();
        true
    }

    // ── Group CRUD ────────────────────────────────────────────────────────

    /// Create a new group and return its generated ID.
    pub fn create_group(&self, name: impl Into<String>, stream_id: impl Into<String>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let grp = Group {
            id: id.clone(),
            name: name.into(),
            stream_id: stream_id.into(),
            client_ids: vec![],
        };
        self.groups.write().insert(id.clone(), grp.clone());
        self.announcements.lock().add_group(id.clone());
        self.events
            .emit(crate::ws::Event::GroupCreated { group: grp });
        self.persist();
        id
    }

    /// Delete a group; clients in the group are moved to "default".
    pub fn delete_group(&self, group_id: &str) -> bool {
        if group_id == "default" {
            return false;
        }
        let mut groups = self.groups.write();
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
            self.events.emit(crate::ws::Event::GroupDeleted {
                group_id: group_id.into(),
            });
            let events = self
                .announcements
                .lock()
                .remove_group(group_id, Utc::now().timestamp_millis());
            self.emit_announcement_events(events);
            drop(groups);
            drop(clients);
            self.reconcile_announcement_group("default", Utc::now().timestamp_millis());
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
        if !self.streams.read().contains_key(stream_id) {
            return false;
        }
        if let Some(g) = groups.get_mut(group_id) {
            g.stream_id = stream_id.into();
            self.events.emit(crate::ws::Event::GroupStreamChanged {
                group_id: group_id.into(),
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
            if status != StreamStatus::Recovering {
                s.recovery = None;
            }
            s.status = status.clone();
            self.events.emit(crate::ws::Event::StreamStatus {
                stream_id: stream_id.into(),
                status: status.clone(),
                recovery: None,
            });
            drop(streams);
            self.notify_stream_status(stream_id, status, None);
        }
    }

    /// Mark a source as awaiting a reopen, with operator-visible retry context.
    pub fn set_stream_recovering(&self, stream_id: &str, attempt: u32, retry_in_ms: u64) {
        let mut streams = self.streams.write();
        if let Some(s) = streams.get_mut(stream_id) {
            s.status = StreamStatus::Recovering;
            let recovery = StreamRecovery {
                attempt,
                retry_in_ms,
            };
            s.recovery = Some(recovery.clone());
            self.events.emit(crate::ws::Event::StreamStatus {
                stream_id: stream_id.into(),
                status: StreamStatus::Recovering,
                recovery: Some(recovery.clone()),
            });
            drop(streams);
            self.notify_stream_status(stream_id, StreamStatus::Recovering, Some(recovery));
        }
    }

    // ── Read access ───────────────────────────────────────────────────────

    pub fn all_clients(&self) -> Vec<ClientInfo> {
        self.clients.read().values().cloned().collect()
    }

    pub fn connected_clients(&self) -> Vec<ClientInfo> {
        self.clients
            .read()
            .values()
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

    // ── Announcements ─────────────────────────────────────────────────────

    /// Admit a bounded, idempotent announcement intent and broadcast its
    /// initial lifecycle state to authenticated WebSocket observers.
    pub fn admit_announcement(
        &self,
        intent: AnnouncementIntent,
        now_ms: i64,
    ) -> Result<AnnouncementAdmission, AnnouncementError> {
        for group_id in &intent.target_groups {
            self.reconcile_announcement_group(group_id, now_ms);
        }
        let SchedulerAdmission { admission, events } =
            self.announcements.lock().admit(intent, now_ms)?;
        self.emit_announcement_events(events);
        Ok(admission)
    }

    pub fn acknowledge_announcement(
        &self,
        id: &str,
        group_id: &str,
        lifecycle: AnnouncementLifecycle,
    ) -> Result<(), AnnouncementError> {
        self.acknowledge_announcement_at(id, group_id, lifecycle, Utc::now().timestamp_millis())
    }

    pub fn acknowledge_announcement_at(
        &self,
        id: &str,
        group_id: &str,
        lifecycle: AnnouncementLifecycle,
        now_ms: i64,
    ) -> Result<(), AnnouncementError> {
        let events = self
            .announcements
            .lock()
            .acknowledge(id, group_id, lifecycle, now_ms)?;
        self.emit_announcement_events(events);
        Ok(())
    }

    pub fn acknowledge_announcement_client_at(
        &self,
        id: &str,
        group_id: &str,
        client_id: &str,
        generation: u64,
        lifecycle: AnnouncementLifecycle,
        now_ms: i64,
    ) -> Result<(), AnnouncementError> {
        let events = self.announcements.lock().acknowledge_client(
            id,
            group_id,
            &AnnouncementClient {
                client_id: client_id.into(),
                generation,
            },
            lifecycle,
            now_ms,
        )?;
        self.emit_announcement_events(events);
        Ok(())
    }

    pub fn cancel_announcement(&self, id: &str) -> Result<(), AnnouncementError> {
        let events = self
            .announcements
            .lock()
            .cancel(id, Utc::now().timestamp_millis())?;
        self.emit_announcement_events(events);
        Ok(())
    }

    pub fn expire_announcements(&self, now_ms: i64) {
        let events = self.announcements.lock().tick(now_ms);
        self.emit_announcement_events(events);
    }

    pub fn all_announcements(&self) -> Vec<AnnouncementRecord> {
        self.announcements.lock().records()
    }

    /// Subscribe a media session to bounded announcement controls.  Browser
    /// lifecycle events remain on the existing EventBus and do not consume
    /// this channel.
    pub fn subscribe_announcement_controls(&self) -> broadcast::Receiver<AnnouncementControlV1> {
        self.announcement_controls.subscribe()
    }

    pub fn pending_announcement_control(&self, group_id: &str) -> Option<AnnouncementControlV1> {
        self.announcements.lock().pending_control(group_id)
    }

    fn emit_announcement_events(&self, events: SchedulerEvents) {
        self.emit_announcement_transitions(events.transitions);
        for control in events.controls {
            let _ = self.announcement_controls.send(control);
        }
    }

    fn emit_announcement_transitions(&self, transitions: Vec<AnnouncementTransition>) {
        for transition in transitions {
            self.events.emit(crate::ws::Event::AnnouncementLifecycle {
                announcement_id: transition.announcement_id,
                group_id: transition.group_id,
                lifecycle: transition.lifecycle,
                resume: transition.resume,
            });
        }
    }

    pub fn get_group(&self, id: &str) -> Option<Group> {
        self.groups.read().get(id).cloned()
    }

    /// Returns the stream_id currently assigned to a client's group.
    pub fn client_stream_id(&self, client_id: &str) -> Option<String> {
        let group_id = self.clients.read().get(client_id)?.group_id.clone();
        let stream_id = self.groups.read().get(&group_id)?.stream_id.clone();
        Some(stream_id)
    }

    /// Register a new stream in the state (idempotent — updates status if already exists).
    #[allow(clippy::too_many_arguments)]
    pub fn register_stream(
        &self,
        id: impl Into<String>,
        display_name: Option<String>,
        codec: impl Into<String>,
        format: impl Into<String>,
        source: impl Into<String>,
        buffer_ms: u32,
        buffer_ms_overridden: bool,
        chunk_ms: u32,
        chunk_ms_overridden: bool,
        idle_timeout_ms: Option<u32>,
        silence_on_idle: bool,
    ) {
        let id = id.into();
        let codec = codec.into();
        let format = format.into();
        let source = source.into();
        let mut streams = self.streams.write();
        streams
            .entry(id.clone())
            .and_modify(|stream| {
                stream.display_name = display_name.clone();
                stream.codec = codec.clone();
                stream.format = format.clone();
                stream.source = source.clone();
                stream.buffer_ms = buffer_ms;
                stream.buffer_ms_overridden = buffer_ms_overridden;
                stream.chunk_ms = chunk_ms;
                stream.chunk_ms_overridden = chunk_ms_overridden;
                stream.idle_timeout_ms = idle_timeout_ms;
                stream.silence_on_idle = silence_on_idle;
            })
            .or_insert_with(|| {
                // Restore EQ settings if this stream was previously saved.
                let (eq_bands, eq_enabled) = self
                    .saved_streams
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| (s.eq_bands.clone(), s.eq_enabled))
                    .unwrap_or_default();

                StreamInfo {
                    id: id.clone(),
                    display_name,
                    codec,
                    format,
                    source,
                    buffer_ms,
                    buffer_ms_overridden,
                    chunk_ms,
                    chunk_ms_overridden,
                    idle_timeout_ms,
                    silence_on_idle,
                    status: StreamStatus::Idle,
                    recovery: None,
                    eq_bands,
                    eq_enabled,
                }
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

    // ── Transport ─────────────────────────────────────────────────────────

    /// Initialise transport config from the loaded config file.
    /// Must be called once from `main` before accepting connections.
    pub fn set_transport_config(&self, mode: TransportMode, server_udp_port: u16) {
        let mut t = self.transport.lock();
        t.mode = mode;
        t.server_udp_port = server_udp_port;
    }

    /// Current active transport mode.
    pub fn transport_mode(&self) -> TransportMode {
        self.transport.lock().mode
    }

    /// Server UDP port for RTP media delivery (`0` = not configured).
    pub fn server_udp_port(&self) -> u16 {
        self.transport.lock().server_udp_port
    }

    /// Change the active transport mode and broadcast a `TransportModeChanged` event.
    pub fn set_transport_mode(&self, mode: TransportMode) {
        let udp_port = {
            let mut t = self.transport.lock();
            t.mode = mode;
            t.server_udp_port
        };
        self.events.emit(crate::ws::Event::TransportModeChanged {
            mode: mode.to_string(),
            server_udp_port: udp_port,
        });
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn state() -> Arc<ServerState> {
        Arc::new(ServerState::new(
            Arc::new(EventBus::new()),
            None,
            vec![],
            vec![],
        ))
    }

    fn state_with_known_client_limit(max_known_clients: usize) -> Arc<ServerState> {
        Arc::new(ServerState::new_with_known_client_limit(
            Arc::new(EventBus::new()),
            None,
            vec![],
            vec![],
            max_known_clients,
        ))
    }

    fn addr() -> SocketAddr {
        "127.0.0.1:50000".parse().unwrap()
    }

    fn connect(s: &ServerState, id: &str) {
        s.client_connected(id, "pi", "Sonium", "linux", "aarch64", addr(), 2);
    }

    #[test]
    fn evicts_the_least_recently_seen_disconnected_client_at_known_client_capacity() {
        let s = state_with_known_client_limit(2);
        connect(&s, "active-client");
        connect(&s, "old-client");
        s.client_disconnected("old-client");

        s.try_client_connected(
            "new-client",
            "new-pi",
            "Sonium",
            "linux",
            "aarch64",
            addr(),
            2,
        )
        .expect("a disconnected LRU client can be replaced");

        assert!(s.get_client("old-client").is_none());
        assert!(s.get_client("active-client").unwrap().is_connected());
        assert!(s.get_client("new-client").unwrap().is_connected());
        assert!(!s
            .get_group("default")
            .unwrap()
            .client_ids
            .contains(&"old-client".to_string()));
    }

    #[test]
    fn rejects_a_duplicate_active_id_without_replacing_its_session_state() {
        let s = state();
        s.try_client_connected(
            "living-room-1",
            "first-host",
            "Sonium",
            "linux",
            "aarch64",
            addr(),
            2,
        )
        .expect("first session admitted");

        assert!(s
            .try_client_connected(
                "living-room-1",
                "second-host",
                "Sonium",
                "linux",
                "aarch64",
                addr(),
                2,
            )
            .is_err());
        assert_eq!(
            s.get_client("living-room-1").unwrap().hostname,
            "first-host"
        );
    }

    #[test]
    fn stale_disconnect_generation_cannot_mark_a_reconnected_client_offline() {
        let s = state();
        let first = s
            .try_client_connected(
                "generation-client",
                "first-host",
                "Sonium",
                "linux",
                "aarch64",
                addr(),
                2,
            )
            .expect("first session admitted");
        assert!(s.client_disconnected_generation("generation-client", first));

        let second = s
            .try_client_connected(
                "generation-client",
                "second-host",
                "Sonium",
                "linux",
                "aarch64",
                addr(),
                2,
            )
            .expect("reconnect admitted after the first session exits");

        assert_ne!(first, second);
        assert!(!s.client_disconnected_generation("generation-client", first));
        assert!(s.get_client("generation-client").unwrap().is_connected());
    }

    #[test]
    fn delete_and_reconnect_keep_the_replacement_membership_and_event_order() {
        use std::sync::mpsc;
        use std::time::Duration;

        let s = state();
        connect(&s, "generation-client");
        s.client_disconnected("generation-client");
        let mut events = s.events().subscribe();
        let lifecycle = s.client_lifecycle.lock();

        let (delete_done_tx, delete_done_rx) = mpsc::channel();
        let deleting = s.clone();
        std::thread::spawn(move || {
            delete_done_tx
                .send(deleting.delete_client("generation-client"))
                .unwrap();
        });

        let (reconnect_done_tx, reconnect_done_rx) = mpsc::channel();
        let reconnecting = s.clone();
        std::thread::spawn(move || {
            reconnect_done_tx
                .send(reconnecting.try_client_connected(
                    "generation-client",
                    "replacement-host",
                    "Sonium",
                    "linux",
                    "aarch64",
                    addr(),
                    2,
                ))
                .unwrap();
        });

        drop(lifecycle);
        let deleted = delete_done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let reconnected = reconnect_done_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .expect("replacement session is admitted");

        // Both operations may acquire the lifecycle mutex first. If reconnect
        // wins, delete observes the replacement and is rejected; if delete
        // wins, it emits ClientDeleted while still holding the mutex, followed
        // by reconnect's ClientConnected event. In either case, the mutex
        // guarantees that the operations cannot interleave.
        let first = events.try_recv().unwrap();
        if matches!(
            first,
            crate::ws::Event::ClientDeleted { ref client_id }
                if client_id == "generation-client"
        ) {
            assert!(deleted);
            assert!(matches!(
                events.try_recv().unwrap(),
                crate::ws::Event::ClientConnected { ref client }
                    if client.hostname == "replacement-host"
            ));
        } else {
            assert!(!deleted);
            assert!(matches!(
                first,
                crate::ws::Event::ClientConnected { ref client }
                    if client.hostname == "replacement-host"
            ));
            assert!(events.try_recv().is_err());
        }

        let replacement = s.get_client("generation-client").unwrap();
        assert_eq!(replacement.hostname, "replacement-host");
        assert!(replacement.is_connected());
        assert!(s
            .get_group("default")
            .unwrap()
            .client_ids
            .contains(&"generation-client".to_string()));

        assert_eq!(reconnected, replacement.session_generation);
    }

    #[test]
    fn removal_hook_runs_on_disconnect_delete_and_lru_eviction() {
        let s = state_with_known_client_limit(1);
        let removed = Arc::new(Mutex::new(Vec::<String>::new()));
        let observed = removed.clone();
        s.set_client_removal_hook(Arc::new(move |id, _generation| {
            observed.lock().unwrap().push(id.to_owned())
        }));

        connect(&s, "first-client");
        s.client_disconnected("first-client");
        assert!(s.delete_client("first-client"));

        connect(&s, "evicted-client");
        s.client_disconnected("evicted-client");
        s.try_client_connected(
            "replacement-client",
            "pi",
            "Sonium",
            "linux",
            "aarch64",
            addr(),
            2,
        )
        .expect("disconnected client can be evicted");

        assert_eq!(
            *removed.lock().unwrap(),
            vec![
                "first-client",
                "first-client",
                "evicted-client",
                "evicted-client"
            ]
        );
    }

    #[test]
    fn unsafe_persisted_client_ids_are_not_restored() {
        let saved_clients = vec![PersistedClient {
            id: "living room/speaker".into(),
            hostname: "pi".into(),
            display_name: None,
            volume: 100,
            muted: false,
            latency_ms: 0,
            observability_enabled: false,
            group_id: "default".into(),
            last_seen: Utc::now(),
        }];
        let s = ServerState::new(Arc::new(EventBus::new()), None, saved_clients, vec![]);

        assert!(
            s.get_client("living room/speaker").is_none(),
            "an unadmitted ID must not occupy persistent server state"
        );
    }

    #[test]
    fn unsafe_connected_client_ids_are_not_added_or_persisted() {
        let s = state();

        s.client_connected(
            "living room/speaker",
            "pi",
            "Sonium",
            "linux",
            "aarch64",
            addr(),
            2,
        );

        assert!(s.all_clients().is_empty());
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
        let s = state();
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
        let s = state();
        let gid = s.create_group("Bedroom", "default");
        connect(&s, "pi-1");
        assert!(s.set_client_group("pi-1", &gid));
        assert_eq!(s.get_client("pi-1").unwrap().group_id, gid);
        // removed from default
        assert!(!s
            .get_group("default")
            .unwrap()
            .client_ids
            .contains(&"pi-1".to_string()));
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
        assert!(!s
            .get_group("default")
            .unwrap()
            .client_ids
            .contains(&"pi-1".to_string()));
    }

    #[test]
    fn delete_client_with_persistence_releases_the_client_lock_before_saving() {
        use std::sync::mpsc;
        use std::time::Duration;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let persistence = Arc::new(PersistenceStore::new(dir.path()));
        let s = Arc::new(ServerState::new(
            Arc::new(EventBus::new()),
            Some(persistence.clone()),
            vec![],
            vec![],
        ));
        connect(&s, "pi-1");
        s.client_disconnected("pi-1");

        let (done_tx, done_rx) = mpsc::channel();
        let deleting = s.clone();
        std::thread::spawn(move || done_tx.send(deleting.delete_client("pi-1")).unwrap());

        assert!(
            done_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "deletion must finish instead of recursively locking clients during persistence"
        );
        let (_, clients, _) = persistence.load();
        assert!(
            clients.is_empty(),
            "deleted client must not remain persisted"
        );
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
        assert_eq!(
            s.get_client("pi-1").unwrap().display_name.as_deref(),
            Some("Living Room Speaker")
        );
    }

    #[test]
    fn client_restored_from_persisted_state() {
        let saved_clients = vec![PersistedClient {
            id: "pi-1".into(),
            hostname: "pi".into(),
            display_name: Some("Kitchen".into()),
            volume: 60,
            muted: true,
            latency_ms: 50,
            observability_enabled: false,
            group_id: "default".into(),
            last_seen: Utc::now(),
        }];
        let s = Arc::new(ServerState::new(
            Arc::new(EventBus::new()),
            None,
            saved_clients,
            vec![],
        ));
        s.client_connected("pi-1", "pi", "Sonium", "linux", "aarch64", addr(), 2);
        let c = s.get_client("pi-1").unwrap();
        assert_eq!(c.volume, 60);
        assert!(c.muted);
        assert_eq!(c.latency_ms, 50);
        assert_eq!(c.display_name.as_deref(), Some("Kitchen"));
    }

    #[test]
    fn deleted_or_evicted_client_does_not_restore_the_startup_snapshot() {
        let saved_client = PersistedClient {
            id: "pi-1".into(),
            hostname: "pi".into(),
            display_name: Some("Stale Name".into()),
            volume: 23,
            muted: true,
            latency_ms: 50,
            observability_enabled: true,
            group_id: "default".into(),
            last_seen: Utc::now(),
        };

        let deleted = ServerState::new(
            Arc::new(EventBus::new()),
            None,
            vec![saved_client.clone()],
            vec![],
        );
        assert!(deleted.delete_client("pi-1"));
        connect(&deleted, "pi-1");
        let client = deleted.get_client("pi-1").unwrap();
        assert_eq!(client.volume, 100);
        assert!(!client.muted);
        assert_eq!(client.display_name, None);

        let evicted = ServerState::new_with_known_client_limit(
            Arc::new(EventBus::new()),
            None,
            vec![saved_client],
            vec![],
            1,
        );
        connect(&evicted, "replacement");
        evicted.client_disconnected("replacement");
        connect(&evicted, "pi-1");
        let client = evicted.get_client("pi-1").unwrap();
        assert_eq!(client.volume, 100);
        assert!(!client.muted);
        assert_eq!(client.display_name, None);
    }

    #[test]
    fn recovery_event_matches_rest_state_and_includes_retry_context() {
        let s = state();
        let mut events = s.events().subscribe();

        s.set_stream_recovering("default", 3, 200);

        let stream = s
            .all_streams()
            .into_iter()
            .find(|stream| stream.id == "default")
            .unwrap();
        assert_eq!(stream.status, StreamStatus::Recovering);
        assert_eq!(
            stream.recovery,
            Some(StreamRecovery {
                attempt: 3,
                retry_in_ms: 200,
            })
        );
        match events.try_recv().unwrap() {
            crate::ws::Event::StreamStatus {
                stream_id,
                status,
                recovery,
            } => {
                assert_eq!(stream_id, "default");
                assert_eq!(status, StreamStatus::Recovering);
                assert_eq!(recovery, stream.recovery);
            }
            event => panic!("unexpected event: {event:?}"),
        }
    }

    #[test]
    fn stream_status_hook_receives_recovery_context_for_metrics() {
        let s = state();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let hook_observed = observed.clone();
        s.set_stream_status_hook(Arc::new(move |id, status, recovery| {
            hook_observed
                .lock()
                .unwrap()
                .push((id.to_owned(), status, recovery));
        }));

        s.set_stream_recovering("default", 2, 100);

        assert_eq!(
            *observed.lock().unwrap(),
            vec![(
                "default".to_owned(),
                StreamStatus::Recovering,
                Some(StreamRecovery {
                    attempt: 2,
                    retry_in_ms: 100,
                }),
            )]
        );
    }
}
