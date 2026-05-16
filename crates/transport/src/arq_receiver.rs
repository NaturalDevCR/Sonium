//! Client-side ARQ receiver: gap detection, NACK sending, FEC recovery,
//! and packet reordering.
//!
//! [`ArqReceiver`] wraps a UDP socket and provides a clean interface for
//! the client controller to receive audio frames with ARQ reliability.
//!
//! # Flow
//!
//! 1. UDP datagrams arrive on the shared socket.
//! 2. The receiver classifies each packet (Audio / NACK / FEC / unknown).
//! 3. Audio packets are placed in a reorder buffer.
//! 4. Sequence gaps trigger NACK packets sent back to the server.
//! 5. FEC packets enable single-packet recovery without retransmission.
//! 6. In-order audio frames are delivered to the caller via
//!    [`ArqReceiver::recv_audio`].

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::arq::{detect_packet_type, ArqPacketType, FecPacket, NackPacket, FEC_GROUP_SIZE};
use crate::rtp::RtpPacket;

/// Maximum number of packets in the reorder buffer.
///
/// Packets older than the delivery point by more than this window are dropped.
const REORDER_BUFFER_CAPACITY: usize = 64;

/// How long to wait for a missing packet before sending a NACK.
///
/// This avoids spurious NACKs for packets that arrive slightly out of order.
const NACK_DELAY: Duration = Duration::from_millis(5);

/// Maximum number of times a gap will be NACKed.
const MAX_NACK_RETRIES: u32 = 3;

/// An audio frame ready for delivery from the ARQ receiver.
#[derive(Debug, Clone)]
pub struct ArqAudioFrame {
    /// RTP sequence number.
    pub sequence: u16,
    /// RTP timestamp (90 kHz clock).
    pub timestamp: u32,
    /// Raw WireChunk payload (can be decoded with `WireChunk::decode`).
    pub payload: Vec<u8>,
}

/// Tracks the NACK state for a single missing sequence number.
#[derive(Debug, Clone)]
struct GapState {
    /// When this gap was first detected.
    first_seen: std::time::Instant,
    /// How many times we've sent a NACK for this gap.
    nack_count: u32,
}

/// Client-side ARQ receiver.
pub struct ArqReceiver {
    socket: Arc<UdpSocket>,
    server_addr: SocketAddr,
    ssrc: u32,
    /// Next expected sequence number for in-order delivery.
    next_deliver_seq: Option<u16>,
    /// Reorder buffer: maps sequence number to received packet.
    reorder_buffer: BTreeMap<u16, ArqAudioFrame>,
    /// FEC group cache: maps base_seq to received FEC packet.
    fec_cache: BTreeMap<u16, FecPacket>,
    /// Audio payloads received in the current FEC group (for XOR recovery).
    /// Maps sequence number to payload.
    fec_received_payloads: BTreeMap<u16, Vec<u8>>,
    /// Current FEC group base sequence (tracks which group we're building).
    current_fec_base: Option<u16>,
    /// Active gap tracking: sequence number → state.
    gaps: BTreeMap<u16, GapState>,
    /// Metrics: total packets received.
    packets_received: u64,
    /// Metrics: total gaps detected.
    gaps_detected: u64,
    /// Metrics: total NACKs sent.
    nacks_sent: u64,
    /// Metrics: packets recovered via FEC.
    fec_recovered: u64,
    /// Metrics: packets recovered via retransmission.
    retransmit_received: u64,
}

impl ArqReceiver {
    /// Create a new ARQ receiver.
    ///
    /// `socket` should be the client's already-bound UDP socket.
    /// `server_addr` is the server's UDP address (for sending NACKs).
    /// `ssrc` should match the session's SSRC (received in ServerSettings or
    /// derived from the first audio packet).
    pub fn new(socket: Arc<UdpSocket>, server_addr: SocketAddr, ssrc: u32) -> Self {
        Self {
            socket,
            server_addr,
            ssrc,
            next_deliver_seq: None,
            reorder_buffer: BTreeMap::new(),
            fec_cache: BTreeMap::new(),
            fec_received_payloads: BTreeMap::new(),
            current_fec_base: None,
            gaps: BTreeMap::new(),
            packets_received: 0,
            gaps_detected: 0,
            nacks_sent: 0,
            fec_recovered: 0,
            retransmit_received: 0,
        }
    }

