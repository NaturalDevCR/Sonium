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
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, info, instrument, warn};

use sonium_common::config::ServerConfig;
use sonium_control::{ws::Event, ServerState};
use sonium_protocol::{
    header::{validate_payload_size, HEADER_SIZE},
    messages::{
        jitter_warning_ms, low_buffer_warning_ms, AudioHealthState, HealthReport, Message,
        ServerSettings, TimeMsg,
    },
    MessageHeader, MessageType, Timestamp,
};

use crate::broadcaster::{lookup, AudioFrame, BroadcasterRegistry};
use crate::metrics;

static MSG_SEQ: AtomicU16 = AtomicU16::new(1);
const READ_TIMEOUT: Duration = Duration::from_secs(20);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct AutoBufferTuner {
    enabled: bool,
    min_ms: u32,
    max_ms: u32,
    step_up_ms: u32,
    step_down_ms: u32,
    cooldown: Duration,
    last_adjust: Instant,
    last_underrun: u32,
    last_stale: u32,
}

impl AutoBufferTuner {
    fn from_config(cfg: &ServerConfig) -> Self {
        let min_ms = cfg.server.auto_buffer_min_ms.max(40);
        let max_ms = cfg.server.auto_buffer_max_ms.max(min_ms);
        Self {
            enabled: cfg.server.auto_buffer,
            min_ms,
            max_ms,
            step_up_ms: cfg.server.auto_buffer_step_up_ms.max(20),
            step_down_ms: cfg.server.auto_buffer_step_down_ms.max(10),
            cooldown: Duration::from_millis(cfg.server.auto_buffer_cooldown_ms.max(1_000)),
            last_adjust: Instant::now(),
            last_underrun: 0,
            last_stale: 0,
        }
    }

    fn on_health(&mut self, report: &HealthReport, current_buffer_ms: u32) -> Option<u32> {
        if !self.enabled {
            self.last_underrun = report.underrun_count;
            self.last_stale = report.stale_drop_count;
            return None;
        }

        let now = Instant::now();
        let underrun_increased = report.underrun_count > self.last_underrun;
        let stale_increased = report.stale_drop_count > self.last_stale;
        let high_jitter = report.jitter_ms > (current_buffer_ms.saturating_mul(7) / 10).max(80);
        let low_jitter = report.jitter_ms < (current_buffer_ms.saturating_mul(35) / 100).max(40);

        self.last_underrun = report.underrun_count;
        self.last_stale = report.stale_drop_count;

        if now.duration_since(self.last_adjust) < self.cooldown {
            return None;
        }

        if (underrun_increased || stale_increased || high_jitter) && current_buffer_ms < self.max_ms
        {
            self.last_adjust = now;
            return Some((current_buffer_ms + self.step_up_ms).min(self.max_ms));
        }

        if !underrun_increased && !stale_increased && low_jitter && current_buffer_ms > self.min_ms
        {
            self.last_adjust = now;
            return Some(
                current_buffer_ms
                    .saturating_sub(self.step_down_ms)
                    .max(self.min_ms),
            );
        }

        None
    }
}

#[derive(Debug, Default)]
struct HealthTransitionTracker {
    last_report: Option<HealthReport>,
    last_state: Option<AudioHealthState>,
}

impl HealthTransitionTracker {
    fn observe(
        &mut self,
        client_id: &str,
        report: &HealthReport,
        current_buffer_ms: u32,
    ) -> AudioHealthState {
        let mut state =
            classify_health_report(report, self.last_report.as_ref(), current_buffer_ms);

        if matches!(
            self.last_state,
            Some(AudioHealthState::Degraded | AudioHealthState::Underrun)
        ) && state == AudioHealthState::Stable
        {
            state = AudioHealthState::Recovering;
        }

        if self.last_state != Some(state) {
            debug!(
                %client_id,
                transport = "tcp",
                state = %state,
                buffer_depth_ms = report.buffer_depth_ms,
                jitter_ms = report.jitter_ms,
                underruns = report.underrun_count,
                stale_drops = report.stale_drop_count,
                overruns = report.overrun_count,
                callback_starvations = report.callback_starvation_count,
                callback_xruns = report.audio_callback_xrun_count,
                clock_offset_ms = report.latency_ms,
                "Client health transition"
            );
        }

        self.last_report = Some(report.clone());
        self.last_state = Some(state);
        state
    }
}

