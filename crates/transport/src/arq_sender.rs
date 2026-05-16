//! Server-side ARQ sender: retransmission buffer, NACK handler, FEC generator.
//!
//! [`ArqSender`] wraps a UDP socket and per-client destination, providing:
//!
//! - **Audio packet send** with automatic RTP framing and buffering for
//!   potential retransmission.
//! - **NACK processing** — when the client detects sequence gaps, it sends
//!   NACK packets back.  The sender retransmits the requested packets from
//!   its sliding window buffer.
//! - **FEC generation** — every [`FEC_GROUP_SIZE`] audio packets, an XOR
//!   parity packet is sent, enabling single-packet recovery without
//!   retransmission latency.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing::debug;

use crate::arq::{FecPacket, NackPacket, FEC_GROUP_SIZE};
use crate::rtp::rtp_from_wire_bytes;
use crate::sender::MediaSender;
use crate::TransportMode;

const UDP_SEND_TIMEOUT: Duration = Duration::from_secs(2);

/// Maximum number of packets kept in the retransmission buffer.
///
/// At 20 ms per chunk this covers ~2 seconds of audio.
const RETX_BUFFER_CAPACITY: usize = 100;

/// Maximum number of times a packet will be retransmitted.
const MAX_RETRANSMISSIONS: u32 = 3;

/// Sliding window buffer entry for retransmission.
#[derive(Debug, Clone)]
struct BufferedPacket {
    /// The fully encoded RTP packet bytes (ready to send).
    rtp_bytes: Vec<u8>,
    /// How many times this packet has been retransmitted.
    retransmit_count: u32,
}

/// Server-side ARQ media sender.
///
/// Each client session creates one `ArqSender`.  It manages a sliding window
/// of recently sent audio packets for retransmission, generates FEC parity
/// packets, and handles incoming NACK requests from the client.
pub struct ArqSender {
    socket: Arc<UdpSocket>,
    peer_addr: SocketAddr,
    ssrc: u32,
    /// Next audio sequence number to assign.
    sequence: u16,
    /// Retransmission buffer: ordered by sequence number (oldest first).
    buffer: VecDeque<(u16, BufferedPacket)>,
    /// FEC accumulator: stores RTP payloads for the current FEC group.
    fec_payloads: Vec<Vec<u8>>,
    /// Sequence number of the first packet in the current FEC group.
    fec_base_seq: u16,
    /// Counter within the current FEC group (0..FEC_GROUP_SIZE).
    fec_group_index: u16,
    /// Total retransmissions sent (for metrics).
    retransmit_count: u64,
    /// Total FEC packets sent (for metrics).
    fec_sent_count: u64,
}

impl ArqSender {
    /// Create a new ARQ sender for one client session.
    pub fn new(socket: Arc<UdpSocket>, peer_addr: SocketAddr, ssrc: u32) -> Self {
        Self {
            socket,
            peer_addr,
            ssrc,
            sequence: 0,
            buffer: VecDeque::with_capacity(RETX_BUFFER_CAPACITY),
            fec_payloads: Vec::with_capacity(FEC_GROUP_SIZE as usize),
            fec_base_seq: 0,
            fec_group_index: 0,
            retransmit_count: 0,
            fec_sent_count: 0,
        }
    }

    /// Send an audio frame and buffer it for potential retransmission.
    ///
    /// `wire_bytes` is a fully framed Sonium `Message::WireChunk` (26-byte
    /// header + payload).  The method converts it to an RTP packet, sends it,
    /// and stores it in the retransmission buffer.
    pub async fn send_audio(&mut self, wire_bytes: &[u8]) -> Result<()> {
        let seq = self.sequence;
        let rtp = rtp_from_wire_bytes(wire_bytes, seq, self.ssrc)?;
        let rtp_bytes = rtp.encode();

        // Accumulate for FEC before sending.
        self.accumulate_for_fec(&rtp.payload);

        // Send the audio packet.
        self.send_udp(&rtp_bytes).await?;

        // Buffer for retransmission.
        self.buffer.push_back((
            seq,
            BufferedPacket {
                rtp_bytes,
                retransmit_count: 0,
            },
        ));
        // Evict oldest if buffer is full.
        if self.buffer.len() > RETX_BUFFER_CAPACITY {
            self.buffer.pop_front();
        }

        self.sequence = self.sequence.wrapping_add(1);

        // Check if we should send a FEC packet.
        self.fec_group_index += 1;
        if self.fec_group_index >= FEC_GROUP_SIZE {
            self.emit_fec_packet().await?;
        }

        Ok(())
    }

