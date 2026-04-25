//! Per-client session handler.
//!
//! Each connected client runs in its own Tokio task.  The session:
//!
//! 1. Reads the initial `Hello` message and registers the client with
//!    [`ServerState`].
//! 2. Sends the current `CodecHeader` (if a stream is active) and
//!    `ServerSettings`.
//! 3. Subscribes to the [`Broadcaster`] channel and forwards audio frames.
//! 4. Handles incoming `Time` and `ClientInfo` messages concurrently.
//! 5. Marks the client disconnected in [`ServerState`] on exit.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, warn};

use sonium_common::config::ServerConfig;
use sonium_control::ServerState;
use sonium_protocol::{
    MessageHeader, MessageType, Timestamp,
    messages::{Message, ServerSettings, TimeMsg},
    header::HEADER_SIZE,
};

use crate::broadcaster::Broadcaster;

/// Global message ID counter (server-side).
static MSG_SEQ: AtomicU16 = AtomicU16::new(1);

fn next_id() -> u16 {
    MSG_SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Handle a single connected client for its entire lifetime.
pub async fn handle(
    mut stream: TcpStream,
    peer:       SocketAddr,
    bc:         Arc<Broadcaster>,
    cfg:        ServerConfig,
    state:      Arc<ServerState>,
) -> anyhow::Result<()> {
    // 1. Read Hello
    let hello_msg = read_message(&mut stream).await?;
    let (client_id, hostname, client_name, os, arch, proto_ver) =
        if let Message::Hello(h) = &hello_msg {
            debug!(%peer, id = %h.id, client = %h.client_name, "Hello received");
            (h.id.clone(), h.hostname.clone(), h.client_name.clone(),
             h.os.clone(), h.arch.clone(), h.protocol_version)
        } else {
            return Err(anyhow::anyhow!("expected Hello, got {:?}", hello_msg.message_type()));
        };

    // 2. Register with ServerState
    state.client_connected(&client_id, &hostname, &client_name, &os, &arch, peer, proto_ver);

    let result = session_loop(&mut stream, peer, bc, cfg, state.clone(), &client_id).await;

    // 3. Always deregister on exit
    state.client_disconnected(&client_id);
    result
}

async fn session_loop(
    stream:    &mut TcpStream,
    peer:      SocketAddr,
    bc:        Arc<Broadcaster>,
    cfg:       ServerConfig,
    state:     Arc<ServerState>,
    client_id: &str,
) -> anyhow::Result<()> {
    // Send CodecHeader if stream is already active
    if let Some(codec_hdr) = bc.codec_header() {
        stream.write_all(&codec_hdr).await?;
    }

    // Send initial ServerSettings
    let settings = ServerSettings {
        buffer_ms: cfg.stream.buffer_ms as i32,
        latency:   0,
        volume:    100,
        muted:     false,
    };
    let mut ss_hdr = MessageHeader::new(MessageType::ServerSettings, 0);
    ss_hdr.id = next_id();
    stream.write_all(&Message::ServerSettings(settings).encode_with_header(ss_hdr)).await?;

    // Subscribe to audio broadcast
    let mut rx = bc.subscribe();
    let mut hdr_buf = [0u8; HEADER_SIZE];

    loop {
        tokio::select! {
            // Incoming message from client
            read_result = stream.read_exact(&mut hdr_buf) => {
                match read_result {
                    Err(_) => break,
                    Ok(_) => {
                        let hdr = MessageHeader::from_bytes(&hdr_buf)?;
                        let mut payload = vec![0u8; hdr.payload_size as usize];
                        stream.read_exact(&mut payload).await?;
                        handle_client_msg(stream, &state, client_id, hdr, &payload).await?;
                    }
                }
            }

            // Outgoing audio frame
            frame = rx.recv() => {
                match frame {
                    Ok(f) => {
                        if stream.write_all(&f.wire_bytes).await.is_err() { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(%peer, dropped = n, "Client lagged");
                    }
                    Err(_) => break,
                }
            }
        }
    }

    Ok(())
}

async fn handle_client_msg(
    stream:    &mut TcpStream,
    state:     &ServerState,
    client_id: &str,
    hdr:       MessageHeader,
    payload:   &[u8],
) -> anyhow::Result<()> {
    match hdr.msg_type {
        MessageType::Time => {
            // Echo back with measured c2s latency
            let now  = Timestamp::now();
            let diff_sec  = now.sec  - hdr.sent.sec;
            let diff_usec = now.usec - hdr.sent.usec;
            let time_msg  = TimeMsg { latency: Timestamp { sec: diff_sec, usec: diff_usec } };

            let mut reply_hdr = MessageHeader::new(MessageType::Time, 8);
            reply_hdr.id        = next_id();
            reply_hdr.refers_to = hdr.id;
            reply_hdr.received  = now;

            stream.write_all(&Message::Time(time_msg).encode_with_header(reply_hdr)).await?;
        }
        MessageType::ClientInfo => {
            if let Ok(Message::ClientInfo(ci)) = Message::from_payload(&hdr, payload) {
                state.set_volume(client_id, ci.volume, ci.muted);
            }
        }
        other => debug!("Ignoring unexpected message from client: {other:?}"),
    }
    Ok(())
}

async fn read_message(stream: &mut TcpStream) -> anyhow::Result<Message> {
    let mut hdr_buf = [0u8; HEADER_SIZE];
    stream.read_exact(&mut hdr_buf).await?;
    let hdr = MessageHeader::from_bytes(&hdr_buf)?;
    let mut payload = vec![0u8; hdr.payload_size as usize];
    stream.read_exact(&mut payload).await?;
    Ok(Message::from_payload(&hdr, &payload)?)
}