fn classify_health_report(
    report: &HealthReport,
    previous: Option<&HealthReport>,
    current_buffer_ms: u32,
) -> AudioHealthState {
    let Some(previous) = previous else {
        return report.snapshot_state(current_buffer_ms);
    };

    let playout_queue_ms = report.total_playout_queue_ms();
    if playout_queue_ms == 0 {
        return AudioHealthState::Buffering;
    }

    let underrun_delta = report
        .underrun_count
        .saturating_sub(previous.underrun_count);
    let stale_delta = report
        .stale_drop_count
        .saturating_sub(previous.stale_drop_count);
    let high_jitter = report.jitter_ms > jitter_warning_ms(current_buffer_ms);
    let low_buffer = playout_queue_ms < low_buffer_warning_ms(current_buffer_ms);
    let callback_unhealthy =
        report.callback_starvation_count > 0 || report.audio_callback_xrun_count > 0;

    if underrun_delta > 0 {
        AudioHealthState::Underrun
    } else if stale_delta > 0
        || report.overrun_count > 0
        || high_jitter
        || low_buffer
        || callback_unhealthy
    {
        AudioHealthState::Degraded
    } else {
        AudioHealthState::Stable
    }
}

enum IncomingClientFrame {
    Message(MessageHeader, Vec<u8>),
    Closed(String),
}

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

    let (reader, mut writer) = stream.into_split();
    let result = session_loop(
        reader,
        &mut writer,
        peer,
        registry,
        cfg,
        state.clone(),
        &client_id,
    )
    .await;
    state.client_disconnected(&client_id);
    metrics::CONNECTED_CLIENTS.dec();
    result
}

async fn session_loop(
    reader: OwnedReadHalf,
    stream: &mut OwnedWriteHalf,
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

    // Send CodecHeader if stream is already active.
    if let Some(b) = &bc {
        if let Some(hdr) = b.codec_header() {
            write_all_with_timeout(stream, &hdr).await?;
        }
    }

    let init_vol = state.get_volume(client_id).unwrap_or((100, false));
    let init_client = state.get_client(client_id);
    let init_latency = init_client.as_ref().map(|c| c.latency_ms).unwrap_or(0);
    let init_observability = init_client
        .as_ref()
        .map(|c| c.observability_enabled)
        .unwrap_or(false);
    let (init_eq_bands, init_eq_enabled) = state.get_stream_eq(&stream_id).unwrap_or_default();

    // Send initial ServerSettings.
    let init_buffer = bc
        .as_ref()
        .map(|b| b.buffer_ms)
        .unwrap_or(cfg.server.buffer_ms);
    let mut current_buffer_ms = init_buffer;
    let mut auto_buffer_tuner = AutoBufferTuner::from_config(&cfg);

    send_server_settings(
        stream,
        current_buffer_ms,
        init_vol.0,
        init_vol.1,
        init_latency,
        init_eq_bands,
        init_eq_enabled,
        init_observability,
    )
    .await?;

    info!(%peer, stream = %stream_id, "Session ready");

    let mut audio_rx: Option<broadcast::Receiver<AudioFrame>> = bc.as_ref().map(|b| b.subscribe());
    let mut events_rx = state.events().subscribe();

    let (incoming_tx, mut incoming_rx) = mpsc::unbounded_channel();
    let read_task = tokio::spawn(socket_reader(reader, incoming_tx));
    let mut health_tracker = HealthTransitionTracker::default();

    let result = loop {
        tokio::select! {
            // ── Incoming message from client ──────────────────────────────
            incoming = incoming_rx.recv() => {
                let Some(incoming) = incoming else {
                    debug!(%peer, "Client reader stopped");
                    break Ok(());
                };
                match incoming {
                    IncomingClientFrame::Message(hdr, payload) => {
                        handle_client_msg(
                            ClientMsgContext {
                                stream,
                                state: &state,
                                client_id,
                                current_buffer_ms: &mut current_buffer_ms,
                                auto_buffer_tuner: &mut auto_buffer_tuner,
                                health_tracker: &mut health_tracker,
                            },
                            hdr,
                            &payload,
                        ).await?;
                    }
                    IncomingClientFrame::Closed(reason) => {
                        info!(%peer, %reason, "Client reader closed");
                        break Ok(());
                    }
                }
            }

            // ── Outgoing audio frame (with server-side volume mixing) ─────
            frame = recv_audio(&mut audio_rx) => {
                match frame {
                    Ok(f) => {
                        if let Err(e) = write_all_with_timeout(stream, &f.wire_bytes).await {
                            warn!(%peer, error = %e, "Audio frame write failed");
                            break Ok(());
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(%peer, dropped = n, "Client lagged");
                    }
                    Err(_) => break Ok(()),
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
                            current_buffer_ms =
                                bc.as_ref().map(|x| x.buffer_ms).unwrap_or(cfg.server.buffer_ms);
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
                        current_buffer_ms =
                            bc.as_ref().map(|x| x.buffer_ms).unwrap_or(cfg.server.buffer_ms);
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
                        current_buffer_ms =
                            bc.as_ref().map(|x| x.buffer_ms).unwrap_or(cfg.server.buffer_ms);
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
                        let obs = c.as_ref().map(|c| c.observability_enabled).unwrap_or(false);
                        let (eq, en) = state.get_stream_eq(&stream_id).unwrap_or_default();
                        send_server_settings(stream, current_buffer_ms, volume, muted, lat, eq, en, obs).await?;
                        debug!(%peer, volume, muted, "Volume settings pushed to client");
                    }

                    Ok(Event::LatencyChanged { client_id: cid, latency_ms })
                        if cid == client_id =>
                    {
                        let (vol, muted) = state.get_volume(client_id).unwrap_or((100, false));
                        let obs = state.get_client(client_id).map(|c| c.observability_enabled).unwrap_or(false);
                        let (eq, en) = state.get_stream_eq(&stream_id).unwrap_or_default();
                        send_server_settings(stream, current_buffer_ms, vol, muted, latency_ms, eq, en, obs).await?;
                        debug!(%peer, latency_ms, "Latency settings pushed to client");
                    }

                    Ok(Event::ClientObservabilityChanged { client_id: cid, enabled })
                        if cid == client_id =>
                    {
                        debug!(%peer, enabled, "Observability updated server-side");
                    }

                    Ok(Event::StreamEqChanged { stream_id: sid, eq_bands, enabled })
                        if sid == stream_id =>
                    {
                        let (vol, muted) = state.get_volume(client_id).unwrap_or((100, false));
                        let c = state.get_client(client_id);
                        let lat = c.as_ref().map(|c| c.latency_ms).unwrap_or(0);
                        let obs = c.as_ref().map(|c| c.observability_enabled).unwrap_or(false);
                        send_server_settings(stream, current_buffer_ms, vol, muted, lat, eq_bands, enabled, obs).await?;
                        debug!(%peer, stream_id, "Stream EQ settings pushed to client");
                    }

                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(%peer, dropped = n, "Event bus lagged");
                    }
                    _ => {}
                }
            }
        }
    };

    read_task.abort();
    result
}

