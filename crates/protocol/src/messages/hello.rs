//! `Hello` message — client introduction sent immediately after TCP connect.
//!
//! ## Payload encoding
//!
//! The payload is a length-prefixed UTF-8 JSON string:
//!
//! ```text
//! u32  json_length
//! u8[] json_bytes[json_length]
//! ```

use serde::{Deserialize, Serialize};
use crate::wire::{WireRead, WireWrite};
use sonium_common::error::Result;

/// Client hello sent as the first message after establishing a TCP connection.
///
/// The server uses the `id` field as a stable client identifier across
/// reconnects.  The `hostname` is shown in the web UI and log output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hello {
    /// MAC address of the primary network interface (informational).
    #[serde(rename = "MAC")]
    pub mac: String,
    /// Human-readable hostname of the client machine.
    #[serde(rename = "HostName")]
    pub hostname: String,
    /// Client software version string.
    #[serde(rename = "Version")]
    pub version: String,
    /// Client application name (e.g. `"Sonium"`, `"Snapclient"`).
    #[serde(rename = "ClientName")]
    pub client_name: String,
    /// Operating system name (e.g. `"linux"`, `"macos"`).
    #[serde(rename = "OS")]
    pub os: String,
    /// CPU architecture string (e.g. `"x86_64"`, `"aarch64"`).
    #[serde(rename = "Arch")]
    pub arch: String,
    /// Instance number — allows multiple clients on the same host (default: 1).
    #[serde(rename = "Instance")]
    pub instance: u32,
    /// Stable unique client identifier (survives reboots).
    #[serde(rename = "ID")]
    pub id: String,
    /// Stream protocol version supported by this client (always 2 for Sonium).
    #[serde(rename = "SnapStreamProtocolVersion")]
    pub protocol_version: u32,
}

impl Hello {
    /// Create a `Hello` populated with the current host's metadata.
    pub fn new(hostname: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            mac:              "00:00:00:00:00:00".into(),
            hostname:         hostname.into(),
            version:          env!("CARGO_PKG_VERSION").into(),
            client_name:      "Sonium".into(),
            os:               std::env::consts::OS.into(),
            arch:             std::env::consts::ARCH.into(),
            instance:         1,
            id:               id.into(),
            protocol_version: 2,
        }
    }

    /// Deserialise from a wire payload slice.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = WireRead::new(payload);
        let json = r.read_str()?;
        serde_json::from_str(&json)
            .map_err(|e| sonium_common::SoniumError::Protocol(format!("Hello JSON: {e}")))
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

    fn sample() -> Hello {
        Hello {
            mac:              "aa:bb:cc:dd:ee:ff".into(),
            hostname:         "test-host".into(),
            version:          "0.1.0".into(),
            client_name:      "Sonium".into(),
            os:               "linux".into(),
            arch:             "x86_64".into(),
            instance:         1,
            id:               "unique-id-123".into(),
            protocol_version: 2,
        }
    }

    #[test]
    fn round_trip() {
        let orig    = sample();
        let encoded = orig.encode();
        let decoded = Hello::decode(&encoded).unwrap();
        assert_eq!(decoded, orig);
    }

    #[test]
    fn round_trip_unicode_hostname() {
        let mut h = sample();
        h.hostname = "küche-pi".into();
        let decoded = Hello::decode(&h.encode()).unwrap();
        assert_eq!(decoded.hostname, "küche-pi");
    }

    #[test]
    fn invalid_json_returns_error() {
        // Build a payload with valid length prefix but garbage JSON
        let mut w = WireWrite::new();
        w.write_str("not-valid-json{{{");
        assert!(Hello::decode(&w.finish()).is_err());
    }

    #[test]
    fn empty_payload_returns_error() {
        assert!(Hello::decode(&[]).is_err());
    }

    #[test]
    fn new_helper_sets_protocol_version() {
        let h = Hello::new("host", "id");
        assert_eq!(h.protocol_version, 2);
    }
}