    /// Receive the next in-order audio frame.
    ///
    /// This method:
    /// 1. Tries to deliver from the reorder buffer if the next expected
    ///    packet is already available.
    /// 2. If not, reads from the UDP socket, processing Audio/FEC/NACK
    ///    packets until an audio frame is ready for delivery.
    ///
    /// Returns `None` only on socket close (channel disconnected).
    pub async fn recv_audio(&mut self) -> Result<Option<ArqAudioFrame>> {
        loop {
            // Try to deliver from the reorder buffer first.
            if let Some(frame) = self.try_deliver() {
                return Ok(Some(frame));
            }

            // Read next UDP datagram.
            let mut buf = vec![0u8; 65_535];
            match timeout(Duration::from_millis(100), self.socket.recv(&mut buf)).await {
                Ok(Ok(n)) => {
                    self.process_datagram(&buf[..n]).await?;
                    self.process_nack_timer().await?;
                }
                Ok(Err(e)) => {
                    warn!(error = %e, "ARQ UDP recv error");
                    return Err(e.into());
                }
                Err(_) => {
                    // Timeout — check for gaps that need NACKing.
                    self.process_nack_timer().await?;
                }
            }
        }
    }

    /// Process a single UDP datagram.
    async fn process_datagram(&mut self, data: &[u8]) -> Result<()> {
        let pkt_type = detect_packet_type(data);
        match pkt_type {
            Some(ArqPacketType::Audio) => self.handle_audio_packet(data).await?,
            Some(ArqPacketType::Fec) => self.handle_fec_packet(data).await?,
            Some(ArqPacketType::Nack) => {
                // NACKs are sent by us, not received (unless loopback — ignore).
                debug!("Ignoring NACK packet (likely loopback)");
            }
            None => {
                debug!("Ignoring unknown UDP packet ({} bytes)", data.len());
            }
        }
        Ok(())
    }

    /// Handle an incoming audio (RTP) packet.
    async fn handle_audio_packet(&mut self, data: &[u8]) -> Result<()> {
        let rtp = RtpPacket::decode(data)?;
        let seq = rtp.sequence;

        if self.ssrc == 0 {
            self.ssrc = rtp.ssrc;
        } else if rtp.ssrc != self.ssrc {
            debug!(
                seq,
                ssrc = rtp.ssrc,
                expected_ssrc = self.ssrc,
                "Ignoring ARQ packet for different SSRC"
            );
            return Ok(());
        }

        self.packets_received += 1;

        // Track received payload for FEC group recovery.
        self.track_fec_payload(seq, &rtp.payload);

        // If this was a gap we were tracking, it's been retransmitted.
        if self.gaps.remove(&seq).is_some() {
            self.retransmit_received += 1;
            debug!(seq, "Retransmitted packet received");
        }

        // First packet: initialize delivery state.
        if self.next_deliver_seq.is_none() {
            self.next_deliver_seq = Some(seq);
        }

        // Insert into reorder buffer.
        let frame = ArqAudioFrame {
            sequence: seq,
            timestamp: rtp.timestamp,
            payload: rtp.payload,
        };

        let next = self.next_deliver_seq.unwrap();
        let diff = seq.wrapping_sub(next);

        if diff == 0 {
            // This is the next expected packet — insert and try to deliver.
            self.reorder_buffer.insert(seq, frame);
        } else if diff < 0x8000 {
            // Ahead of delivery point — insert and detect gaps.
            self.reorder_buffer.insert(seq, frame);
            self.detect_gaps(next, seq);
        } else {
            // Behind delivery point — late/duplicate, discard.
            debug!(seq, next, "Dropping late ARQ packet");
        }

        // Evict oversized reorder buffer.
        while self.reorder_buffer.len() > REORDER_BUFFER_CAPACITY {
            if let Some((&oldest, _)) = self.reorder_buffer.iter().next() {
                self.reorder_buffer.remove(&oldest);
            }
        }

        Ok(())
    }

