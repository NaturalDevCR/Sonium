//! Central NACK router for ARQ transport.
//!
//! A single Tokio task reads from the shared UDP socket and dispatches
//! incoming NACK packets to the per-session handler via an SSRC-keyed
//! channel map.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, warn};

use sonium_sync::time_provider::now_us;
use sonium_transport::arq::{
    detect_packet_type, encode_time_echo, ArqPacketType, NackPacket, ARQ_MAGIC_TIME_PROBE,
    TIME_PROBE_SIZE,
};

/// Sender half of a per-session NACK channel.
pub type NackSender = mpsc::UnboundedSender<NackPacket>;

/// Receiver half of a per-session NACK channel.
pub type NackReceiver = mpsc::UnboundedReceiver<NackPacket>;

struct NackRoute {
    peer: SocketAddr,
    tx: NackSender,
}

/// Shared state for the NACK router.
///
/// Sessions register their SSRC + channel on startup and unregister on exit.
/// The central recv task looks up the SSRC of each incoming NACK and forwards
/// it to the correct session.
#[derive(Clone)]
pub struct NackRouter {
    channels: Arc<RwLock<BTreeMap<u32, NackRoute>>>,
}

impl NackRouter {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Register a session for NACK delivery.  Returns the receiver end.
    pub async fn register(&self, ssrc: u32, peer: SocketAddr) -> NackReceiver {
        let (tx, rx) = mpsc::unbounded_channel();
        self.channels
            .write()
            .await
            .insert(ssrc, NackRoute { peer, tx });
        rx
    }

    /// Unregister a session (call on session exit).
    pub async fn unregister(&self, ssrc: u32) {
        self.channels.write().await.remove(&ssrc);
    }

    /// Spawn the central NACK recv loop.
    ///
    /// Reads from the shared UDP socket, classifies packets, and routes
    /// NACKs to the appropriate session.
    pub fn spawn_recv_loop(self, socket: Arc<UdpSocket>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut buf = vec![0u8; 65_535];
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((n, peer)) => {
                        let data = &buf[..n];

                        // Time-probe: echo back immediately with server receive timestamp.
                        // Works for both RTP and ARQ sessions — no SSRC routing needed.
                        if data.len() >= TIME_PROBE_SIZE && data[0] == ARQ_MAGIC_TIME_PROBE {
                            let t_server_recv_us = now_us();
                            let seq = u16::from_le_bytes([data[1], data[2]]);
                            let t_sent_us =
                                i64::from_le_bytes(data[3..11].try_into().unwrap_or([0u8; 8]));
                            let echo = encode_time_echo(seq, t_sent_us, t_server_recv_us);
                            if let Err(e) = socket.send_to(&echo, peer).await {
                                debug!(error = %e, %peer, "Failed to send time echo");
                            } else {
                                debug!(seq, %peer, "Time echo sent");
                            }
                            continue;
                        }

                        match detect_packet_type(data) {
                            Some(ArqPacketType::Nack) => match NackPacket::decode(data) {
                                Ok(nack) => {
                                    let nack_ssrc = nack.ssrc;
                                    let missing = nack.total_missing();
                                    let channels = self.channels.read().await;
                                    if let Some(route) = channels.get(&nack_ssrc) {
                                        if route.peer != peer {
                                            debug!(
                                                ssrc = nack_ssrc,
                                                expected = %route.peer,
                                                got = %peer,
                                                "NACK source does not match registered client — ignoring"
                                            );
                                        } else if route.tx.send(nack).is_err() {
                                            debug!(
                                                ssrc = nack_ssrc,
                                                "NACK channel closed — session gone"
                                            );
                                        } else {
                                            debug!(
                                                ssrc = nack_ssrc,
                                                missing, "NACK routed to session"
                                            );
                                        }
                                    } else {
                                        debug!(
                                            ssrc = nack_ssrc,
                                            "NACK for unknown SSRC — ignoring"
                                        );
                                    }
                                }
                                Err(e) => {
                                    debug!(error = %e, "Failed to decode NACK packet");
                                }
                            },
                            Some(ArqPacketType::Audio) => {
                                // Server doesn't expect audio from clients — ignore.
                            }
                            Some(ArqPacketType::Fec) => {
                                // Server doesn't expect FEC from clients — ignore.
                            }
                            Some(ArqPacketType::TimeProbe) | Some(ArqPacketType::TimeEcho) => {
                                // Handled above before detect_packet_type — unreachable.
                            }
                            None => {
                                debug!(bytes = n, "Unknown UDP packet type — ignoring");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "UDP recv error in NACK router");
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                }
            }
        })
    }
}

impl Default for NackRouter {
    fn default() -> Self {
        Self::new()
    }
}