/// Re-subscribe to a different stream broadcaster and notify the client.
async fn switch_stream(
    wire: &mut OwnedWriteHalf,
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
            write_all_with_timeout(wire, &hdr).await?;
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

#[allow(clippy::too_many_arguments)]
async fn send_server_settings(
    stream: &mut OwnedWriteHalf,
    buffer_ms: u32,
    volume: u8,
    muted: bool,
    latency_ms: i32,
    eq_bands: Vec<sonium_protocol::messages::EqBand>,
    eq_enabled: bool,
    observability_enabled: bool,
) -> anyhow::Result<()> {
    let settings = ServerSettings {
        buffer_ms: buffer_ms as i32,
        latency: latency_ms,
        volume,
        muted,
        eq_bands,
        eq_enabled,
        observability_enabled,
    };
    let mut hdr = MessageHeader::new(MessageType::ServerSettings, 0);
    hdr.id = next_id();
    write_all_with_timeout(
        stream,
        &Message::ServerSettings(settings).encode_with_header(hdr),
    )
    .await?;
    Ok(())
}

struct ClientMsgContext<'a> {
    stream: &'a mut OwnedWriteHalf,
    state: &'a ServerState,
    client_id: &'a str,
    current_buffer_ms: &'a mut u32,
    auto_buffer_tuner: &'a mut AutoBufferTuner,
    health_tracker: &'a mut HealthTransitionTracker,
}

