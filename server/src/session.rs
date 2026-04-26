//! Per-client session handler.
//!
//! Each connected client runs in its own Tokio task.  The session:
//!
//! 1. Reads the initial `Hello` message and registers the client with
//!    [`ServerState`].
//! 2. Resolves the client's group → stream → [`Broadcaster`] and subscribes.
//! 3. Sends the current `CodecHeader` and `ServerSettings`.
//! 4. Forwards audio frames while concurrently handling `Time` / `ClientInfo`.
//! 5. **Live stream switching**: watches the [`EventBus`] for
//!    `ClientGroupChanged` and `GroupStreamChanged` events and re-subscribes
//!    to the new broadcaster without dropping the TCP connection.
//! 6. Marks the client disconnected in [`ServerState`] on exit.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tracing::{debug, info, instrument, warn};
use bytes::Bytes;

use sonium_common::config::ServerConfig;
use sonium_control::{ServerState, ws::Event};
use sonium_protocol::{
    MessageHeader, MessageType, Timestamp,
    messages::{Message, ServerSettings, TimeMsg},
    header::HEADER_SIZE,
};

use crate::broadcaster::{AudioFrame, BroadcasterRegistry, lookup};

static MSG_SEQ: AtomicU16 = AtomicU16::new(1);

fn next_id() -> u16 {
    MSG_SEQ.fetch_add(1, Ordering::Relaxed)
}

#[instrument(skip_all, fields(%peer, client_id = tracing::field::Empty))]
pub async fn handle(
    mut stream: TcpStream,
    peer:       SocketAddr,
    registry:   Arc<BroadcasterRegistry>,
    cfg:        ServerConfig,
    state:      Arc<ServerState>,
) -> anyhow::Result<()> {
    let hello_msg = read_message(&mut stream).await?;
    let (client_id, hostname, client_name, os, arch, proto_ver) =
        if let Message::Hello(h) = &hello_msg {
            debug!(%peer, id = %h.id, "Hello received");
            (h.id.clone(), h.hostname.clone(), h.client_name.clone(),
             h.os.clone(), h.arch.clone(), h.protocol_version)
        } else {
            return Err(anyhow::anyhow!("expected Hello, got {:?}", hello_msg.message_type()));
        };

    tracing::Span::current().record("client_id", &client_id.as_str());
    state.client_connected(&client_id, &hostname, &client_name, &os, &arch, peer, proto_ver);

    let result = session_loop(&mut stream, peer, registry, cfg, state.clone(), &client_id).await;
    state.client_disconnected(&client_id);
    result
}

