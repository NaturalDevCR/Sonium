//! ARQ (Automatic Repeat reQuest) packet types and wire format for
//! reliable audio delivery over UDP.
//!
//! # Protocol Overview
//!
//! Sonium ARQ adds reliability on top of RTP-style UDP audio delivery
//! without depending on external C libraries like libRIST.  Three packet
//! types share the same UDP socket:
//!
//! | Magic | Direction      | Type   | Purpose                          |
//! |-------|----------------|--------|----------------------------------|
//! | 0x80  | server → client | Audio  | RTP-style audio frame            |
//! | 0xC1  | client → server | NACK   | Request retransmission           |
//! | 0xC2  | server → client | FEC    | XOR parity for packet recovery   |
//!
//! Audio packets use the same format as [`crate::rtp::RtpPacket`] (12-byte
//! RTP header + payload).  NACK and FEC packets use a compact Sonium-native
//! format identified by the magic byte.
//!
//! # Retransmission
//!
//! The server maintains a sliding window buffer of recently sent audio
//! packets.  When the client detects a gap in sequence numbers, it sends
//! a NACK listing the missing range.  The server retransmits the requested
//! packets as fresh Audio packets (same wire format, so the client's
//! existing RTP decoder handles them transparently).
//!
//! # Forward Error Correction (FEC)
//!
//! Every [`FEC_GROUP_SIZE`] audio packets, the server sends one XOR parity
//! packet covering the entire group.  The client can recover a single
//! missing packet per group without waiting for a retransmission round-trip.
//! This is especially effective for burst losses of 1-2 packets.

use anyhow::{anyhow, Result};

// ── Packet type magic bytes ──────────────────────────────────────────────────

/// Audio packet magic byte.  This is `0x80` which matches RTP V=2, so
/// standard RTP decoders (and our existing `RtpPacket::decode`) can parse
/// audio packets without any special handling.
pub const ARQ_MAGIC_AUDIO: u8 = 0x80;

/// NACK packet magic byte (client → server).
pub const ARQ_MAGIC_NACK: u8 = 0xC1;

/// FEC parity packet magic byte (server → client).
pub const ARQ_MAGIC_FEC: u8 = 0xC2;

// ── NACK wire format ─────────────────────────────────────────────────────────

/// Fixed header size of a NACK packet (no ranges).
///
/// Layout:
/// ```text
/// [0]     magic (0xC1)
/// [1..5]  ssrc (u32 BE)
/// [5]     num_ranges (u8)
/// [6..]   ranges: each 4 bytes (start u16 BE + count u16 BE)
/// ```
pub const NACK_HEADER_SIZE: usize = 6;

/// Maximum number of ranges in a single NACK packet.
pub const NACK_MAX_RANGES: usize = 16;

/// A NACK (Negative Acknowledgement) sent from client to server.
///
/// Lists one or more contiguous ranges of missing audio sequence numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NackPacket {
    /// Client's SSRC (echoed from the audio stream for routing).
    pub ssrc: u32,
    /// Contiguous ranges of missing sequence numbers.
    /// Each entry is `(first_missing_seq, count)`.
    pub ranges: Vec<(u16, u16)>,
}

impl NackPacket {
    /// Encode to bytes for UDP transmission.
    pub fn encode(&self) -> Vec<u8> {
        let num_ranges = self.ranges.len().min(NACK_MAX_RANGES);
        let mut buf = Vec::with_capacity(NACK_HEADER_SIZE + num_ranges * 4);
        buf.push(ARQ_MAGIC_NACK);
        buf.extend_from_slice(&self.ssrc.to_be_bytes());
        // Number of ranges
        buf.push(num_ranges as u8);
        for &(start, count) in &self.ranges[..num_ranges] {
            buf.extend_from_slice(&start.to_be_bytes());
            buf.extend_from_slice(&count.to_be_bytes());
        }
        buf
    }

    /// Decode from received UDP datagram.
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < NACK_HEADER_SIZE {
            return Err(anyhow!(
                "NACK packet too short: {} < {NACK_HEADER_SIZE}",
                data.len()
            ));
        }
        if data[0] != ARQ_MAGIC_NACK {
            return Err(anyhow!(
                "not a NACK packet: magic 0x{:02X} != 0x{:02X}",
                data[0],
                ARQ_MAGIC_NACK
            ));
        }
        let ssrc = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
        let num_ranges = data[5] as usize;
        if num_ranges > NACK_MAX_RANGES {
            return Err(anyhow!(
                "NACK has {num_ranges} ranges, max is {NACK_MAX_RANGES}"
            ));
        }
        let expected = NACK_HEADER_SIZE + num_ranges * 4;
        if data.len() < expected {
            return Err(anyhow!(
                "NACK packet too short for {num_ranges} ranges: {} < {expected}",
                data.len()
            ));
        }
        let mut ranges = Vec::with_capacity(num_ranges);
        let mut offset = 6;
        for _ in 0..num_ranges {
            let start = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let count = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
            ranges.push((start, count));
            offset += 4;
        }
        Ok(Self { ssrc, ranges })
    }

    /// Total number of missing sequence numbers requested.
    pub fn total_missing(&self) -> u32 {
        self.ranges.iter().map(|&(_, c)| c as u32).sum()
    }
}

