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
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tracing::{debug, info, instrument, warn};

use sonium_common::config::ServerConfig;
use sonium_control::{ws::Event, ServerState};
use sonium_protocol::{
    header::HEADER_SIZE,
    messages::{Message, ServerSettings, TimeMsg},
    MessageHeader, MessageType, Timestamp,
};

use crate::broadcaster::{lookup, AudioFrame, BroadcasterRegistry};
use crate::metrics;

static MSG_SEQ: AtomicU16 = AtomicU16::new(1);

fn next_id() -> u16 {
    MSG_SEQ.fetch_add(1, Ordering::Relaxed)
}

#[instrument(skip_all, fields(%peer, client_id = tracing::field::Empty))]
pub async fn handle(
    mut stream: TcpStream,
    peer: SocketAddr,
    registry: Arc<BroadcasterRegistry>,
    cfg: ServerConfig,
    state: Arc<ServerState>,
) -> anyhow::Result<()> {
    let hello_msg = read_message(&mut stream).await?;
    let (client_id, hostname, client_name, os, arch, proto_ver) =
        if let Message::Hello(h) = &hello_msg {
            debug!(%peer, id = %h.id, "Hello received");
            (
                h.id.clone(),
                h.hostname.clone(),
                h.client_name.clone(),
                h.os.clone(),
                h.arch.clone(),
                h.protocol_version,
            )
        } else {
            return Err(anyhow::anyhow!(
                "expected Hello, got {:?}",
                hello_msg.message_type()
            ));
        };

    tracing::Span::current().record("client_id", client_id.as_str());
    metrics::TOTAL_CONNECTIONS.inc();
    metrics::CONNECTED_CLIENTS.inc();
    state.client_connected(
        &client_id,
        &hostname,
        &client_name,
        &os,
        &arch,
        peer,
        proto_ver,
    );

    let result = session_loop(&mut stream, peer, registry, cfg, state.clone(), &client_id).await;
    state.client_disconnected(&client_id);
    metrics::CONNECTED_CLIENTS.dec();
    result
}

