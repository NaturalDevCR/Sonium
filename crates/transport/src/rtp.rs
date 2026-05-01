//! RTP packet encode/decode for Sonium's UDP media path (Phase 2).
//!
//! ## Wire format (RFC 3550, 12-byte fixed header)
//!
//! ```text
//! 0                   1                   2                   3
//! 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |V=2|P|X|  CC   |M|     PT      |       sequence number         |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                           timestamp                           |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |           synchronization source (SSRC) identifier           |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |  payload bytes …                                              |
//! ```
//!
//! Sonium uses PT=96 (dynamic) and a 90 kHz timestamp clock.  The payload
//! is the raw WireChunk bytes (excluding the 26-byte Sonium message header),
//! so the client can call [`WireChunk::decode`] directly on `packet.payload`.

use anyhow::{anyhow, Result};

/// Size of the fixed RTP header in bytes.
pub const RTP_HEADER_SIZE: usize = 12;

/// Dynamic payload type for Sonium audio frames.
pub const SONIUM_RTP_PAYLOAD_TYPE: u8 = 96;

/// RTP timestamp clock rate (90 000 Hz — standard for compressed media).
pub const RTP_CLOCK_RATE: u64 = 90_000;

/// Minimum length of a `wire_bytes` slice accepted by [`rtp_from_wire_bytes`]:
/// 26-byte Sonium message header + 8-byte WireChunk timestamp (sec + usec).
const MIN_WIRE_BYTES_LEN: usize = 34;

/// A single RTP packet produced and consumed by the UDP media path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpPacket {
    /// Monotonically increasing sequence number (wraps at u16::MAX).
    pub sequence: u16,
    /// Playout timestamp in 90 kHz units, derived from the WireChunk timestamp.
    pub timestamp: u32,
    /// Synchronisation source identifier — randomly generated per-session.
    pub ssrc: u32,
    /// WireChunk payload bytes (sec + usec + data_size + encoded audio).
    /// The client calls `WireChunk::decode(&packet.payload)` directly.
    pub payload: Vec<u8>,
}

impl RtpPacket {
    /// Encode to a byte buffer ready for [`UdpSocket::send_to`].
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(RTP_HEADER_SIZE + self.payload.len());
        buf.push(0x80); // V=2 P=0 X=0 CC=0
        buf.push(SONIUM_RTP_PAYLOAD_TYPE); // M=0, PT=96
        buf.extend_from_slice(&self.sequence.to_be_bytes());
        buf.extend_from_slice(&self.timestamp.to_be_bytes());
        buf.extend_from_slice(&self.ssrc.to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Decode from a received UDP datagram.
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < RTP_HEADER_SIZE {
            return Err(anyhow!(
                "RTP packet too short: {} < {RTP_HEADER_SIZE}",
                data.len()
            ));
        }
        if data[0] >> 6 != 2 {
            return Err(anyhow!("unsupported RTP version {}", data[0] >> 6));
        }
        let payload_type = data[1] & 0x7F;
        if payload_type != SONIUM_RTP_PAYLOAD_TYPE {
            return Err(anyhow!(
                "unexpected RTP payload type {payload_type}; expected {SONIUM_RTP_PAYLOAD_TYPE}"
            ));
        }
        let sequence = u16::from_be_bytes([data[2], data[3]]);
        let timestamp = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ssrc = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        Ok(Self {
            sequence,
            timestamp,
            ssrc,
            payload: data[RTP_HEADER_SIZE..].to_vec(),
        })
    }
}

/// Build an [`RtpPacket`] from a Sonium `wire_bytes` slice.
///
/// `wire_bytes` is the fully-framed buffer the broadcaster produces:
/// 26-byte Sonium message header followed by the WireChunk payload.
///
/// The WireChunk payload starts with `i32 sec + i32 usec` (little-endian),
/// which is converted to a 90 kHz RTP timestamp.  The packet payload is set
/// to `wire_bytes[26..]`, so the client can call `WireChunk::decode` directly.
pub fn rtp_from_wire_bytes(wire_bytes: &[u8], sequence: u16, ssrc: u32) -> Result<RtpPacket> {
    if wire_bytes.len() < MIN_WIRE_BYTES_LEN {
        return Err(anyhow!(
            "wire_bytes too short for RTP conversion: {} < {MIN_WIRE_BYTES_LEN}",
            wire_bytes.len()
        ));
    }

    let sec = i32::from_le_bytes([
        wire_bytes[26],
        wire_bytes[27],
        wire_bytes[28],
        wire_bytes[29],
    ]);
    let usec = i32::from_le_bytes([
        wire_bytes[30],
        wire_bytes[31],
        wire_bytes[32],
        wire_bytes[33],
    ]);

    Ok(RtpPacket {
        sequence,
        timestamp: rtp_timestamp(sec, usec),
        ssrc,
        payload: wire_bytes[26..].to_vec(),
    })
}