// ── FEC wire format ──────────────────────────────────────────────────────────

/// Number of audio packets protected by one FEC packet.
pub const FEC_GROUP_SIZE: u16 = 5;

/// Fixed header size of a FEC packet.
///
/// Layout:
/// ```text
/// [0]      magic (0xC2)
/// [1..3]   base_seq (u16 BE) — first audio seq in this group
/// [3..5]   count (u16 BE) — number of audio packets in group
/// [5..9]   ssrc (u32 BE)
/// [9..11]  max_payload_size (u16 BE) — size of XOR data
/// [11..]   xor_payload (variable)
/// ```
pub const FEC_HEADER_SIZE: usize = 11;

/// A FEC (Forward Error Correction) parity packet.
///
/// Contains the XOR of all audio packet payloads in a group, enabling
/// recovery of any single missing packet without retransmission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FecPacket {
    /// Sequence number of the first audio packet in this group.
    pub base_seq: u16,
    /// Number of audio packets in the group (typically [`FEC_GROUP_SIZE`]).
    pub count: u16,
    /// SSRC of the audio stream.
    pub ssrc: u32,
    /// XOR parity data (length = max payload size in the group).
    pub xor_payload: Vec<u8>,
}

impl FecPacket {
    /// Encode to bytes for UDP transmission.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(FEC_HEADER_SIZE + self.xor_payload.len());
        buf.push(ARQ_MAGIC_FEC);
        buf.extend_from_slice(&self.base_seq.to_be_bytes());
        buf.extend_from_slice(&self.count.to_be_bytes());
        buf.extend_from_slice(&self.ssrc.to_be_bytes());
        buf.extend_from_slice(&(self.xor_payload.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.xor_payload);
        buf
    }

    /// Decode from received UDP datagram.
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < FEC_HEADER_SIZE {
            return Err(anyhow!(
                "FEC packet too short: {} < {FEC_HEADER_SIZE}",
                data.len()
            ));
        }
        if data[0] != ARQ_MAGIC_FEC {
            return Err(anyhow!(
                "not a FEC packet: magic 0x{:02X} != 0x{:02X}",
                data[0],
                ARQ_MAGIC_FEC
            ));
        }
        let base_seq = u16::from_be_bytes([data[1], data[2]]);
        let count = u16::from_be_bytes([data[3], data[4]]);
        let ssrc = u32::from_be_bytes([data[5], data[6], data[7], data[8]]);
        let payload_size = u16::from_be_bytes([data[9], data[10]]) as usize;
        let expected = FEC_HEADER_SIZE + payload_size;
        if data.len() < expected {
            return Err(anyhow!(
                "FEC packet too short for {payload_size} bytes of payload: {} < {expected}",
                data.len()
            ));
        }
        Ok(Self {
            base_seq,
            count,
            ssrc,
            xor_payload: data[FEC_HEADER_SIZE..expected].to_vec(),
        })
    }

    /// Returns `true` if `seq` falls within this FEC group.
    pub fn covers_seq(&self, seq: u16) -> bool {
        let end = self.base_seq.wrapping_add(self.count);
        if self.base_seq <= end {
            seq >= self.base_seq && seq < end
        } else {
            // Wrapped around u16::MAX
            seq >= self.base_seq || seq < end
        }
    }

    /// Returns the sequence number range `[base_seq, base_seq + count)`.
    pub fn seq_range(&self) -> (u16, u16) {
        (self.base_seq, self.base_seq.wrapping_add(self.count))
    }
}

// ── Packet type detection ────────────────────────────────────────────────────

/// Detect the ARQ packet type from the first byte of a UDP datagram.
///
/// Returns `None` if the packet is not a recognized ARQ type (caller should
/// treat it as a raw/unknown packet).
pub fn detect_packet_type(data: &[u8]) -> Option<ArqPacketType> {
    if data.is_empty() {
        return None;
    }
    match data[0] {
        ARQ_MAGIC_AUDIO => Some(ArqPacketType::Audio),
        ARQ_MAGIC_NACK => Some(ArqPacketType::Nack),
        ARQ_MAGIC_FEC => Some(ArqPacketType::Fec),
        _ => None,
    }
}