async fn session_loop(
    stream: &mut TcpStream,
    peer: SocketAddr,
    registry: Arc<BroadcasterRegistry>,
    cfg: ServerConfig,
    state: Arc<ServerState>,
    client_id: &str,
) -> anyhow::Result<()> {
    // Resolve initial stream subscription.
    let mut stream_id = state
        .client_stream_id(client_id)
        .unwrap_or_else(|| "default".into());
    let mut group_id = state
        .get_client(client_id)
        .map(|c| c.group_id.clone())
        .unwrap_or_else(|| "default".into());

    let mut bc = lookup(&registry, &stream_id);

    let buffer_ms = bc
        .as_ref()
        .map(|b| b.buffer_ms)
        .unwrap_or_else(|| cfg.default_stream().buffer_ms);

    // Send CodecHeader if stream is already active.
    if let Some(b) = &bc {
        if let Some(hdr) = b.codec_header() {
            stream.write_all(&hdr).await?;
        }
    }

    let init_vol = state.get_volume(client_id).unwrap_or((100, false));
    let init_client = state.get_client(client_id);
    let init_latency = init_client.as_ref().map(|c| c.latency_ms).unwrap_or(0);
    let (init_eq_bands, init_eq_enabled) = state.get_stream_eq(&stream_id).unwrap_or_default();

    // Send initial ServerSettings.
    send_server_settings(
        stream,
        buffer_ms,
        init_vol.0,
        init_vol.1,
        init_latency,
        init_eq_bands,
        init_eq_enabled,
    )
    .await?;

    info!(%peer, stream = %stream_id, "Session ready");

    let mut audio_rx: Option<broadcast::Receiver<AudioFrame>> = bc.as_ref().map(|b| b.subscribe());
    let mut events_rx = state.events().subscribe();

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
                        if stream.write_all(&f.wire_bytes).await.is_err() { break; }
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

                    Ok(Event::StreamRestarted { stream_id: sid })
                        if sid == stream_id =>
                    {
                        // The stream we are listening to was restarted (e.g. config reload).
                        // Force a re-subscription. We trick switch_stream by temporarily
                        // clearing our current stream_id.
                        let current_sid = stream_id.clone();
                        stream_id.clear();
                        switch_stream(
                            stream, &registry,
                            &mut audio_rx, &mut stream_id, &mut bc,
                            &current_sid,
                        ).await?;
                    }

                    Ok(Event::StreamRemoved { stream_id: sid })
                        if sid == stream_id =>
                    {
                        // The stream we were listening to was removed entirely.
                        // We stay connected but drop the audio subscription.
                        audio_rx = None;
                        bc = None;
                    }

                    Ok(Event::VolumeChanged { client_id: cid, volume, muted })
                        if cid == client_id =>
                    {
                        let c = state.get_client(client_id);
                        let lat = c.as_ref().map(|c| c.latency_ms).unwrap_or(0);
                        let (eq, en) = state.get_stream_eq(&stream_id).unwrap_or_default();
                        send_server_settings(stream, buffer_ms, volume, muted, lat, eq, en).await?;
                        debug!(%peer, volume, muted, "Volume settings pushed to client");
                    }

                    Ok(Event::LatencyChanged { client_id: cid, latency_ms })
                        if cid == client_id =>
                    {
                        let (vol, muted) = state.get_volume(client_id).unwrap_or((100, false));
                        let (eq, en) = state.get_stream_eq(&stream_id).unwrap_or_default();
                        send_server_settings(stream, buffer_ms, vol, muted, latency_ms, eq, en).await?;
                        debug!(%peer, latency_ms, "Latency settings pushed to client");
                    }

                    Ok(Event::StreamEqChanged { stream_id: sid, eq_bands, enabled })
                        if sid == stream_id =>
                    {
                        let (vol, muted) = state.get_volume(client_id).unwrap_or((100, false));
                        let c = state.get_client(client_id);
                        let lat = c.as_ref().map(|c| c.latency_ms).unwrap_or(0);
                        send_server_settings(stream, buffer_ms, vol, muted, lat, eq_bands, enabled).await?;
                        debug!(%peer, stream_id, "Stream EQ settings pushed to client");
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
    wire: &mut TcpStream,
    registry: &Arc<BroadcasterRegistry>,
    audio_rx: &mut Option<broadcast::Receiver<AudioFrame>>,
    stream_id: &mut String,
    current_bc: &mut Option<Arc<crate::broadcaster::Broadcaster>>,
    new_sid: &str,
) -> anyhow::Result<()> {
    if *stream_id == new_sid {
        return Ok(());
    }

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
        None => std::future::pending().await,
    }
}

async fn send_server_settings(
    stream: &mut TcpStream,
    buffer_ms: u32,
    volume: u8,
    muted: bool,
    latency_ms: i32,
    eq_bands: Vec<sonium_protocol::messages::EqBand>,
    eq_enabled: bool,
) -> anyhow::Result<()> {
    let settings = ServerSettings {
        buffer_ms: buffer_ms as i32,
        latency: latency_ms,
        volume,
        muted,
        eq_bands,
        eq_enabled,
    };
    let mut hdr = MessageHeader::new(MessageType::ServerSettings, 0);
    hdr.id = next_id();
    stream
        .write_all(&Message::ServerSettings(settings).encode_with_header(hdr))
        .await?;
    Ok(())
}

async fn handle_client_msg(
    stream: &mut TcpStream,
    state: &ServerState,
    client_id: &str,
    hdr: MessageHeader,
    payload: &[u8],
) -> anyhow::Result<()> {
    match hdr.msg_type {
        MessageType::Time => {
            let now = Timestamp::now();
            let diff = Timestamp {
                sec: now.sec - hdr.sent.sec,
                usec: now.usec - hdr.sent.usec,
            };
            let mut reply = MessageHeader::new(MessageType::Time, 8);
            reply.id = next_id();
            reply.refers_to = hdr.id;
            reply.received = now;
            stream
                .write_all(&Message::Time(TimeMsg { latency: diff }).encode_with_header(reply))
                .await?;
        }
        MessageType::ClientInfo => {
            if let Ok(Message::ClientInfo(ci)) = Message::from_payload(&hdr, payload) {
                state.set_volume(client_id, ci.volume, ci.muted);
            }
        }
        MessageType::HealthReport => {
            if let Ok(Message::HealthReport(health)) = Message::from_payload(&hdr, payload) {
                state.set_client_health(client_id, health);
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
