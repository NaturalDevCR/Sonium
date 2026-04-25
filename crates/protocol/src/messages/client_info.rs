//! `ClientInfo` — volume / mute update sent by the client to the server.

use serde::{Deserialize, Serialize};
use crate::wire::{WireRead, WireWrite};
use sonium_common::error::Result;

/// Volume or mute change initiated by the client (e.g. from the web UI).
///
/// After the server processes this message it should reflect the new settings
/// back via [`super::ServerSettings`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Desired volume (0 – 100).
    pub volume: u8,
    /// Whether playback should be muted.
    pub muted:  bool,
}

impl ClientInfo {
    /// Deserialise from a wire payload slice.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = WireRead::new(payload);
        let json  = r.read_str()?;
        serde_json::from_str(&json)
            .map_err(|e| sonium_common::SoniumError::Protocol(format!("ClientInfo JSON: {e}")))
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
    fn round_trip_unmuted() {
        let orig    = ClientInfo { volume: 75, muted: false };
        let decoded = ClientInfo::decode(&orig.encode()).unwrap();
        assert_eq!(decoded, orig);
    }

    #[test]
    fn round_trip_muted() {
        let orig    = ClientInfo { volume: 0, muted: true };
        let decoded = ClientInfo::decode(&orig.encode()).unwrap();
        assert_eq!(decoded, orig);
    }

    #[test]
    fn round_trip_max_volume() {
        let orig    = ClientInfo { volume: 100, muted: false };
        let decoded = ClientInfo::decode(&orig.encode()).unwrap();
        assert_eq!(decoded.volume, 100);
    }
}
