//! `ServerSettings` — volume, mute, and buffer configuration pushed by the server.

use serde::{Deserialize, Serialize};
use crate::wire::{WireRead, WireWrite};
use sonium_common::error::Result;

/// Dynamic playback settings pushed by the server to a specific client.
///
/// The server sends this message immediately after accepting a [`super::Hello`]
/// and can re-send it at any time to update the client's playback configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettings {
    /// Requested jitter buffer size in milliseconds.
    pub buffer_ms: i32,
    /// Additional client-specific latency offset in milliseconds
    /// (positive = play later, useful to compensate for Bluetooth delay).
    pub latency:   i32,
    /// Playback volume (0 – 100).
    pub volume:    u8,
    /// Whether the client should mute its output.
    pub muted:     bool,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self { buffer_ms: 1000, latency: 0, volume: 100, muted: false }
    }
}

impl ServerSettings {
    /// Deserialise from a wire payload slice.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = WireRead::new(payload);
        let json  = r.read_str()?;
        serde_json::from_str(&json)
            .map_err(|e| sonium_common::SoniumError::Protocol(format!("ServerSettings JSON: {e}")))
    }

    /// Serialise to a wire payload.
    pub fn encode(&self) -> Vec<u8> {
        let json = serde_json::to_string(self).unwrap_or_default();
        let mut w = WireWrite::with_capacity(4 + json.len());
        w.write_str(&json);
        w.finish()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trip() {
        let orig    = ServerSettings::default();
        let decoded = ServerSettings::decode(&orig.encode()).unwrap();
        assert_eq!(decoded, orig);
    }

    #[test]
    fn muted_round_trip() {
        let msg     = ServerSettings { volume: 0, muted: true, ..Default::default() };
        let decoded = ServerSettings::decode(&msg.encode()).unwrap();
        assert!(decoded.muted);
        assert_eq!(decoded.volume, 0);
    }

    #[test]
    fn latency_offset_round_trip() {
        let msg     = ServerSettings { latency: 150, ..Default::default() };
        let decoded = ServerSettings::decode(&msg.encode()).unwrap();
        assert_eq!(decoded.latency, 150);
    }

    #[test]
    fn invalid_json_returns_error() {
        let mut w = WireWrite::new();
        w.write_str("{{broken");
        assert!(ServerSettings::decode(&w.finish()).is_err());
    }
}
