//! `ErrorMsg` — error notification from the server.

use crate::wire::{WireRead, WireWrite};
use sonium_common::error::Result;

/// Error notification sent by the server.
///
/// Common error codes:
/// - `401` — authentication required / invalid credentials
/// - `403` — operation not permitted
/// - `404` — requested resource not found
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorMsg {
    /// Numeric error code (HTTP-like conventions).
    pub code: u32,
    /// Short human-readable error description.
    pub message: String,
    /// Optional extended detail (may be empty).
    pub detail: String,
}

impl ErrorMsg {
    /// Create an error with a code and message, no detail.
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: String::new(),
        }
    }

    /// Deserialise from a wire payload slice.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = WireRead::new(payload);
        let code = r.read_u32()?;
        let message = r.read_str()?;
        let detail = if r.remaining() > 0 {
            r.read_str()?
        } else {
            String::new()
        };
        Ok(Self {
            code,
            message,
            detail,
        })
    }

    /// Serialise to a wire payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = WireWrite::with_capacity(12 + self.message.len() + self.detail.len());
        w.write_u32(self.code);
        w.write_str(&self.message);
        w.write_str(&self.detail);
        w.finish()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_detail() {
        let orig = ErrorMsg {
            code: 401,
            message: "Unauthorized".into(),
            detail: "bad token".into(),
        };
        let decoded = ErrorMsg::decode(&orig.encode()).unwrap();
        assert_eq!(decoded, orig);
    }

    #[test]
    fn round_trip_no_detail() {
        let orig = ErrorMsg::new(403, "Forbidden");
        let decoded = ErrorMsg::decode(&orig.encode()).unwrap();
        assert_eq!(decoded.code, 403);
        assert_eq!(decoded.message, "Forbidden");
        assert_eq!(decoded.detail, "");
    }

    #[test]
    fn truncated_payload_returns_error() {
        assert!(ErrorMsg::decode(&[0u8; 3]).is_err());
    }
}
