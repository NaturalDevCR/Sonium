//! `TimeMsg` — NTP-like clock synchronisation.
//!
//! ## Protocol flow
//!
//! ```text
//! Client                          Server
//!   │─── Time { latency: 0 } ────►│   (client records t_sent)
//!   │                              │   server sets latency = recv_time - sent_time
//!   │◄─── Time { latency: Δ } ────│   (client records t_recv)
//!   │
//!   │  rtt    = t_recv - t_sent
//!   │  c2s    = Δ  (server-measured one-way latency)
//!   │  s2c    = rtt - c2s
//!   │  offset = (c2s - s2c) / 2   → fed into median filter
//! ```
//!
//! ## Payload encoding
//!
//! ```text
//! i32  latency_sec
//! i32  latency_usec
//! ```

use crate::header::Timestamp;
use crate::wire::{WireRead, WireWrite};
use sonium_common::error::Result;

/// Clock synchronisation message, used in both directions.
///
/// The client sends this with `latency` zeroed.  The server fills `latency`
/// with the client-to-server transit time and echoes the message back.  The
/// client then computes the full round-trip and updates its clock offset.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeMsg {
    /// Server-measured client-to-server latency (filled by the server on
    /// echo; zero in the initial request).
    pub latency: Timestamp,
}

impl TimeMsg {
    /// Create a fresh clock-sync request (latency zeroed).
    pub fn zero() -> Self {
        Self {
            latency: Timestamp::default(),
        }
    }

    /// Deserialise from a wire payload slice.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = WireRead::new(payload);
        let sec = r.read_i32()?;
        let usec = r.read_i32()?;
        Ok(Self {
            latency: Timestamp { sec, usec },
        })
    }

    /// Serialise to a wire payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = WireWrite::with_capacity(8);
        w.write_i32(self.latency.sec);
        w.write_i32(self.latency.usec);
        w.finish()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_round_trip() {
        let msg = TimeMsg::zero();
        let back = TimeMsg::decode(&msg.encode()).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn nonzero_latency_round_trip() {
        let msg = TimeMsg {
            latency: Timestamp {
                sec: 0,
                usec: 7_500,
            },
        };
        let back = TimeMsg::decode(&msg.encode()).unwrap();
        assert_eq!(back.latency.sec, 0);
        assert_eq!(back.latency.usec, 7_500);
    }

    #[test]
    fn payload_is_exactly_8_bytes() {
        assert_eq!(TimeMsg::zero().encode().len(), 8);
    }

    #[test]
    fn truncated_payload_returns_error() {
        assert!(TimeMsg::decode(&[0u8; 7]).is_err());
    }
}
