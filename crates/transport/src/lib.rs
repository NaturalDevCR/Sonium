//! Transport abstraction layer for Sonium media delivery.
//!
//! # Design
//!
//! Sonium separates the **control plane** (session setup, codec negotiation,
//! volume, health reports — always TCP) from the **media plane** (audio frame
//! delivery — pluggable via this crate).
//!
//! The core interface is [`MediaSender`]: a per-session handle that accepts a
//! pre-encoded audio frame and delivers it to the connected client.
//!
//! # Phase status
//!
//! | Phase | Transport            | Status          |
//! |-------|----------------------|-----------------|
//! | 1     | `tcp`                | Implemented     |
//! | 2     | `rtp_udp`            | Implemented, pending live validation |
//! | 5     | `quic_dgram`         | Stub — Phase 5  |
//!
//! # Wire-format note
//!
//! The bytes passed to [`MediaSender::send_wire_bytes`] are already fully
//! framed as a Sonium `Message::WireChunk` (26-byte header + payload). TCP
//! sends those bytes unchanged. RTP/UDP strips the Sonium header and places the
//! raw WireChunk payload inside an RTP packet before delivery.

pub mod rtp;
pub mod sender;

pub use rtp::{rtp_timestamp, RtpPacket, RTP_CLOCK_RATE, RTP_HEADER_SIZE, SONIUM_RTP_PAYLOAD_TYPE};
pub use sender::{MediaSender, QuicDgramMediaSender, RtpUdpMediaSender, TcpMediaSender};

use serde::{Deserialize, Serialize};
use std::fmt;

/// The protocol used to deliver media frames from server to client.
///
/// `Tcp` is the stable default. `RtpUdp` is implemented for Phase 2 validation.
/// `QuicDgram` remains config-visible but not implemented yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransportMode {
    /// Reliable ordered TCP stream. Control plane and media plane share the
    /// same connection. Maximum compatibility; subject to head-of-line
    /// blocking under packet loss.
    #[default]
    Tcp,
    /// RTP/UDP unicast. Media plane decoupled from control plane.
    /// No head-of-line blocking; requires explicit jitter-buffer handling.
    /// Implemented for Phase 2 validation.
    RtpUdp,
    /// QUIC DATAGRAM. Encrypted datagram delivery for WAN/routed networks.
    /// **Not yet implemented — Phase 5.**
    QuicDgram,
}

impl fmt::Display for TransportMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp => f.write_str("tcp"),
            Self::RtpUdp => f.write_str("rtp_udp"),
            Self::QuicDgram => f.write_str("quic_dgram"),
        }
    }
}

/// Transport configuration block.
///
/// Add to `sonium.toml` under `[server.transport]`:
///
/// ```toml
/// [server.transport]
/// mode     = "tcp"   # "tcp" (default) | "rtp_udp" | "quic_dgram"
/// udp_port = 1711    # server UDP port for RTP media (0 = disabled)
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TransportConfig {
    pub mode: TransportMode,
    /// UDP port the server listens on for RTP/UDP media delivery.
    /// `0` disables the UDP media path even when `mode = "rtp_udp"`.
    pub udp_port: u16,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            mode: TransportMode::Tcp,
            udp_port: 0,
        }
    }
}