    /// Handle an incoming FEC parity packet.
    async fn handle_fec_packet(&mut self, data: &[u8]) -> Result<()> {
        let fec = FecPacket::decode(data)?;
        let base = fec.base_seq;

        // Cache the FEC packet.
        self.fec_cache.insert(base, fec.clone());

        // Try to recover any missing packet in this group.
        self.try_fec_recovery(&fec).await?;

        // Evict old FEC packets from cache.
        while self.fec_cache.len() > 8 {
            if let Some((&oldest, _)) = self.fec_cache.iter().next() {
                self.fec_cache.remove(&oldest);
            }
        }

        Ok(())
    }

    /// Track a received audio payload for FEC group recovery.
    fn track_fec_payload(&mut self, seq: u16, payload: &[u8]) {
        // Determine which FEC group this packet belongs to.
        // FEC groups are aligned to multiples of FEC_GROUP_SIZE from seq 0.
        let group_base = seq - (seq % FEC_GROUP_SIZE);

        // If we've moved to a new group, clear old data.
        if self.current_fec_base != Some(group_base) {
            // Keep the old group data for a bit (FEC packet might arrive late).
            self.current_fec_base = Some(group_base);
            // Only keep payloads from the current group window.
            let window_start = group_base.saturating_sub(FEC_GROUP_SIZE * 2);
            self.fec_received_payloads.retain(|&s, _| s >= window_start);
        }

        self.fec_received_payloads.insert(seq, payload.to_vec());
    }

    /// Try to recover a missing packet using FEC.
    async fn try_fec_recovery(&mut self, fec: &FecPacket) -> Result<()> {
        if self.ssrc != 0 && fec.ssrc != self.ssrc {
            debug!(
                ssrc = fec.ssrc,
                expected_ssrc = self.ssrc,
                "Ignoring FEC packet for different SSRC"
            );
            return Ok(());
        }

        // Find which sequence numbers in this group are missing from payloads
        // we have actually received. Gaps are eligible for FEC recovery.
        let mut missing_seqs = Vec::new();
        for i in 0..fec.count {
            let seq = fec.base_seq.wrapping_add(i);
            if !self.fec_received_payloads.contains_key(&seq) {
                missing_seqs.push(seq);
            }
        }

        // FEC can only recover one missing packet per group.
        if missing_seqs.len() != 1 {
            return Ok(());
        }

        let missing_seq = missing_seqs[0];

        // XOR all received payloads in the group with the FEC parity data.
        let mut recovered = fec.xor_payload.clone();
        let mut found_count = 0u16;
        for i in 0..fec.count {
            let seq = fec.base_seq.wrapping_add(i);
            if seq == missing_seq {
                continue;
            }
            if let Some(payload) = self.fec_received_payloads.get(&seq) {
                for (j, &byte) in payload.iter().enumerate() {
                    if j < recovered.len() {
                        recovered[j] ^= byte;
                    }
                }
                found_count += 1;
            }
        }

        // We need all other packets in the group to recover the missing one.
        if found_count != fec.count - 1 {
            debug!(
                missing_seq,
                found_count,
                expected = fec.count - 1,
                "FEC recovery not possible — not enough packets in group"
            );
            return Ok(());
        }

        // Recovered! We need to figure out the RTP timestamp. Since we don't
        // have it directly, estimate from neighboring packets.
        let estimated_timestamp = self.estimate_timestamp(missing_seq);

        let frame = ArqAudioFrame {
            sequence: missing_seq,
            timestamp: estimated_timestamp,
            payload: recovered.clone(),
        };

        self.reorder_buffer.insert(missing_seq, frame);
        self.fec_received_payloads.insert(missing_seq, recovered);
        self.fec_recovered += 1;

        // Remove from gap tracking.
        self.gaps.remove(&missing_seq);

        debug!(missing_seq, "Packet recovered via FEC");
        Ok(())
    }