/// ARQ packet type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArqPacketType {
    /// Audio frame (same wire format as RTP).
    Audio,
    /// NACK — client requests retransmission.
    Nack,
    /// FEC — XOR parity for a group of audio packets.
    Fec,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nack_round_trip() {
        let nack = NackPacket {
            ssrc: 0xDEAD_BEEF,
            ranges: vec![(100, 3), (200, 1)],
        };
        let encoded = nack.encode();
        let decoded = NackPacket::decode(&encoded).unwrap();
        assert_eq!(decoded, nack);
    }

    #[test]
    fn nack_total_missing() {
        let nack = NackPacket {
            ssrc: 0,
            ranges: vec![(100, 5), (200, 3), (300, 1)],
        };
        assert_eq!(nack.total_missing(), 9);
    }

    #[test]
    fn nack_decode_empty_ranges() {
        let nack = NackPacket {
            ssrc: 42,
            ranges: vec![],
        };
        let encoded = nack.encode();
        let decoded = NackPacket::decode(&encoded).unwrap();
        assert_eq!(decoded.ssrc, 42);
        assert!(decoded.ranges.is_empty());
    }

    #[test]
    fn nack_rejects_wrong_magic() {
        let mut data = vec![0x00; NACK_HEADER_SIZE];
        data[0] = 0xFF;
        assert!(NackPacket::decode(&data).is_err());
    }

    #[test]
    fn nack_rejects_too_short() {
        assert!(NackPacket::decode(&[ARQ_MAGIC_NACK; 5]).is_err());
    }

    #[test]
    fn fec_round_trip() {
        let fec = FecPacket {
            base_seq: 50,
            count: FEC_GROUP_SIZE,
            ssrc: 0x1234_5678,
            xor_payload: vec![0xAA; 200],
        };
        let encoded = fec.encode();
        let decoded = FecPacket::decode(&encoded).unwrap();
        assert_eq!(decoded, fec);
    }

    #[test]
    fn fec_covers_seq_basic() {
        let fec = FecPacket {
            base_seq: 10,
            count: 5,
            ssrc: 0,
            xor_payload: vec![],
        };
        assert!(fec.covers_seq(10));
        assert!(fec.covers_seq(14));
        assert!(!fec.covers_seq(15));
        assert!(!fec.covers_seq(9));
    }

    #[test]
    fn fec_covers_seq_wrapped() {
        let fec = FecPacket {
            base_seq: u16::MAX - 2,
            count: 5,
            ssrc: 0,
            xor_payload: vec![],
        };
        assert!(fec.covers_seq(u16::MAX - 2));
        assert!(fec.covers_seq(u16::MAX - 1));
        assert!(fec.covers_seq(u16::MAX));
        assert!(fec.covers_seq(0));
        assert!(fec.covers_seq(1));
        assert!(!fec.covers_seq(2));
    }

    #[test]
    fn fec_rejects_wrong_magic() {
        let mut data = vec![0x00; FEC_HEADER_SIZE];
        data[0] = 0xFF;
        assert!(FecPacket::decode(&data).is_err());
    }

    #[test]
    fn fec_rejects_too_short() {
        assert!(FecPacket::decode(&[ARQ_MAGIC_FEC; 5]).is_err());
    }

    #[test]
    fn detect_packet_type_audio() {
        assert_eq!(
            detect_packet_type(&[0x80, 0x00]),
            Some(ArqPacketType::Audio)
        );
    }

    #[test]
    fn detect_packet_type_nack() {
        assert_eq!(
            detect_packet_type(&[ARQ_MAGIC_NACK]),
            Some(ArqPacketType::Nack)
        );
    }

    #[test]
    fn detect_packet_type_fec() {
        assert_eq!(
            detect_packet_type(&[ARQ_MAGIC_FEC]),
            Some(ArqPacketType::Fec)
        );
    }

    #[test]
    fn detect_packet_type_unknown() {
        assert_eq!(detect_packet_type(&[0x00]), None);
    }

    #[test]
    fn detect_packet_type_empty() {
        assert_eq!(detect_packet_type(&[]), None);
    }

    #[test]
    fn fec_seq_range_no_wrap() {
        let fec = FecPacket {
            base_seq: 100,
            count: 5,
            ssrc: 0,
            xor_payload: vec![],
        };
        assert_eq!(fec.seq_range(), (100, 105));
    }

    #[test]
    fn fec_seq_range_wraps() {
        let fec = FecPacket {
            base_seq: u16::MAX - 1,
            count: 3,
            ssrc: 0,
            xor_payload: vec![],
        };
        let (start, end) = fec.seq_range();
        assert_eq!(start, u16::MAX - 1);
        assert_eq!(end, 1); // wrapped
    }
}
