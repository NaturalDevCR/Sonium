//! Per-session media sender interface and implementations.
//!
//! The [`MediaSender`] trait is the single point through which a server session
//! pushes encoded audio frames to a connected client.  Each transport mode has
//! its own implementation; TCP is the stable default and RTP/UDP is available
//! for Phase 2 validation.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::UdpSocket;
use tokio::time::timeout;

use crate::TransportMode;

/// Boxed async future used as the return type of [`MediaSender::send_wire_bytes`].
///
/// BoxFuture allows dynamic dispatch (`dyn MediaSender`) in future phases
/// without requiring the `async_trait` proc-macro.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Per-session server-side media delivery interface.
///
/// Implementations are responsible for framing and delivering a pre-encoded
/// audio frame to a single connected client over the selected transport.
///
/// # Phase 1 usage
///
/// Only [`TcpMediaSender`] is implemented.  The session creates one inline for
/// each audio-frame write and drops it immediately after — the underlying TCP
/// writer borrow is released before any subsequent control-plane write.
///
/// ```text
/// let mut sender = TcpMediaSender::new(stream);
/// sender.send_wire_bytes(&frame.wire_bytes).await?;
/// // `stream` borrow released; control writes continue normally.
/// ```
///
/// # Datagram transports
///
/// [`RtpUdpMediaSender`] re-frames the WireChunk payload as an RTP packet and
/// sends it over a per-client UDP endpoint. The call site in the session loop
/// remains unchanged.
pub trait MediaSender: Send {
    /// The transport protocol this sender uses.
    fn transport_mode(&self) -> TransportMode;

    /// Deliver a pre-encoded audio frame to the client.
    ///
    /// `bytes` is a fully framed `Message::WireChunk`: 26-byte Sonium header
    /// plus payload. TCP sends it unchanged; datagram transports may decompose
    /// and re-frame the payload behind this interface.
    fn send_wire_bytes<'a>(&'a mut self, bytes: &'a [u8]) -> BoxFuture<'a, Result<()>>;
}

// ── TCP ───────────────────────────────────────────────────────────────────────

const TCP_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// TCP media sender — writes pre-encoded `WireChunk` bytes directly to the
/// client's write-half of the TCP stream.
///
/// Borrows the writer for the lifetime of the sender; the borrow is short-lived
/// (one audio frame) so the session can use the same writer for control-plane
/// messages between audio sends.
pub struct TcpMediaSender<'w> {
    writer: &'w mut OwnedWriteHalf,
}

impl<'w> TcpMediaSender<'w> {
    pub fn new(writer: &'w mut OwnedWriteHalf) -> Self {
        Self { writer }
    }
}

impl MediaSender for TcpMediaSender<'_> {
    fn transport_mode(&self) -> TransportMode {
        TransportMode::Tcp
    }

    fn send_wire_bytes<'a>(&'a mut self, bytes: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            match timeout(TCP_WRITE_TIMEOUT, self.writer.write_all(bytes)).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e.into()),
                Err(_) => Err(anyhow::anyhow!("tcp audio write timed out")),
            }
        })
    }
}

// ── Phase 2: RTP/UDP ──────────────────────────────────────────────────────────

const UDP_SEND_TIMEOUT: Duration = Duration::from_secs(2);

/// RTP/UDP unicast media sender.
///
/// Wraps a shared server-side `UdpSocket` and a per-client destination
/// address.  Each call to [`send_wire_bytes`] extracts the WireChunk
/// timestamp from the Sonium wire bytes, builds a 12-byte RTP header, and
/// fires the packet to the client's UDP endpoint.
///
/// The payload of every RTP packet is `wire_bytes[26..]` — the raw WireChunk
/// payload bytes — so the client can call `WireChunk::decode` directly without
/// any extra framing.
pub struct RtpUdpMediaSender {
    socket: Arc<UdpSocket>,
    peer_addr: SocketAddr,
    ssrc: u32,
    sequence: u16,
}

impl RtpUdpMediaSender {
    /// Create a new sender for one client session.
    ///
    /// `socket` is the server's shared UDP socket (bound once at startup).
    /// `peer_addr` is the client's UDP address (`peer_tcp_ip:hello.udp_port`).
    /// `ssrc` should be randomly generated per session.
    pub fn new(socket: Arc<UdpSocket>, peer_addr: SocketAddr, ssrc: u32) -> Self {
        Self {
            socket,
            peer_addr,
            ssrc,
            sequence: 0,
        }
    }
}

impl MediaSender for RtpUdpMediaSender {
    fn transport_mode(&self) -> TransportMode {
        TransportMode::RtpUdp
    }

    fn send_wire_bytes<'a>(&'a mut self, bytes: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let packet = crate::rtp::rtp_from_wire_bytes(bytes, self.sequence, self.ssrc)?;
            self.sequence = self.sequence.wrapping_add(1);
            let encoded = packet.encode();
            match timeout(
                UDP_SEND_TIMEOUT,
                self.socket.send_to(&encoded, self.peer_addr),
            )
            .await
            {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(e)) => Err(e.into()),
                Err(_) => Err(anyhow::anyhow!("UDP audio send timed out")),
            }
        })
    }
}

// ── Phase 5 stub: QUIC DATAGRAM ───────────────────────────────────────────────

/// Placeholder for the QUIC DATAGRAM media sender (Phase 5).
///
/// Will implement encrypted datagram delivery using the same [`MediaSender`]
/// interface as [`RtpUdpMediaSender`], sharing the same jitter-buffer and
/// playout logic on the client side.
pub struct QuicDgramMediaSender;

impl MediaSender for QuicDgramMediaSender {
    fn transport_mode(&self) -> TransportMode {
        TransportMode::QuicDgram
    }

    fn send_wire_bytes<'a>(&'a mut self, _bytes: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            unimplemented!("QUIC DATAGRAM media sender is not yet implemented (Phase 5)")
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quic_stub_reports_correct_transport_mode() {
        assert_eq!(
            QuicDgramMediaSender.transport_mode(),
            TransportMode::QuicDgram
        );
    }

    #[tokio::test]
    async fn rtp_sender_reports_correct_transport_mode() {
        let sock = std::sync::Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let sender = RtpUdpMediaSender::new(sock, peer, 0xDEAD_BEEF);
        assert_eq!(sender.transport_mode(), TransportMode::RtpUdp);
    }
}