    /// Estimate the RTP timestamp for a missing packet based on neighbors.
    fn estimate_timestamp(&self, seq: u16) -> u32 {
        // Try to find the timestamps of the previous and next packets.
        let prev_ts = self
            .reorder_buffer
            .range(..seq)
            .next_back()
            .map(|(_, f)| f.timestamp);
        let next_ts = self
            .reorder_buffer
            .range(seq..)
            .next()
            .map(|(_, f)| f.timestamp);

        match (prev_ts, next_ts) {
            (Some(prev), Some(next)) => {
                // Interpolate: assume uniform spacing.
                let prev_seq = self
                    .reorder_buffer
                    .range(..seq)
                    .next_back()
                    .map(|(&s, _)| s)
                    .unwrap_or(seq);
                let next_seq = self
                    .reorder_buffer
                    .range(seq..)
                    .next()
                    .map(|(&s, _)| s)
                    .unwrap_or(seq);
                let span = next_seq.wrapping_sub(prev_seq) as u32;
                let offset = seq.wrapping_sub(prev_seq) as u32;
                let adjustment = next
                    .wrapping_sub(prev)
                    .wrapping_mul(offset)
                    .checked_div(span)
                    .unwrap_or(0);
                prev.wrapping_add(adjustment)
            }
            (Some(prev), None) => prev.wrapping_add(900), // ~10ms at 90kHz
            (None, Some(next)) => next.wrapping_sub(900),
            (None, None) => 0,
        }
    }

    /// Detect sequence gaps between `from` (inclusive) and `to` (exclusive).
    fn detect_gaps(&mut self, from: u16, to: u16) {
        let mut seq = from;
        while seq != to {
            if !self.reorder_buffer.contains_key(&seq) && !self.gaps.contains_key(&seq) {
                self.gaps.insert(
                    seq,
                    GapState {
                        first_seen: std::time::Instant::now(),
                        nack_count: 0,
                    },
                );
                self.gaps_detected += 1;
            }
            seq = seq.wrapping_add(1);
        }
    }

    /// Check gap timers and send NACKs for overdue gaps.
    async fn process_nack_timer(&mut self) -> Result<()> {
        let now = std::time::Instant::now();
        let mut nack_seqs = Vec::new();
        let mut abandoned = BTreeSet::new();

        self.gaps.retain(|&seq, state| {
            if now.duration_since(state.first_seen) >= NACK_DELAY {
                if state.nack_count < MAX_NACK_RETRIES {
                    nack_seqs.push(seq);
                    state.nack_count += 1;
                    state.first_seen = now;
                    true
                } else {
                    // Max retries exceeded — give up on this gap.
                    debug!(seq, "Gap exceeded max NACK retries — giving up");
                    abandoned.insert(seq);
                    false
                }
            } else {
                true
            }
        });

        self.advance_over_abandoned_gaps(&abandoned);

        if !nack_seqs.is_empty() {
            self.send_nack(&nack_seqs).await?;
        }

        Ok(())
    }

    /// Advance delivery past gaps that exhausted ARQ recovery.
    ///
    /// The downstream UDP media path already performs packet-loss concealment
    /// when it sees a sequence jump, so ARQ should eventually unblock and let
    /// the next available packet through instead of stalling the stream.
    fn advance_over_abandoned_gaps(&mut self, abandoned: &BTreeSet<u16>) {
        while let Some(next) = self.next_deliver_seq {
            if abandoned.contains(&next) {
                self.next_deliver_seq = Some(next.wrapping_add(1));
            } else {
                break;
            }
        }
    }

