//! 26-byte message header, shared by every Sonium protocol message.
//!
//! ## Wire layout (all fields little-endian)
//!
//! ```text
//! Offset  Bytes  Type    Field
//! ──────  ─────  ──────  ──────────────────────────────
//!  0       2     u16     Message type  (see [`MessageType`])
//!  2       2     u16     Message ID    (sequence number)
//!  4       2     u16     Refers-to ID  (reply links back to request)
//!  6       4     i32     Sent seconds  (timeval.tv_sec)
//! 10       4     i32     Sent microseconds (timeval.tv_usec)
//! 14       4     i32     Received seconds
//! 18       4     i32     Received microseconds
//! 22       4     u32     Payload size in bytes
//!                        ─────────────────────  total: 26 bytes
//! ```

use sonium_common::SoniumError;

/// Size of the fixed header in bytes.
pub const HEADER_SIZE: usize = 26;

/// Discriminant that identifies the payload type of a message.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageType {
    /// Unused base type.
    Base = 0,
    /// Codec initialisation data — sent once per stream.
    CodecHeader = 1,
    /// One encoded audio frame with its playout timestamp.
    WireChunk = 2,
    /// Server-side volume, mute, and buffer settings.
    ServerSettings = 3,
    /// NTP-like clock synchronisation request/response.
    Time = 4,
    /// Client greeting — first message after TCP connect.
    Hello = 5,
    /// Volume or mute change initiated by the client.
    ClientInfo = 7,
    /// Error notification from the server.
    ErrorMsg = 8,
}

impl TryFrom<u16> for MessageType {
    type Error = SoniumError;
    fn try_from(v: u16) -> Result<Self, <Self as TryFrom<u16>>::Error> {
        match v {
            0 => Ok(Self::Base),
            1 => Ok(Self::CodecHeader),
            2 => Ok(Self::WireChunk),
            3 => Ok(Self::ServerSettings),
            4 => Ok(Self::Time),
            5 => Ok(Self::Hello),
            7 => Ok(Self::ClientInfo),
            8 => Ok(Self::ErrorMsg),
            n => Err(SoniumError::Protocol(format!("unknown message type {n}"))),
        }
    }
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Base => "Base",
            Self::CodecHeader => "CodecHeader",
            Self::WireChunk => "WireChunk",
            Self::ServerSettings => "ServerSettings",
            Self::Time => "Time",
            Self::Hello => "Hello",
            Self::ClientInfo => "ClientInfo",
            Self::ErrorMsg => "Error",
        };
        f.write_str(s)
    }
}

/// A `(seconds, microseconds)` timestamp embedded in every message header.
///
/// Mirrors POSIX `struct timeval`.  The server uses the `sent` / `received`
/// pair to compute round-trip latency for clock synchronisation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Timestamp {
    /// Seconds since the Unix epoch.
    pub sec: i32,
    /// Microseconds component (0 – 999 999).
    pub usec: i32,
}

impl Timestamp {
    /// Capture the current wall-clock time.
    pub fn now() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let d = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            sec: d.as_secs() as i32,
            usec: d.subsec_micros() as i32,
        }
    }

    /// Convert to a single microsecond value (since the Unix epoch).
    pub fn to_micros(&self) -> i64 {
        self.sec as i64 * 1_000_000 + self.usec as i64
    }

    /// Reconstruct from a microsecond value.
    pub fn from_micros(us: i64) -> Self {
        Self {
            sec: (us / 1_000_000) as i32,
            usec: (us % 1_000_000) as i32,
        }
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{:06}", self.sec, self.usec)
    }
}

/// The 26-byte header that precedes every message payload.
#[derive(Debug, Clone)]
pub struct MessageHeader {
    /// Payload type.
    pub msg_type: MessageType,
    /// Monotonically increasing sender sequence number.
    pub id: u16,
    /// For replies: the `id` of the message being answered.
    pub refers_to: u16,
    /// Wall-clock time when the message was sent.
    pub sent: Timestamp,
    /// Wall-clock time when the message was received (filled by the receiver).
    pub received: Timestamp,
    /// Number of payload bytes that follow this header.
    pub payload_size: u32,
}

impl MessageHeader {
    /// Create a header for an outgoing message with `sent` set to now.
    pub fn new(msg_type: MessageType, payload_size: u32) -> Self {
        Self {
            msg_type,
            id: 0,
            refers_to: 0,
            sent: Timestamp::now(),
            received: Timestamp::default(),
            payload_size,
        }
    }

    /// Deserialise from a 26-byte little-endian slice.
    ///
    /// Returns [`SoniumError::Protocol`] if the slice is too short or the
    /// message type is unknown.
    pub fn from_bytes(b: &[u8]) -> sonium_common::error::Result<Self> {
        if b.len() < HEADER_SIZE {
            return Err(SoniumError::Protocol(format!(
                "header too short: {} < {HEADER_SIZE}",
                b.len()
            )));
        }
        let msg_type = MessageType::try_from(u16::from_le_bytes([b[0], b[1]]))?;
        let id = u16::from_le_bytes([b[2], b[3]]);
        let refers_to = u16::from_le_bytes([b[4], b[5]]);
        let sent_sec = i32::from_le_bytes([b[6], b[7], b[8], b[9]]);
        let sent_usec = i32::from_le_bytes([b[10], b[11], b[12], b[13]]);
        let recv_sec = i32::from_le_bytes([b[14], b[15], b[16], b[17]]);
        let recv_usec = i32::from_le_bytes([b[18], b[19], b[20], b[21]]);
        let payload_size = u32::from_le_bytes([b[22], b[23], b[24], b[25]]);

        Ok(Self {
            msg_type,
            id,
            refers_to,
            sent: Timestamp {
                sec: sent_sec,
                usec: sent_usec,
            },
            received: Timestamp {
                sec: recv_sec,
                usec: recv_usec,
            },
            payload_size,
        })
    }

