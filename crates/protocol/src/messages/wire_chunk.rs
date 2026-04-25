//! `WireChunk` message — one encoded audio frame with its playout timestamp.
//!
//! ## Payload encoding
//!
//! ```text
//! i32  timestamp_sec   (server-clock playout time)
//! i32  timestamp_usec
//! u32  data_size
//! u8[] data[data_size]  (Opus / FLAC / PCM bytes)
//! ```

use crate::wire::{WireRead, WireWrite};
use crate::header::Timestamp;
use sonium_common::error::Result;

/// One encoded audio frame delivered by the server to all clients.
///
/// The `timestamp` is an **absolute playout time** in the server's clock.
/// The client converts it to local time using the offset estimated by
/// [`crate::messages::TimeMsg`] exchanges, then schedules playback for
/// that moment ± latency budget.
#[derive(Debug, Clone, PartialEq)]
pub struct WireChunk {
    /// Absolute playout time in server clock.
    pub timestamp: Timestamp,
    /// Encoded audio bytes (Opus packet, FLAC block, or raw PCM i16 LE).
    pub data: Vec<u8>,
}

impl WireChunk {
    /// Construct a new chunk.
    pub fn new(timestamp: Timestamp, data: Vec<u8>) -> Self {
        Self { timestamp, data }
    }

    /// Deserialise from a wire payload slice.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = WireRead::new(payload);
        let sec  = r.read_i32()?;
        let usec = r.read_i32()?;
        let data = r.read_blob()?;
        Ok(Self { timestamp: Timestamp { sec, usec }, data })
    }

    /// Serialise to a wire payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = WireWrite::with_capacity(12 + self.data.len());
        w.write_i32(self.timestamp.sec);
        w.write_i32(self.timestamp.usec);
        w.write_blob(&self.data);
        w.finish()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_small_payload() {
        let chunk   = WireChunk::new(Timestamp { sec: 1_700_000_000, usec: 500_000 }, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let decoded = WireChunk::decode(&chunk.encode()).unwrap();
        assert_eq!(decoded, chunk);
    }

    #[test]
    fn round_trip_empty_data() {
        let chunk   = WireChunk::new(Timestamp::default(), vec![]);
        let decoded = WireChunk::decode(&chunk.encode()).unwrap();
        assert_eq!(decoded.data, Vec::<u8>::new());
    }

    #[test]
    fn round_trip_large_payload() {
        let data    = vec![0xAAu8; 4096];
        let chunk   = WireChunk::new(Timestamp::now(), data.clone());
        let decoded = WireChunk::decode(&chunk.encode()).unwrap();
        assert_eq!(decoded.data, data);
    }

    #[test]
    fn timestamp_preserved() {
        let ts    = Timestamp { sec: 1_234_567_890, usec: 999_999 };
        let chunk = WireChunk::new(ts, vec![1, 2, 3]);
        let back  = WireChunk::decode(&chunk.encode()).unwrap();
        assert_eq!(back.timestamp.sec,  ts.sec);
        assert_eq!(back.timestamp.usec, ts.usec);
    }

    #[test]
    fn truncated_payload_returns_error() {
        // Only 7 bytes — not enough for the 12-byte header (sec + usec + size)
        assert!(WireChunk::decode(&[0u8; 7]).is_err());
    }

    #[test]
    fn payload_size_mismatch_returns_error() {
        let mut raw = WireChunk::new(Timestamp::default(), vec![1, 2, 3]).encode();
        // Corrupt the data_size field to claim more data than available
        let size_offset = 8; // after i32+i32
        let bad_size    = 9999u32.to_le_bytes();
        raw[size_offset..size_offset + 4].copy_from_slice(&bad_size);
        assert!(WireChunk::decode(&raw).is_err());
    }
}