    /// Send a NACK packet to the server for the given missing sequence numbers.
    async fn send_nack(&mut self, missing_seqs: &[u16]) -> Result<()> {
        if missing_seqs.is_empty() {
            return Ok(());
        }

        // Group consecutive sequences into ranges for efficiency.
        let mut ranges = Vec::new();
        let mut start = missing_seqs[0];
        let mut count = 1u16;

        for &seq in &missing_seqs[1..] {
            if seq == start.wrapping_add(count) {
                count += 1;
            } else {
                ranges.push((start, count));
                start = seq;
                count = 1;
            }
        }
        ranges.push((start, count));

        let nack = NackPacket {
            ssrc: self.ssrc,
            ranges,
        };

        let encoded = nack.encode();
        // NACKs are small and time-sensitive — don't block long.
        let _ = timeout(
            Duration::from_millis(50),
            self.socket.send_to(&encoded, self.server_addr),
        )
        .await;

        self.nacks_sent += 1;
        debug!(
            missing = missing_seqs.len(),
            "NACK sent for missing packets"
        );
        Ok(())
    }

    /// Try to deliver the next in-order packet from the reorder buffer.
    fn try_deliver(&mut self) -> Option<ArqAudioFrame> {
        let next = self.next_deliver_seq?;
        if let Some(frame) = self.reorder_buffer.remove(&next) {
            self.next_deliver_seq = Some(next.wrapping_add(1));
            // Remove any gap tracking for this sequence.
            self.gaps.remove(&next);
            Some(frame)
        } else {
            None
        }
    }

    /// Metrics: total audio packets received (including retransmissions).
    pub fn packets_received(&self) -> u64 {
        self.packets_received
    }

    /// Metrics: total sequence gaps detected.
    pub fn gaps_detected(&self) -> u64 {
        self.gaps_detected
    }

    /// Metrics: total NACK packets sent.
    pub fn nacks_sent(&self) -> u64 {
        self.nacks_sent
    }

    /// Metrics: packets recovered via FEC (no retransmission needed).
    pub fn fec_recovered(&self) -> u64 {
        self.fec_recovered
    }

    /// Metrics: retransmitted packets received.
    pub fn retransmit_received(&self) -> u64 {
        self.retransmit_received
    }

    /// Current number of gaps being tracked (waiting for retransmission).
    pub fn pending_gaps(&self) -> usize {
        self.gaps.len()
    }

    /// Current number of packets in the reorder buffer.
    pub fn reorder_buffer_len(&self) -> usize {
        self.reorder_buffer.len()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn arq_receiver_starts_with_zero_metrics() {
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let server: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let receiver = ArqReceiver::new(sock, server, 0xBEEF);
        assert_eq!(receiver.packets_received(), 0);
        assert_eq!(receiver.gaps_detected(), 0);
        assert_eq!(receiver.nacks_sent(), 0);
        assert_eq!(receiver.fec_recovered(), 0);
        assert_eq!(receiver.retransmit_received(), 0);
        assert_eq!(receiver.pending_gaps(), 0);
        assert_eq!(receiver.reorder_buffer_len(), 0);
    }

    #[test]
    fn gap_detection_simple() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let sock = rt.block_on(async { Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap()) });
        let server: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let mut receiver = ArqReceiver::new(sock, server, 0);

        // Simulate: we have packet 0, then receive packet 3 (gap at 1, 2).
        receiver.next_deliver_seq = Some(0);
        receiver.reorder_buffer.insert(
            0,
            ArqAudioFrame {
                sequence: 0,
                timestamp: 0,
                payload: vec![],
            },
        );