/// Convert a `(seconds, microseconds)` pair to a 90 kHz RTP timestamp (u32, wraps).
pub fn rtp_timestamp(sec: i32, usec: i32) -> u32 {
    let ts = (sec as i64 * RTP_CLOCK_RATE as i64)
        + (usec as i64 * RTP_CLOCK_RATE as i64 / 1_000_000);
    ts as u32
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_packet() -> RtpPacket {
        RtpPacket {
            sequence: 42,
            timestamp: 12345,
            ssrc: 0xDEAD_BEEF,
            payload: vec![1, 2, 3, 4, 5],
        }
    }

    #[test]
    fn encode_decode_round_trip() {
        let pkt = sample_packet();
        let enc = pkt.encode();
        assert_eq!(enc.len(), RTP_HEADER_SIZE + 5);
        let dec = RtpPacket::decode(&enc).unwrap();
        assert_eq!(dec.sequence, 42);
        assert_eq!(dec.timestamp, 12345);
        assert_eq!(dec.ssrc, 0xDEAD_BEEF);
        assert_eq!(dec.payload, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn header_byte_layout() {
        let pkt = RtpPacket {
            sequence: 1,
            timestamp: 0,
            ssrc: 0,
            payload: vec![],
        };
        let enc = pkt.encode();
        assert_eq!(enc[0], 0x80, "V=2 P=0 X=0 CC=0");
        assert_eq!(enc[1], SONIUM_RTP_PAYLOAD_TYPE, "M=0 PT=96");
        assert_eq!(&enc[2..4], &[0x00, 0x01], "sequence big-endian");
    }

    #[test]
    fn decode_too_short_returns_error() {
        assert!(RtpPacket::decode(&[0u8; RTP_HEADER_SIZE - 1]).is_err());
    }

    #[test]
    fn decode_rejects_wrong_version() {
        let mut enc = sample_packet().encode();
        enc[0] = 0x40; // V=1
        assert!(RtpPacket::decode(&enc).is_err());
    }

    #[test]
    fn decode_rejects_wrong_payload_type() {
        let mut enc = sample_packet().encode();
        enc[1] = 97;
        assert!(RtpPacket::decode(&enc).is_err());
    }

    #[test]
    fn rtp_timestamp_one_second() {
        assert_eq!(rtp_timestamp(1, 0), 90_000);
    }

    #[test]
    fn rtp_timestamp_half_second() {
        assert_eq!(rtp_timestamp(0, 500_000), 45_000);
    }

    #[test]
    fn rtp_from_wire_bytes_correct_payload_and_timestamp() {
        // 26-byte Sonium header (zeroed) + WireChunk payload
        let mut wire = vec![0u8; 26];
        wire.extend_from_slice(&1i32.to_le_bytes()); // sec = 1
        wire.extend_from_slice(&0i32.to_le_bytes()); // usec = 0
        wire.extend_from_slice(&4u32.to_le_bytes()); // data_size = 4
        wire.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);

        let pkt = rtp_from_wire_bytes(&wire, 5, 0x1234_5678).unwrap();
        assert_eq!(pkt.sequence, 5);
        assert_eq!(pkt.timestamp, 90_000); // 1 sec = 90000
        assert_eq!(pkt.ssrc, 0x1234_5678);
        assert_eq!(pkt.payload, &wire[26..]);
    }

    #[test]
    fn rtp_from_wire_bytes_too_short() {
        assert!(rtp_from_wire_bytes(&[0u8; 33], 0, 0).is_err());
    }

    #[test]
    fn sequence_wraps_at_u16_max() {
        assert_eq!(u16::MAX.wrapping_add(1), 0);
    }

    #[test]
    fn empty_payload_encodes_to_header_only() {
        let pkt = RtpPacket {
            sequence: 0,
            timestamp: 0,
            ssrc: 0,
            payload: vec![],
        };
        assert_eq!(pkt.encode().len(), RTP_HEADER_SIZE);
    }
}