    /// Serialise to a 26-byte little-endian array.
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut b = [0u8; HEADER_SIZE];
        b[0..2].copy_from_slice(&(self.msg_type as u16).to_le_bytes());
        b[2..4].copy_from_slice(&self.id.to_le_bytes());
        b[4..6].copy_from_slice(&self.refers_to.to_le_bytes());
        b[6..10].copy_from_slice(&self.sent.sec.to_le_bytes());
        b[10..14].copy_from_slice(&self.sent.usec.to_le_bytes());
        b[14..18].copy_from_slice(&self.received.sec.to_le_bytes());
        b[18..22].copy_from_slice(&self.received.usec.to_le_bytes());
        b[22..26].copy_from_slice(&self.payload_size.to_le_bytes());
        b
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── MessageType ──────────────────────────────────────────────────────────

    #[test]
    fn message_type_round_trip_all_variants() {
        let variants = [
            (0u16, MessageType::Base),
            (1, MessageType::CodecHeader),
            (2, MessageType::WireChunk),
            (3, MessageType::ServerSettings),
            (4, MessageType::Time),
            (5, MessageType::Hello),
            (7, MessageType::ClientInfo),
            (8, MessageType::ErrorMsg),
        ];
        for (raw, expected) in variants {
            let got = MessageType::try_from(raw).expect("should parse");
            assert_eq!(got, expected, "type id {raw}");
            // The discriminant must match the raw value
            assert_eq!(got as u16, raw);
        }
    }

    #[test]
    fn unknown_type_returns_error() {
        // Type 6 is not assigned in the Sonium protocol
        assert!(MessageType::try_from(6u16).is_err());
        assert!(MessageType::try_from(9u16).is_err());
        assert!(MessageType::try_from(u16::MAX).is_err());
    }

    // ── Timestamp ────────────────────────────────────────────────────────────

    #[test]
    fn timestamp_micros_round_trip() {
        let ts = Timestamp {
            sec: 1_700_000_000,
            usec: 123_456,
        };
        let us = ts.to_micros();
        let back = Timestamp::from_micros(us);
        assert_eq!(back.sec, ts.sec);
        assert_eq!(back.usec, ts.usec);
    }

    #[test]
    fn timestamp_zero() {
        let ts = Timestamp::default();
        assert_eq!(ts.to_micros(), 0);
    }

    #[test]
    fn timestamp_now_is_positive() {
        let ts = Timestamp::now();
        assert!(ts.sec > 1_700_000_000, "clock looks wrong: sec={}", ts.sec);
        assert!(ts.usec >= 0);
    }

    // ── MessageHeader ─────────────────────────────────────────────────────────

    #[test]
    fn header_encode_decode_round_trip() {
        let hdr = MessageHeader {
            msg_type: MessageType::WireChunk,
            id: 42,
            refers_to: 0,
            sent: Timestamp {
                sec: 1_700_000_000,
                usec: 123_456,
            },
            received: Timestamp {
                sec: 1_700_000_000,
                usec: 200_000,
            },
            payload_size: 512,
        };
        let bytes = hdr.to_bytes();
        assert_eq!(bytes.len(), HEADER_SIZE);

        let parsed = MessageHeader::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.msg_type, MessageType::WireChunk);
        assert_eq!(parsed.id, 42);
        assert_eq!(parsed.refers_to, 0);
        assert_eq!(parsed.sent.sec, 1_700_000_000);
        assert_eq!(parsed.sent.usec, 123_456);
        assert_eq!(parsed.received.sec, 1_700_000_000);
        assert_eq!(parsed.received.usec, 200_000);
        assert_eq!(parsed.payload_size, 512);
    }

    #[test]
    fn header_all_message_types_encode() {
        for &t in &[
            MessageType::CodecHeader,
            MessageType::WireChunk,
            MessageType::ServerSettings,
            MessageType::Time,
            MessageType::Hello,
            MessageType::ClientInfo,
            MessageType::ErrorMsg,
        ] {
            let hdr = MessageHeader::new(t, 0);
            let bytes = hdr.to_bytes();
            let back = MessageHeader::from_bytes(&bytes).unwrap();
            assert_eq!(back.msg_type, t);
        }
    }

    #[test]
    fn header_too_short_returns_error() {
        let short = [0u8; HEADER_SIZE - 1];
        assert!(MessageHeader::from_bytes(&short).is_err());
    }

    #[test]
    fn header_little_endian_byte_order() {
        // type = 2 (WireChunk) → bytes [0x02, 0x00]
        let hdr = MessageHeader::new(MessageType::WireChunk, 0);
        let b = hdr.to_bytes();
        assert_eq!(b[0], 0x02, "type LSB");
        assert_eq!(b[1], 0x00, "type MSB");
    }

    #[test]
    fn header_payload_size_zero() {
        let hdr = MessageHeader::new(MessageType::Hello, 0);
        let back = MessageHeader::from_bytes(&hdr.to_bytes()).unwrap();
        assert_eq!(back.payload_size, 0);
    }

    #[test]
    fn header_max_payload_size() {
        let hdr = MessageHeader::new(MessageType::WireChunk, u32::MAX);
        let back = MessageHeader::from_bytes(&hdr.to_bytes()).unwrap();
        assert_eq!(back.payload_size, u32::MAX);
    }
}