        // Detect gaps between 1 and 3.
        receiver.detect_gaps(1, 3);
        assert_eq!(receiver.gaps_detected(), 2);
        assert!(receiver.gaps.contains_key(&1));
        assert!(receiver.gaps.contains_key(&2));
    }

    #[test]
    fn try_deliver_returns_none_when_buffer_empty() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let sock = rt.block_on(async { Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap()) });
        let server: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let mut receiver = ArqReceiver::new(sock, server, 0);
        receiver.next_deliver_seq = Some(0);
        assert!(receiver.try_deliver().is_none());
    }

    #[test]
    fn try_deliver_returns_packet_when_available() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let sock = rt.block_on(async { Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap()) });
        let server: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let mut receiver = ArqReceiver::new(sock, server, 0);
        receiver.next_deliver_seq = Some(5);
        receiver.reorder_buffer.insert(
            5,
            ArqAudioFrame {
                sequence: 5,
                timestamp: 45000,
                payload: vec![1, 2, 3],
            },
        );
        let frame = receiver.try_deliver().unwrap();
        assert_eq!(frame.sequence, 5);
        assert_eq!(receiver.next_deliver_seq, Some(6));
    }

    #[test]
    fn estimate_timestamp_interpolates() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let sock = rt.block_on(async { Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap()) });
        let server: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let mut receiver = ArqReceiver::new(sock, server, 0);

        receiver.reorder_buffer.insert(
            10,
            ArqAudioFrame {
                sequence: 10,
                timestamp: 90000, // 1 second
                payload: vec![],
            },
        );
        receiver.reorder_buffer.insert(
            14,
            ArqAudioFrame {
                sequence: 14,
                timestamp: 93600, // 1.04 seconds
                payload: vec![],
            },
        );

        // Packet 12 is halfway between 10 and 14.
        let ts = receiver.estimate_timestamp(12);
        assert_eq!(ts, 91800); // 90000 + (3600 * 2 / 4) = 91800
    }

    #[tokio::test]
    async fn audio_packet_learns_ssrc_from_first_packet() {
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let server: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let mut receiver = ArqReceiver::new(sock, server, 0);
        let packet = RtpPacket {
            sequence: 0,
            timestamp: 0,
            ssrc: 0xCAFE_BABE,
            payload: vec![1, 2, 3],
        };

        receiver
            .handle_audio_packet(&packet.encode())
            .await
            .unwrap();

        assert_eq!(receiver.ssrc, 0xCAFE_BABE);
    }

    #[tokio::test]
    async fn fec_recovery_recovers_tracked_gap() {
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let server: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let mut receiver = ArqReceiver::new(sock, server, 0xBEEF);

        let payloads = [
            vec![1, 2, 3],
            vec![4, 5, 6],
            vec![7, 8, 9],
            vec![10, 11, 12],
            vec![13, 14, 15],
        ];
        let mut xor = vec![0u8; 3];
        for payload in &payloads {
            for (idx, byte) in payload.iter().enumerate() {
                xor[idx] ^= byte;
            }
        }

        for seq in [0u16, 2, 3, 4] {
            receiver
                .fec_received_payloads
                .insert(seq, payloads[seq as usize].clone());
        }
        receiver.gaps.insert(
            1,
            GapState {
                first_seen: std::time::Instant::now(),
                nack_count: 1,
            },
        );

        let fec = FecPacket {
            base_seq: 0,
            count: 5,
            ssrc: 0xBEEF,
            xor_payload: xor,
        };

        receiver.try_fec_recovery(&fec).await.unwrap();

        let recovered = receiver.reorder_buffer.get(&1).unwrap();
        assert_eq!(recovered.payload, payloads[1]);
        assert!(!receiver.gaps.contains_key(&1));
        assert_eq!(receiver.fec_recovered(), 1);
    }

    #[tokio::test]
    async fn abandoned_gap_advances_delivery_to_next_buffered_packet() {
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let server: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let mut receiver = ArqReceiver::new(sock, server, 0xBEEF);

        receiver.next_deliver_seq = Some(7);
        receiver.gaps.insert(
            7,
            GapState {
                first_seen: std::time::Instant::now() - Duration::from_millis(10),
                nack_count: MAX_NACK_RETRIES,
            },
        );
        receiver.reorder_buffer.insert(
            8,
            ArqAudioFrame {
                sequence: 8,
                timestamp: 72_000,
                payload: vec![1, 2, 3],
            },
        );

        receiver.process_nack_timer().await.unwrap();

        assert_eq!(receiver.next_deliver_seq, Some(8));
        assert_eq!(receiver.try_deliver().unwrap().sequence, 8);
    }
}