async fn handle_client_msg(
    ctx: ClientMsgContext<'_>,
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
            write_all_with_timeout(
                ctx.stream,
                &Message::Time(TimeMsg { latency: diff }).encode_with_header(reply),
            )
            .await?;
        }
        MessageType::ClientInfo => {
            if let Ok(Message::ClientInfo(ci)) = Message::from_payload(&hdr, payload) {
                ctx.state.set_volume(ctx.client_id, ci.volume, ci.muted);
            }
        }
        MessageType::HealthReport => {
            if let Ok(Message::HealthReport(health)) = Message::from_payload(&hdr, payload) {
                let health_state =
                    ctx.health_tracker
                        .observe(ctx.client_id, &health, *ctx.current_buffer_ms);
                metrics::observe_client_health(ctx.client_id, "tcp", &health, health_state);

                if let Some(next_buffer_ms) = ctx
                    .auto_buffer_tuner
                    .on_health(&health, *ctx.current_buffer_ms)
                {
                    let (volume, muted) =
                        ctx.state.get_volume(ctx.client_id).unwrap_or((100, false));
                    let c = ctx.state.get_client(ctx.client_id);
                    let latency_ms = c.as_ref().map(|x| x.latency_ms).unwrap_or(0);
                    let obs = c.as_ref().map(|x| x.observability_enabled).unwrap_or(false);
                    let stream_id = ctx
                        .state
                        .client_stream_id(ctx.client_id)
                        .unwrap_or_else(|| "default".into());
                    let (eq_bands, eq_enabled) =
                        ctx.state.get_stream_eq(&stream_id).unwrap_or_default();
                    send_server_settings(
                        ctx.stream,
                        next_buffer_ms,
                        volume,
                        muted,
                        latency_ms,
                        eq_bands,
                        eq_enabled,
                        obs,
                    )
                    .await?;
                    *ctx.current_buffer_ms = next_buffer_ms;
                    debug!(client_id = %ctx.client_id, buffer_ms = next_buffer_ms, "Auto buffer adjusted");
                }
                if ctx
                    .state
                    .get_client(ctx.client_id)
                    .map(|c| c.observability_enabled)
                    .unwrap_or(false)
                {
                    ctx.state.set_client_health(ctx.client_id, health);
                }
            }
        }
        other => debug!("Ignoring message: {other:?}"),
    }
    Ok(())
}

async fn socket_reader(mut reader: OwnedReadHalf, tx: mpsc::UnboundedSender<IncomingClientFrame>) {
    loop {
        let mut hdr_buf = [0u8; HEADER_SIZE];
        if let Err(e) = read_exact_with_timeout(&mut reader, &mut hdr_buf).await {
            let _ = tx.send(IncomingClientFrame::Closed(e.to_string()));
            break;
        }

        let hdr = match MessageHeader::from_bytes(&hdr_buf) {
            Ok(hdr) => hdr,
            Err(e) => {
                let _ = tx.send(IncomingClientFrame::Closed(format!("invalid header: {e}")));
                break;
            }
        };

        let payload_size = match validate_payload_size(&hdr) {
            Ok(size) => size,
            Err(e) => {
                let _ = tx.send(IncomingClientFrame::Closed(e.to_string()));
                break;
            }
        };

        let mut payload = vec![0u8; payload_size];
        if let Err(e) = read_exact_with_timeout(&mut reader, &mut payload).await {
            let _ = tx.send(IncomingClientFrame::Closed(format!(
                "error reading payload: {e}"
            )));
            break;
        }

        if tx.send(IncomingClientFrame::Message(hdr, payload)).is_err() {
            break;
        }
    }
}

async fn read_message(stream: &mut TcpStream) -> anyhow::Result<Message> {
    let mut hdr_buf = [0u8; HEADER_SIZE];
    read_exact_with_timeout(stream, &mut hdr_buf).await?;
    let hdr = MessageHeader::from_bytes(&hdr_buf)?;
    let payload_size = validate_payload_size(&hdr)?;
    let mut payload = vec![0u8; payload_size];
    read_exact_with_timeout(stream, &mut payload).await?;
    Ok(Message::from_payload(&hdr, &payload)?)
}

async fn read_exact_with_timeout<R>(reader: &mut R, buf: &mut [u8]) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
{
    match timeout(READ_TIMEOUT, reader.read_exact(buf)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => Err(anyhow::anyhow!("read timed out after {:?}", READ_TIMEOUT)),
    }
}

async fn write_all_with_timeout<W>(writer: &mut W, buf: &[u8]) -> anyhow::Result<()>
where
    W: AsyncWrite + Unpin,
{
    match timeout(WRITE_TIMEOUT, writer.write_all(buf)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => Err(anyhow::anyhow!("write timed out after {:?}", WRITE_TIMEOUT)),
    }
}