async fn session_loop(
    stream:    &mut TcpStream,
    peer:      SocketAddr,
    registry:  Arc<BroadcasterRegistry>,
    cfg:       ServerConfig,
    state:     Arc<ServerState>,
    client_id: &str,
) -> anyhow::Result<()> {
    // Resolve initial stream subscription.
    let mut stream_id = state.client_stream_id(client_id)
        .unwrap_or_else(|| "default".into());
    let mut group_id = state.get_client(client_id)
        .map(|c| c.group_id.clone())
        .unwrap_or_else(|| "default".into());

    let mut bc = lookup(&registry, &stream_id);

    let buffer_ms = bc.as_ref()
        .map(|b| b.buffer_ms)
        .unwrap_or_else(|| cfg.default_stream().buffer_ms);

    // Send CodecHeader if stream is already active.
    if let Some(b) = &bc {
        if let Some(hdr) = b.codec_header() {
            stream.write_all(&hdr).await?;
        }
    }

    // Send initial ServerSettings.
    send_server_settings(stream, buffer_ms).await?;

    info!(%peer, stream = %stream_id, "Session ready");

    let mut audio_rx: Option<broadcast::Receiver<AudioFrame>> =
        bc.as_ref().map(|b| b.subscribe());
    let mut events_rx = state.events().subscribe();

    // ── Server-side volume mixing ─────────────────────────────────────────
    let (init_vol, init_muted) = state.get_volume(client_id).unwrap_or((100, false));
    let mut client_volume: u8   = init_vol;
    let mut client_muted:  bool = init_muted;
    let mut hdr_buf = [0u8; HEADER_SIZE];

    loop {
        tokio::select! {
            // ── Incoming message from client ──────────────────────────────
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

            // ── Outgoing audio frame (with server-side volume mixing) ─────
            frame = recv_audio(&mut audio_rx) => {
                match frame {
                    Ok(f) => {
                        let wire = apply_volume(&f.wire_bytes, client_volume, client_muted);
                        if stream.write_all(&wire).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(%peer, dropped = n, "Client lagged");
                    }
                    Err(_) => break,
                }
            }

            // ── Server-side state events (live stream switching) ──────────
            event = events_rx.recv() => {
                match event {
                    Ok(Event::ClientGroupChanged { client_id: cid, group_id: new_gid })
                        if cid == client_id =>
                    {
                        group_id = new_gid.clone();
                        // Look up the stream assigned to the new group.
                        if let Some(new_sid) = state.get_group(&new_gid)
                            .map(|g| g.stream_id.clone())
                        {
                            switch_stream(
                                stream, &registry,
                                &mut audio_rx, &mut stream_id, &mut bc,
                                &new_sid,
                            ).await?;
                        }
                    }

                    Ok(Event::GroupStreamChanged { group_id: gid, stream_id: new_sid })
                        if gid == group_id =>
                    {
                        switch_stream(
                            stream, &registry,
                            &mut audio_rx, &mut stream_id, &mut bc,
                            &new_sid,
                        ).await?;
                    }

                    Ok(Event::VolumeChanged { client_id: cid, volume, muted })
                        if cid == client_id =>
                    {
                        client_volume = volume;
                        client_muted  = muted;
                        debug!(%peer, volume, muted, "Volume updated (server-side mix)");
                    }

                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(%peer, dropped = n, "Event bus lagged");
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

/// Re-subscribe to a different stream broadcaster and notify the client.
async fn switch_stream(
    wire:       &mut TcpStream,
    registry:   &Arc<BroadcasterRegistry>,
    audio_rx:   &mut Option<broadcast::Receiver<AudioFrame>>,
    stream_id:  &mut String,
    current_bc: &mut Option<Arc<crate::broadcaster::Broadcaster>>,
    new_sid:    &str,
) -> anyhow::Result<()> {
    if *stream_id == new_sid { return Ok(()); }

    info!(old = %stream_id, new = %new_sid, "Live stream switch");
    *stream_id = new_sid.to_owned();

    let new_bc = lookup(registry, new_sid);
    if let Some(bc) = &new_bc {
        // Send the new stream's CodecHeader so the client re-initialises its decoder.
        if let Some(hdr) = bc.codec_header() {
            wire.write_all(&hdr).await?;
        }
        *audio_rx = Some(bc.subscribe());
    } else {
        *audio_rx = None;
    }
    *current_bc = new_bc;
    Ok(())
}

/// Receive the next audio frame.  Returns `Pending` when `audio_rx` is `None`
/// (no stream assigned yet), keeping the select loop from spinning.
async fn recv_audio(
    rx: &mut Option<broadcast::Receiver<AudioFrame>>,
) -> Result<AudioFrame, broadcast::error::RecvError> {
    match rx {
        Some(r) => r.recv().await,
        None    => std::future::pending().await,
    }
}

async fn send_server_settings(stream: &mut TcpStream, buffer_ms: u32) -> anyhow::Result<()> {
    let settings = ServerSettings {
        buffer_ms: buffer_ms as i32,
        latency:   0,
        volume:    100,
        muted:     false,
    };
    let mut hdr = MessageHeader::new(MessageType::ServerSettings, 0);
    hdr.id = next_id();
    stream.write_all(&Message::ServerSettings(settings).encode_with_header(hdr)).await?;
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
            let now  = Timestamp::now();
            let diff = Timestamp {
                sec:  now.sec  - hdr.sent.sec,
                usec: now.usec - hdr.sent.usec,
            };
            let mut reply = MessageHeader::new(MessageType::Time, 8);
            reply.id        = next_id();
            reply.refers_to = hdr.id;
            reply.received  = now;
            stream.write_all(
                &Message::Time(TimeMsg { latency: diff }).encode_with_header(reply)
            ).await?;
        }
        MessageType::ClientInfo => {
            if let Ok(Message::ClientInfo(ci)) = Message::from_payload(&hdr, payload) {
                state.set_volume(client_id, ci.volume, ci.muted);
            }
        }
        other => debug!("Ignoring message: {other:?}"),
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

// ── Server-side volume mixing ────────────────────────────────────────────────

/// Audio data offset inside a full wire message (26-byte header + 12-byte
/// WireChunk header = 38).
const AUDIO_DATA_OFFSET: usize = HEADER_SIZE + 12; // 26 + 4 + 4 + 4

/// Apply volume/mute to a raw wire message, returning the (possibly modified)
/// bytes ready to write.
///
/// **Hot path optimisation:** volume = 100, not muted → returns the original
/// `Bytes` with zero copies.  Mute and volume scaling only allocate when
/// actually needed.
///
/// The function operates on interleaved i16 LE PCM samples inside the
/// WireChunk data section.  For compressed codecs (Opus, FLAC) the volume
/// scaling is approximate (it scales the raw bytes as if they were PCM), but
/// mute always works correctly since it zeros the entire payload.
fn apply_volume(wire: &Bytes, volume: u8, muted: bool) -> Bytes {
    // Fast path: full volume, not muted — zero-copy.
    if !muted && volume >= 100 {
        return wire.clone();
    }

    let mut buf = wire.to_vec();

    if buf.len() <= AUDIO_DATA_OFFSET {
        // Message too short to contain audio data — pass through unchanged.
        return wire.clone();
    }

    let audio = &mut buf[AUDIO_DATA_OFFSET..];

    if muted {
        // Zero entire audio payload → silence.
        audio.iter_mut().for_each(|b| *b = 0);
    } else {
        // Scale each i16 LE sample by volume / 100.
        let gain = volume as f32 / 100.0;
        for chunk in audio.chunks_exact_mut(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            let scaled = (sample as f32 * gain) as i16;
            let bytes  = scaled.to_le_bytes();
            chunk[0] = bytes[0];
            chunk[1] = bytes[1];
        }
    }

    Bytes::from(buf)
}
