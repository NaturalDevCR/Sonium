//! State persistence — saves/loads groups and per-client settings to/from
//! `sonium-state.json` alongside `sonium.toml`.
//!
//! The file is small (< 10 KB for typical deployments) so synchronous disk I/O
//! is fine; we never call this on the hot audio path.

use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use sonium_protocol::messages::EqBand;

const STATE_FILE: &str = "sonium-state.json";
const CURRENT_VERSION: u32 = 1;

// ── Persisted types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedGroup {
    pub id:        String,
    pub name:      String,
    pub stream_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedClient {
    pub id:           String,
    pub hostname:     String,
    /// Optional display name set by the operator (overrides hostname in the UI).
    #[serde(default)]
    pub display_name: Option<String>,
    pub volume:       u8,
    pub muted:        bool,
    pub latency_ms:   i32,
    pub group_id:     String,
    pub last_seen:    DateTime<Utc>,
    /// Per-client EQ bands (empty = flat).
    #[serde(default)]
    pub eq_bands:     Vec<EqBand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StateFile {
    version: u32,
    groups:  Vec<PersistedGroup>,
    clients: Vec<PersistedClient>,
}

// ── PersistenceStore ──────────────────────────────────────────────────────

/// Thin wrapper around the JSON state file.
///
/// Load once at startup with [`PersistenceStore::load_or_empty`], then call
/// [`PersistenceStore::save`] after every mutation that should survive a restart.
pub struct PersistenceStore {
    path: PathBuf,
}

impl PersistenceStore {
    /// Create a store pointing at `<config_dir>/sonium-state.json`.
    pub fn new(config_dir: &Path) -> Self {
        Self { path: config_dir.join(STATE_FILE) }
    }

    /// Load persisted state from disk.  Returns an empty state if the file
    /// does not exist yet (first run) or is unreadable.
    pub fn load(&self) -> (Vec<PersistedGroup>, Vec<PersistedClient>) {
        if !self.path.exists() {
            return (Vec::new(), Vec::new());
        }
        match self.try_load() {
            Ok((groups, clients)) => {
                info!(
                    path = %self.path.display(),
                    groups = groups.len(),
                    clients = clients.len(),
                    "Loaded persisted state"
                );
                (groups, clients)
            }
            Err(e) => {
                warn!(path = %self.path.display(), "Failed to load state file: {e} — starting fresh");
                (Vec::new(), Vec::new())
            }
        }
    }

    fn try_load(&self) -> anyhow::Result<(Vec<PersistedGroup>, Vec<PersistedClient>)> {
        let raw = std::fs::read_to_string(&self.path)?;
        let file: StateFile = serde_json::from_str(&raw)?;
        Ok((file.groups, file.clients))
    }

    /// Persist the current groups and clients to disk.
    pub fn save(&self, groups: &[PersistedGroup], clients: &[PersistedClient]) {
        let file = StateFile {
            version: CURRENT_VERSION,
            groups:  groups.to_vec(),
            clients: clients.to_vec(),
        };
        match serde_json::to_string_pretty(&file) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.path, json) {
                    warn!(path = %self.path.display(), "Failed to save state: {e}");
                }
            }
            Err(e) => warn!("Failed to serialize state: {e}"),
        }
    }
}