    /// Process an incoming NACK from the client and retransmit requested packets.
    pub async fn handle_nack(&mut self, nack: &NackPacket) -> Result<()> {
        for &(start, count) in &nack.ranges {
            for i in 0..count {
                let seq = start.wrapping_add(i);
                // Find the packet and clone its bytes to release the buffer borrow
                // before calling send_udp (which borrows self).
                let packet_to_retx = self
                    .buffer
                    .iter_mut()
                    .find(|(s, _)| *s == seq)
                    .filter(|(_, p)| p.retransmit_count < MAX_RETRANSMISSIONS)
                    .map(|(_, p)| {
                        p.retransmit_count += 1;
                        p.rtp_bytes.clone()
                    });

                if let Some(rtp_bytes) = packet_to_retx {
                    self.send_udp(&rtp_bytes).await?;
                    self.retransmit_count += 1;
                    debug!(seq, "Retransmitted audio packet");
                } else if self.buffer.iter().any(|(s, _)| *s == seq) {
                    debug!(seq, "Packet exceeded max retransmissions — skipping");
                } else {
                    debug!(seq, "NACK for packet outside retransmission window");
                }
            }
        }
        Ok(())
    }

    /// Accumulate an audio payload into the current FEC group.
    fn accumulate_for_fec(&mut self, payload: &[u8]) {
        if self.fec_group_index == 0 {
            self.fec_base_seq = self.sequence;
        }
        self.fec_payloads.push(payload.to_vec());
    }

    /// Generate and send a FEC parity packet for the current group.
    async fn emit_fec_packet(&mut self) -> Result<()> {
        if self.fec_payloads.is_empty() {
            return Ok(());
        }

        // Find the maximum payload size in the group.
        let max_len = self.fec_payloads.iter().map(|p| p.len()).max().unwrap_or(0);
        if max_len == 0 {
            self.reset_fec_group();
            return Ok(());
        }

        // XOR all payloads (padded to max_len with zeros).
        let mut xor = vec![0u8; max_len];
        for payload in &self.fec_payloads {
            for (i, &byte) in payload.iter().enumerate() {
                xor[i] ^= byte;
            }
        }

        let fec = FecPacket {
            base_seq: self.fec_base_seq,
            count: self.fec_payloads.len() as u16,
            ssrc: self.ssrc,
            xor_payload: xor,
        };

        self.send_udp(&fec.encode()).await?;
        self.fec_sent_count += 1;
        debug!(
            base_seq = self.fec_base_seq,
            count = self.fec_payloads.len(),
            "FEC packet sent"
        );

        self.reset_fec_group();
        Ok(())
    }

    /// Reset the FEC group accumulator for the next group.
    fn reset_fec_group(&mut self) {
        self.fec_payloads.clear();
        self.fec_group_index = 0;
    }

    /// Send raw bytes to the client's UDP endpoint.
    async fn send_udp(&self, data: &[u8]) -> Result<()> {
        match timeout(UDP_SEND_TIMEOUT, self.socket.send_to(data, self.peer_addr)).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err(anyhow::anyhow!("ARQ UDP send timed out")),
        }
    }

    /// Number of retransmissions performed so far.
    pub fn retransmit_count(&self) -> u64 {
        self.retransmit_count
    }

    /// Number of FEC packets sent so far.
    pub fn fec_sent_count(&self) -> u64 {
        self.fec_sent_count
    }

    /// Current sequence number (next packet will use this).
    pub fn next_sequence(&self) -> u16 {
        self.sequence
    }

    /// Number of packets currently in the retransmission buffer.
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }
}

impl MediaSender for ArqSender {
    fn transport_mode(&self) -> TransportMode {
        TransportMode::Rist
    }

    fn send_wire_bytes<'a>(
        &'a mut self,
        bytes: &'a [u8],
    ) -> crate::sender::BoxFuture<'a, Result<()>> {
        Box::pin(async move { self.send_audio(bytes).await })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    #[tokio::test]
    async fn arq_sender_reports_correct_transport_mode() {
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let peer: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let sender = ArqSender::new(sock, peer, 0xBEEF);
        assert_eq!(sender.transport_mode(), TransportMode::Rist);
    }

    #[tokio::test]
    async fn arq_sender_tracks_retransmit_and_fec_counts() {
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let peer: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let sender = ArqSender::new(sock, peer, 0xBEEF);
        assert_eq!(sender.retransmit_count(), 0);
        assert_eq!(sender.fec_sent_count(), 0);
        assert_eq!(sender.next_sequence(), 0);
        assert_eq!(sender.buffer_len(), 0);
    }
}
