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
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;
use tracing::{debug, info, instrument, warn};

/// Capacity of the per-session audio write queue.
///
/// At 20 ms/chunk this is ~20 seconds of audio.  When the queue fills
/// (client cannot keep up), the oldest frame is dropped — preventing
/// backpressure from stalling the session select loop.
const AUDIO_QUEUE_CAPACITY: usize = 1024;

use sonium_common::{
    config::{validate_client_id, ProtocolError, ServerConfig},
    SoniumError,
};
use sonium_control::{ws::Event, ServerState};
use sonium_protocol::{
    header::{validate_payload_size, HEADER_SIZE},
    messages::{
        jitter_warning_ms, low_buffer_warning_ms, AnnouncementControlV1, AudioHealthState,
        HealthReport, Message, ServerSettings, TimeMsg,
    },
    MessageHeader, MessageType, Timestamp,
};
use sonium_transport::{sender::MediaSender, ArqSender, RtpUdpMediaSender, TransportMode};

use crate::broadcaster::{lookup, AudioFrame, BroadcasterRegistry};
use crate::metrics;

static MSG_SEQ: AtomicU16 = AtomicU16::new(1);
const READ_TIMEOUT: Duration = Duration::from_secs(20);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// TCP send-buffer size for the audio stream connection.
///
/// 256 KB gives plenty of headroom for bursty audio writes, letting the
/// kernel absorb short-term TCP backpressure instead of blocking the
/// user-space write.
const TCP_SNDBUF: u32 = 262_144;
// Consecutive clean health-report intervals required before leaving Recovering.
// At the default 2-second reporting cadence this equals 10 seconds of clean signal.
const STABLE_STREAK_REQUIRED: u32 = 5;
const AUTO_BUFFER_CLEAN_INTERVALS_BEFORE_STEP_DOWN: u32 = 8;
const AUTO_BUFFER_RTP_BURST_THRESHOLD: u32 = 6;
const AUTO_BUFFER_STALE_BURST_THRESHOLD: u32 = 4;

/// A global admission slot held for the complete lifetime of one TCP session.
/// Dropping it releases capacity on every normal, error, or cancellation path.
pub struct ClientSessionPermit {
    _permit: OwnedSemaphorePermit,
}

impl ClientSessionPermit {
    pub fn try_acquire(capacity: Arc<Semaphore>) -> Result<Self, ProtocolError> {
        capacity
            .try_acquire_owned()
            .map(|permit| Self { _permit: permit })
            .map_err(|_| SoniumError::Protocol("maximum concurrent client sessions reached".into()))
    }
}

/// Dedicated TCP writer task.  Owns the `OwnedWriteHalf` exclusively so the
/// select loop never blocks on a TCP write.  Audio frames arrive via a bounded
/// channel (`try_send` — drop oldest on full); control messages use a separate
/// unbounded channel so they are never lost.
///
/// Audio is latency-sensitive: the writer always drains the audio queue first
/// with non-blocking `try_recv` before blocking on either channel.  This
/// prevents a slow control write (e.g. TCP backpressure on a Time reply) from
/// delaying audio and causing the bounded channel to fill up.
async fn tcp_writer_task(
    mut writer: OwnedWriteHalf,
    mut audio_rx: mpsc::Receiver<bytes::Bytes>,
    mut ctrl_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    peer: SocketAddr,
) {
    let mut audio_frames_written: u64 = 0;
    let audio_frames_dropped: u64 = 0;
    let mut slow_writes: u64 = 0;

    loop {
        // Drain audio first — non-blocking so control never starves.
        match audio_rx.try_recv() {
            Ok(buf) => {
                let t0 = std::time::Instant::now();
                if let Err(e) = write_all_with_timeout(&mut writer, &buf).await {
                    warn!(%peer, error = %e, "TCP audio write failed");
                    break;
                }
                let elapsed_ms = t0.elapsed().as_millis();
                audio_frames_written += 1;
                if elapsed_ms > 20 {
                    slow_writes += 1;
                    warn!(%peer, elapsed_ms, audio_frames_written, slow_writes, "SLOW audio write (TCP backpressure?)");
                } else {
                    tracing::trace!(%peer, elapsed_ms, audio_frames_written, "audio write ok");
                }

                if audio_frames_written.is_multiple_of(500) {
                    tracing::debug!(
                        %peer,
                        audio_frames_written,
                        audio_frames_dropped,
                        slow_writes,
                        drop_rate_pct = audio_frames_dropped * 100 / audio_frames_written.max(1),
                        "writer task stats"
                    );
                }
                continue;
            }
            Err(mpsc::error::TryRecvError::Disconnected) => break,
            Err(mpsc::error::TryRecvError::Empty) => {
                // No audio pending — block on either channel.
            }
        }

        tokio::select! {
            msg = ctrl_rx.recv() => {
                let Some(buf) = msg else { break };
                let t0 = std::time::Instant::now();
                if let Err(e) = write_all_with_timeout(&mut writer, &buf).await {
                    warn!(%peer, error = %e, "TCP control write failed");
                    break;
                }
                let elapsed_ms = t0.elapsed().as_millis();
                if elapsed_ms > 5 {
                    slow_writes += 1;
                    warn!(%peer, elapsed_ms, slow_writes, "SLOW control write (TCP backpressure?)");
                } else {
                    tracing::trace!(%peer, elapsed_ms, "ctrl write ok");
                }
            }
            msg = audio_rx.recv() => {
                let Some(buf) = msg else { break };
                let t0 = std::time::Instant::now();
                if let Err(e) = write_all_with_timeout(&mut writer, &buf).await {
                    warn!(%peer, error = %e, "TCP audio write failed");
                    break;
                }
                let elapsed_ms = t0.elapsed().as_millis();
                audio_frames_written += 1;
                if elapsed_ms > 20 {
                    slow_writes += 1;
                    warn!(%peer, elapsed_ms, audio_frames_written, slow_writes, "SLOW audio write (TCP backpressure?)");
                } else {
                    tracing::trace!(%peer, elapsed_ms, audio_frames_written, "audio write ok");
                }

                if audio_frames_written.is_multiple_of(500) {
                    tracing::debug!(
                        %peer,
                        audio_frames_written,
                        audio_frames_dropped,
                        slow_writes,
                        drop_rate_pct = audio_frames_dropped * 100 / audio_frames_written.max(1),
                        "writer task stats"
                    );
                }
            }
        }
    }

    warn!(%peer, audio_frames_written, audio_frames_dropped, slow_writes, "writer task exiting");
}

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
    last_rtp_gaps: u32,
    last_rtp_concealed: u32,
    last_rtp_decode_errors: u32,
    clean_intervals: u32,
}

impl AutoBufferTuner {
    fn from_config(cfg: &ServerConfig) -> Self {
        let min_ms = cfg.server.auto_buffer.min_ms.max(40);
        let max_ms = cfg.server.auto_buffer.max_ms.max(min_ms);
        Self {
            enabled: cfg.server.auto_buffer.enabled,
            min_ms,
            max_ms,
            step_up_ms: cfg.server.auto_buffer.step_up_ms.max(20),
            step_down_ms: cfg.server.auto_buffer.step_down_ms.max(10),
            cooldown: Duration::from_millis(cfg.server.auto_buffer.cooldown_ms.max(1_000)),
            last_adjust: Instant::now(),
            last_underrun: 0,
            last_stale: 0,
            last_rtp_gaps: 0,
            last_rtp_concealed: 0,
            last_rtp_decode_errors: 0,
            clean_intervals: 0,
        }
    }

    fn on_health(&mut self, report: &HealthReport, current_buffer_ms: u32) -> Option<u32> {
        if !self.enabled {
            self.remember(report);
            return None;
        }

        let now = Instant::now();
        let underrun_delta = report.underrun_count.saturating_sub(self.last_underrun);
        let stale_delta = report.stale_drop_count.saturating_sub(self.last_stale);
        let rtp_gap_delta = report.rtp_sequence_gaps.saturating_sub(self.last_rtp_gaps);
        let rtp_concealed_delta = report
            .rtp_concealed_packets
            .saturating_sub(self.last_rtp_concealed);
        let rtp_decode_error_delta = report
            .rtp_decode_error_count
            .saturating_sub(self.last_rtp_decode_errors);
        let high_jitter = report.jitter_ms > (current_buffer_ms.saturating_mul(7) / 10).max(80);
        let low_jitter = report.jitter_ms < (current_buffer_ms.saturating_mul(35) / 100).max(40);
        let stale_burst = stale_delta > AUTO_BUFFER_STALE_BURST_THRESHOLD;
        let rtp_burst = rtp_gap_delta > AUTO_BUFFER_RTP_BURST_THRESHOLD
            || rtp_concealed_delta > AUTO_BUFFER_RTP_BURST_THRESHOLD;
        let rtp_activity =
            rtp_gap_delta > 0 || rtp_concealed_delta > 0 || rtp_decode_error_delta > 0;
        let unhealthy = underrun_delta > 0
            || stale_delta > 0
            || high_jitter
            || rtp_decode_error_delta > 0
            || rtp_burst;

        if unhealthy || rtp_activity {
            self.clean_intervals = 0;
        } else {
            self.clean_intervals = self.clean_intervals.saturating_add(1);
        }

        self.remember(report);

        if now.duration_since(self.last_adjust) < self.cooldown {
            return None;
        }

        if unhealthy && current_buffer_ms < self.max_ms {
            self.last_adjust = now;
            let severe =
                underrun_delta > 0 || stale_burst || rtp_burst || rtp_decode_error_delta > 0;
            let step = if severe {
                self.step_up_ms.saturating_mul(2)
            } else {
                self.step_up_ms
            };
            return Some(current_buffer_ms.saturating_add(step).min(self.max_ms));
        }

        if self.clean_intervals >= AUTO_BUFFER_CLEAN_INTERVALS_BEFORE_STEP_DOWN
            && low_jitter
            && current_buffer_ms > self.min_ms
        {
            self.last_adjust = now;
            self.clean_intervals = 0;
            return Some(
                current_buffer_ms
                    .saturating_sub(self.step_down_ms)
                    .max(self.min_ms),
            );
        }

        None
    }

    fn remember(&mut self, report: &HealthReport) {
        self.last_underrun = report.underrun_count;
        self.last_stale = report.stale_drop_count;
        self.last_rtp_gaps = report.rtp_sequence_gaps;
        self.last_rtp_concealed = report.rtp_concealed_packets;
        self.last_rtp_decode_errors = report.rtp_decode_error_count;
    }
}

#[derive(Debug, Default)]
struct HealthTransitionTracker {
    last_report: Option<HealthReport>,
    last_state: Option<AudioHealthState>,
    // Counts consecutive clean intervals during recovery. Only meaningful while
    // last_state == Recovering. Reset to 0 on any degradation event.
    stable_streak: u32,
}

impl HealthTransitionTracker {
    fn observe(
        &mut self,
        client_id: &str,
        transport: &str,
        report: &HealthReport,
        current_buffer_ms: u32,
    ) -> AudioHealthState {
        let classified =
            classify_health_report(report, self.last_report.as_ref(), current_buffer_ms);

        let state = match classified {
            AudioHealthState::Stable => match self.last_state {
                // First clean interval after a bad state — enter Recovering.
                Some(AudioHealthState::Degraded | AudioHealthState::Underrun) => {
                    self.stable_streak = 1;
                    AudioHealthState::Recovering
                }
                // Already recovering — count clean intervals; promote only when streak is met.
                Some(AudioHealthState::Recovering) => {
                    self.stable_streak += 1;
                    if self.stable_streak >= STABLE_STREAK_REQUIRED {
                        self.stable_streak = 0;
                        AudioHealthState::Stable
                    } else {
                        AudioHealthState::Recovering
                    }
                }
                // No prior bad state — Stable immediately.
                _ => {
                    self.stable_streak = 0;
                    AudioHealthState::Stable
                }
            },
            other => {
                self.stable_streak = 0;
                other
            }
        };

        if self.last_state != Some(state) {
            debug!(
                %client_id,
                %transport,
                state = %state,
                stable_streak = self.stable_streak,
                buffer_depth_ms = report.buffer_depth_ms,
                jitter_ms = report.jitter_ms,
                underruns = report.underrun_count,
                stale_drops = report.stale_drop_count,
                overruns = report.overrun_count,
                rtp_packets_received = report.rtp_packets_received,
                rtp_sequence_gaps = report.rtp_sequence_gaps,
                rtp_decode_errors = report.rtp_decode_error_count,
                rtp_concealed_packets = report.rtp_concealed_packets,
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
    let rtp_gap_delta = report
        .rtp_sequence_gaps
        .saturating_sub(previous.rtp_sequence_gaps);
    let rtp_decode_error_delta = report
        .rtp_decode_error_count
        .saturating_sub(previous.rtp_decode_error_count);

    // Allow up to 2 RTP sequence gaps per 2-second health interval before
    // marking Degraded.  On Wi-Fi, 1–2 concealed packets per interval is
    // normal and handled by PLC; immediately degrading on every single gap
    // would keep the state permanently in Degraded/Recovering on any wireless
    // path.  Decode errors and larger gap bursts remain hard signals.
    let rtp_degraded = rtp_gap_delta > 2 || rtp_decode_error_delta > 0;

    if underrun_delta > 0 {
        AudioHealthState::Underrun
    } else if stale_delta > 0
        || report.overrun_count > 0
        || high_jitter
        || low_buffer
        || callback_unhealthy
        || rtp_degraded
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
#[allow(clippy::too_many_arguments)] // Session dependencies are passed independently for testability.
pub async fn handle(
    mut stream: TcpStream,
    peer: SocketAddr,
    registry: Arc<BroadcasterRegistry>,
    cfg: ServerConfig,
    state: Arc<ServerState>,
    udp_socket: Option<Arc<UdpSocket>>,
    nack_router: crate::nack_router::NackRouter,
    _permit: ClientSessionPermit,
) -> anyhow::Result<()> {
    // ── TCP tuning ────────────────────────────────────────────────────────
    // Ensure TCP_NODELAY is set (main.rs sets it too, but belt-and-braces).
    let _ = stream.set_nodelay(true);
    // Bump the kernel send buffer to absorb bursty audio writes.
    {
        let sock = socket2::SockRef::from(&stream);
        let _ = sock.set_send_buffer_size(TCP_SNDBUF as usize);
    }

    let hello_msg = read_message(&mut stream).await?;
    let (client_id, hostname, client_name, os, arch, proto_ver, hello_udp_port) =
        if let Message::Hello(h) = &hello_msg {
            debug!(%peer, id = %h.id, udp_port = h.udp_port, "Hello received");
            (
                h.id.clone(),
                h.hostname.clone(),
                h.client_name.clone(),
                h.os.clone(),
                h.arch.clone(),
                h.protocol_version,
                h.udp_port,
            )
        } else {
            return Err(anyhow::anyhow!(
                "expected Hello, got {:?}",
                hello_msg.message_type()
            ));
        };

    validate_client_id(&client_id)?;
    tracing::Span::current().record("client_id", client_id.as_str());
    let session_generation = state.try_client_connected(
        &client_id,
        &hostname,
        &client_name,
        &os,
        &arch,
        peer,
        proto_ver,
    )?;
    metrics::register_client_session(&client_id, session_generation);
    metrics::TOTAL_CONNECTIONS.inc();
    metrics::CONNECTED_CLIENTS.inc();

    let (reader, writer) = stream.into_split();
    let result = session_loop(
        reader,
        writer,
        peer,
        SessionLoopContext {
            registry,
            cfg,
            state: state.clone(),
            client_id: client_id.clone(),
            udp_socket,
            hello_udp_port,
            nack_router,
        },
    )
    .await;
    state.client_disconnected_generation(&client_id, session_generation);
    metrics::CONNECTED_CLIENTS.dec();
    result
}

struct SessionLoopContext {
    registry: Arc<BroadcasterRegistry>,
    cfg: ServerConfig,
    state: Arc<ServerState>,
    client_id: String,
    udp_socket: Option<Arc<UdpSocket>>,
    hello_udp_port: u16,
    nack_router: crate::nack_router::NackRouter,
}

async fn session_loop(
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
    peer: SocketAddr,
    ctx: SessionLoopContext,
) -> anyhow::Result<()> {
    let SessionLoopContext {
        registry,
        cfg,
        state,
        client_id,
        udp_socket,
        hello_udp_port,
        nack_router,
    } = ctx;
    let client_id = client_id.as_str();

    // Resolve initial stream subscription.
    let mut stream_id = state
        .client_stream_id(client_id)
        .unwrap_or_else(|| "default".into());
    let mut group_id = state
        .get_client(client_id)
        .map(|c| c.group_id.clone())
        .unwrap_or_else(|| "default".into());

    let mut bc = lookup(&registry, &stream_id);

    // ── Set up writer channels ───────────────────────────────────────
    // All TCP writes go through dedicated channels so the select loop
    // never blocks on write backpressure.
    let (audio_tx, audio_write_rx) = mpsc::channel::<bytes::Bytes>(AUDIO_QUEUE_CAPACITY);
    let (ctrl_tx, ctrl_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Send CodecHeader if stream is already active (via writer task).
    // We must send these before spawning the writer so they arrive
    // before we start sending audio — but the writer task hasn't started
    // yet, so we queue them.
    if let Some(b) = &bc {
        if let Some(hdr) = b.codec_header() {
            let _ = ctrl_tx.send(hdr.to_vec());
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
        .unwrap_or(cfg.server.audio.buffer_ms);
    let mut current_buffer_ms = init_buffer;
    let mut auto_buffer_tuner = AutoBufferTuner::from_config(&cfg);

    // Determine transport mode from runtime state (operator-mutable via API).
    let transport_mode = state.transport_mode();
    let server_udp_port = state.server_udp_port();

    // Build the per-session RTP/UDP or ARQ sender if all prerequisites are met.
    enum UdpMedia {
        Rtp(RtpUdpMediaSender),
        Arq(ArqSender),
    }
    let mut udp_media: Option<UdpMedia> = None;
    let mut nack_rx: Option<crate::nack_router::NackReceiver> = None;
    let mut arq_ssrc: Option<u32> = None;

    if transport_mode == TransportMode::RtpUdp {
        match udp_socket.as_ref() {
            None => {
                warn!(%peer, "rtp_udp requested but no UDP socket bound — falling back to tcp");
            }
            Some(sock) => {
                if hello_udp_port > 0 {
                    let client_udp_addr = SocketAddr::new(peer.ip(), hello_udp_port);
                    let ssrc = {
                        use std::hash::{Hash, Hasher};
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        peer.hash(&mut h);
                        h.finish() as u32
                    };
                    udp_media = Some(UdpMedia::Rtp(RtpUdpMediaSender::new(
                        sock.clone(),
                        client_udp_addr,
                        ssrc,
                    )));
                    info!(%peer, %client_udp_addr, "RTP/UDP media path active");
                } else {
                    warn!(%peer, "rtp_udp requested but client sent udp_port=0 — falling back to tcp");
                }
            }
        }
    } else if transport_mode == TransportMode::Rist {
        match udp_socket.as_ref() {
            None => {
                warn!(%peer, "rist requested but no UDP socket bound — falling back to tcp");
            }
            Some(sock) => {
                if hello_udp_port > 0 {
                    let client_udp_addr = SocketAddr::new(peer.ip(), hello_udp_port);
                    let ssrc = {
                        use std::hash::{Hash, Hasher};
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        peer.hash(&mut h);
                        h.finish() as u32
                    };
                    // Register with the NACK router to receive client NACKs.
                    nack_rx = Some(nack_router.register(ssrc, client_udp_addr).await);
                    arq_ssrc = Some(ssrc);
                    udp_media = Some(UdpMedia::Arq(ArqSender::new(
                        sock.clone(),
                        client_udp_addr,
                        ssrc,
                    )));
                    info!(%peer, %client_udp_addr, "ARQ (Rist) media path active");
                } else {
                    warn!(%peer, "rist requested but client sent udp_port=0 — falling back to tcp");
                }
            }
        }
    } else if transport_mode != TransportMode::Tcp {
        warn!(%peer, %transport_mode, "Transport not yet implemented; falling back to tcp");
    }

    let effective_mode = match &udp_media {
        Some(UdpMedia::Rtp(_)) => TransportMode::RtpUdp,
        Some(UdpMedia::Arq(_)) => TransportMode::Rist,
        None => TransportMode::Tcp,
    };

    // Queue initial ServerSettings via control channel.
    {
        let settings = ServerSettings {
            buffer_ms: current_buffer_ms as i32,
            output_prefill_ms: cfg.server.audio.output_prefill_ms,
            latency: init_latency,
            volume: init_vol.0,
            muted: init_vol.1,
            eq_bands: init_eq_bands,
            eq_enabled: init_eq_enabled,
            observability_enabled: init_observability,
            transport_mode: effective_mode.to_string(),
            server_udp_port,
        };
        let mut hdr = MessageHeader::new(MessageType::ServerSettings, 0);
        hdr.id = next_id();
        let _ = ctrl_tx.send(Message::ServerSettings(settings).encode_with_header(hdr));
        info!(%peer, buffer_ms = current_buffer_ms, "ServerSettings sent to client");
    }

    info!(%peer, stream = %stream_id, transport = %effective_mode, "Session ready");
    metrics::observe_active_transport(client_id, &effective_mode.to_string());

    let mut audio_rx: Option<broadcast::Receiver<AudioFrame>> = bc.as_ref().map(|b| b.subscribe());
    let mut events_rx = state.events().subscribe();
    let mut announcement_rx = state.subscribe_announcement_controls();
    if let Some(control) = state.pending_announcement_control(&group_id) {
        queue_announcement_control(&ctrl_tx, &group_id, &control);
    }

    let (incoming_tx, mut incoming_rx) = mpsc::unbounded_channel();
    let read_task = tokio::spawn(socket_reader(reader, incoming_tx));
    let mut health_tracker = HealthTransitionTracker::default();

    // Group sync broadcast: shared timeline for multi-room sync.
    // 100 ms interval (was 500 ms) keeps group convergence under 1 s with
    // the adaptive nudge in TimeProvider::nudge_group_offset.
    let mut group_sync_tick = tokio::time::interval(tokio::time::Duration::from_millis(100));
    group_sync_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // ── Spawn the dedicated writer task ──────────────────────────────
    let audio_write_task = if let Some(media) = udp_media {
        // For UDP transports (RTP or ARQ): audio goes through the UDP sender,
        // control through TCP writer.
        let peer_for_task = peer;
        let udp_audio_rx = audio_write_rx;

        match media {
            UdpMedia::Rtp(mut rtp) => {
                // RTP: simple send loop, no NACK handling.
                let udp_task = tokio::spawn(async move {
                    let mut rx = udp_audio_rx;
                    while let Some(wire_bytes) = rx.recv().await {
                        if let Err(e) = rtp.send_wire_bytes(&wire_bytes).await {
                            warn!(%peer_for_task, error = %e, "RTP audio frame write failed");
                            break;
                        }
                    }
                });
                // Keep the dummy sender alive inside the task so tcp_writer_task
                // sees Empty (not Disconnected) and only exits when ctrl_rx closes.
                let (dummy_tx, dummy_rx) = mpsc::channel::<bytes::Bytes>(1);
                let _tcp_ctrl_task = tokio::spawn(async move {
                    let _keep = dummy_tx;
                    tcp_writer_task(writer, dummy_rx, ctrl_rx, peer).await;
                });
                udp_task
            }
            UdpMedia::Arq(mut arq) => {
                // ARQ: audio send loop + NACK handling from the NackRouter.
                let mut arq_nack_rx = nack_rx.take().expect("ARQ mode requires nack_rx");
                let udp_task = tokio::spawn(async move {
                    let mut rx = udp_audio_rx;
                    loop {
                        tokio::select! {
                            biased;
                            frame = rx.recv() => {
                                match frame {
                                    Some(wire_bytes) => {
                                        if let Err(e) = arq.send_audio(&wire_bytes).await {
                                            warn!(%peer_for_task, error = %e, "ARQ audio frame write failed");
                                            break;
                                        }
                                    }
                                    None => break,
                                }
                            }
                            nack = arq_nack_rx.recv() => {
                                match nack {
                                    Some(nack) => {
                                        if let Err(e) = arq.handle_nack(&nack).await {
                                            warn!(%peer_for_task, error = %e, "ARQ NACK handling failed");
                                        }
                                    }
                                    None => {
                                        // NACK channel closed — session is shutting down.
                                        break;
                                    }
                                }
                            }
                        }
                    }
                });
                // Same fix: keep dummy sender alive so the TCP control task only
                // exits when ctrl_rx closes (session end), not on Disconnected.
                let (dummy_tx, dummy_rx) = mpsc::channel::<bytes::Bytes>(1);
                let _tcp_ctrl_task = tokio::spawn(async move {
                    let _keep = dummy_tx;
                    tcp_writer_task(writer, dummy_rx, ctrl_rx, peer).await;
                });
                udp_task
            }
        }
    } else {
        // For TCP: both audio AND control go through the unified writer task.
        tokio::spawn(tcp_writer_task(writer, audio_write_rx, ctrl_rx, peer))
    };

    let result = loop {
        tokio::select! {
            biased;
            // Audio is latency-sensitive — always process before control or events.
            // ── Outgoing audio frame ──────────────────────────────────────
            frame = recv_audio(&mut audio_rx) => {
                match frame {
                    Ok(f) => {
                        if audio_tx.try_send(f.wire_bytes.clone()).is_err() {
                            let queue_cap = audio_tx.max_capacity();
                            warn!(%peer, queue_cap, "Audio queue FULL — frame dropped (client cannot consume)");
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(%peer, dropped = n, "Client lagged — broadcaster backpressure");
                    }
                    Err(_) => break Ok(()),
                }
            }

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
                                ctrl_tx: &ctrl_tx,
                                state: &state,
                                client_id,
                                group_id: &group_id,
                                current_buffer_ms: &mut current_buffer_ms,
                                auto_buffer_tuner: &mut auto_buffer_tuner,
                                health_tracker: &mut health_tracker,
                                transport_mode: effective_mode.to_string(),
                                server_udp_port,
                                output_prefill_ms: cfg.server.audio.output_prefill_ms,
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

            // ── Timestamped announcement controls for this group ──────────
            control = announcement_rx.recv() => {
                match control {
                    Ok(control) => {
                        queue_announcement_control(&ctrl_tx, &group_id, &control);
                    }
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        // The scheduler's ACK timeout recovers dropped control
                        // messages without blocking or replaying audio here.
                        warn!(%peer, dropped, "Announcement control receiver lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break Ok(()),
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
                                &ctrl_tx, &registry,
                                &mut audio_rx, &mut stream_id, &mut bc,
                                &new_sid,
                            )?;
                            current_buffer_ms =
                                bc.as_ref().map(|x| x.buffer_ms).unwrap_or(cfg.server.audio.buffer_ms);
                        }
                    }

                    Ok(Event::GroupStreamChanged { group_id: gid, stream_id: new_sid })
                        if gid == group_id =>
                    {
                        switch_stream(
                            &ctrl_tx, &registry,
                            &mut audio_rx, &mut stream_id, &mut bc,
                            &new_sid,
                        )?;
                        current_buffer_ms =
                            bc.as_ref().map(|x| x.buffer_ms).unwrap_or(cfg.server.audio.buffer_ms);
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
                            &ctrl_tx, &registry,
                            &mut audio_rx, &mut stream_id, &mut bc,
                            &current_sid,
                        )?;
                        current_buffer_ms =
                            bc.as_ref().map(|x| x.buffer_ms).unwrap_or(cfg.server.audio.buffer_ms);
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
                        send_server_settings_via_channel(&ctrl_tx, current_buffer_ms, volume, muted, lat, eq, en, obs, effective_mode.to_string(), server_udp_port, cfg.server.audio.output_prefill_ms);
                        debug!(%peer, volume, muted, "Volume settings pushed to client");
                    }

                    Ok(Event::LatencyChanged { client_id: cid, latency_ms })
                        if cid == client_id =>
                    {
                        let (vol, muted) = state.get_volume(client_id).unwrap_or((100, false));
                        let obs = state.get_client(client_id).map(|c| c.observability_enabled).unwrap_or(false);
                        let (eq, en) = state.get_stream_eq(&stream_id).unwrap_or_default();
                        send_server_settings_via_channel(&ctrl_tx, current_buffer_ms, vol, muted, latency_ms, eq, en, obs, effective_mode.to_string(), server_udp_port, cfg.server.audio.output_prefill_ms);
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
                        send_server_settings_via_channel(&ctrl_tx, current_buffer_ms, vol, muted, lat, eq_bands, enabled, obs, effective_mode.to_string(), server_udp_port, cfg.server.audio.output_prefill_ms);
                        debug!(%peer, stream_id, "Stream EQ settings pushed to client");
                    }

                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(%peer, dropped = n, "Event bus lagged");
                    }
                    _ => {}
                }
            }

            // ── Group sync broadcast ──────────────────────────────────────
            _ = group_sync_tick.tick() => {
                let server_now_us = sonium_sync::time_provider::now_us();
                // Compute the target group offset as the median of all connected
                // clients' NTP clock offsets in this group.  Every client should
                // converge its total offset (NTP + group) to this value.
                let group_offset_us = state
                    .group_median_clock_offset_us(&group_id)
                    .unwrap_or(0);
                let gs = sonium_protocol::messages::GroupSync::new(
                    server_now_us,
                    group_offset_us,
                    0, // rate_ppm: future drift correction
                    0.0, // source_quality: will be populated from chrony status when available
                );
                let mut hdr = MessageHeader::new(MessageType::GroupSync, 24);
                hdr.id = next_id();
                let _ = ctrl_tx.send(Message::GroupSync(gs).encode_with_header(hdr));
                tracing::trace!(%peer, server_now_us, group_offset_us, "GroupSync broadcast");
            }
        }
    };

    read_task.abort();
    audio_write_task.abort();
    // Unregister from the NACK router if we were in ARQ mode.
    if let Some(ssrc) = arq_ssrc {
        nack_router.unregister(ssrc).await;
    }
    result
}

/// Re-subscribe to a different stream broadcaster and notify the client.
fn switch_stream(
    ctrl_tx: &mpsc::UnboundedSender<Vec<u8>>,
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
            let _ = ctrl_tx.send(hdr.to_vec());
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
fn send_server_settings_via_channel(
    ctrl_tx: &mpsc::UnboundedSender<Vec<u8>>,
    buffer_ms: u32,
    volume: u8,
    muted: bool,
    latency_ms: i32,
    eq_bands: Vec<sonium_protocol::messages::EqBand>,
    eq_enabled: bool,
    observability_enabled: bool,
    transport_mode: String,
    server_udp_port: u16,
    output_prefill_ms: u32,
) {
    let settings = ServerSettings {
        buffer_ms: buffer_ms as i32,
        output_prefill_ms,
        latency: latency_ms,
        volume,
        muted,
        eq_bands,
        eq_enabled,
        observability_enabled,
        transport_mode,
        server_udp_port,
    };
    let mut hdr = MessageHeader::new(MessageType::ServerSettings, 0);
    hdr.id = next_id();
    let _ = ctrl_tx.send(Message::ServerSettings(settings).encode_with_header(hdr));
}

struct ClientMsgContext<'a> {
    ctrl_tx: &'a mpsc::UnboundedSender<Vec<u8>>,
    state: &'a ServerState,
    client_id: &'a str,
    group_id: &'a str,
    current_buffer_ms: &'a mut u32,
    auto_buffer_tuner: &'a mut AutoBufferTuner,
    health_tracker: &'a mut HealthTransitionTracker,
    transport_mode: String,
    server_udp_port: u16,
    output_prefill_ms: u32,
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
            // c2s_us = server_now - client_sent, so it includes clock skew between
            // the two machines.  Only warn at a threshold high enough to indicate a
            // genuine stall (>2 s), not normal clock-offset noise (~300 ms).
            let c2s_us = (diff.sec as i64) * 1_000_000 + diff.usec as i64;
            let c2s_ms = c2s_us / 1000;
            if c2s_ms > 2_000 {
                warn!(client_id = %ctx.client_id, c2s_ms, "Time sync stall — client may be hung or network severely congested");
            } else {
                tracing::debug!(client_id = %ctx.client_id, c2s_ms, "Time sync");
            }
            let mut reply = MessageHeader::new(MessageType::Time, 8);
            reply.id = next_id();
            reply.refers_to = hdr.id;
            reply.received = now;
            let _ = ctx
                .ctrl_tx
                .send(Message::Time(TimeMsg { latency: diff }).encode_with_header(reply));
        }
        MessageType::ClientInfo => {
            if let Ok(Message::ClientInfo(ci)) = Message::from_payload(&hdr, payload) {
                ctx.state.set_volume(ctx.client_id, ci.volume, ci.muted);
            }
        }
        MessageType::HealthReport => {
            if let Ok(Message::HealthReport(health)) = Message::from_payload(&hdr, payload) {
                // Always store the clock offset for group-sync calculations,
                // regardless of observability setting.
                ctx.state
                    .set_client_clock_offset(ctx.client_id, health.latency_ms);

                let health_state = ctx.health_tracker.observe(
                    ctx.client_id,
                    &ctx.transport_mode,
                    &health,
                    *ctx.current_buffer_ms,
                );
                metrics::observe_client_health(
                    ctx.client_id,
                    &ctx.transport_mode,
                    &health,
                    health_state,
                );

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
                    send_server_settings_via_channel(
                        ctx.ctrl_tx,
                        next_buffer_ms,
                        volume,
                        muted,
                        latency_ms,
                        eq_bands,
                        eq_enabled,
                        obs,
                        ctx.transport_mode.clone(),
                        ctx.server_udp_port,
                        ctx.output_prefill_ms,
                    );
                    *ctx.current_buffer_ms = next_buffer_ms;
                    debug!(client_id = %ctx.client_id, buffer_ms = next_buffer_ms, "Auto buffer adjusted");
                }
                // Always store the latest health snapshot so the sync page and
                // group-sync calculations have up-to-date data.
                ctx.state.set_client_health(ctx.client_id, health);
            }
        }
        MessageType::AnnouncementControl => {
            let control = AnnouncementControlV1::decode(payload)?;
            handle_announcement_ack(
                ctx.state,
                ctx.group_id,
                &control,
                sonium_sync::time_provider::now_us() / 1_000,
            )?;
        }
        other => debug!("Ignoring message: {other:?}"),
    }
    Ok(())
}

fn queue_announcement_control(
    ctrl_tx: &mpsc::UnboundedSender<Vec<u8>>,
    session_group_id: &str,
    control: &AnnouncementControlV1,
) -> bool {
    if control.group_id != session_group_id {
        return false;
    }
    let mut header = MessageHeader::new(MessageType::AnnouncementControl, 0);
    header.id = next_id();
    ctrl_tx
        .send(Message::AnnouncementControl(control.clone()).encode_with_header(header))
        .is_ok()
}

fn handle_announcement_ack(
    state: &ServerState,
    session_group_id: &str,
    control: &AnnouncementControlV1,
    now_ms: i64,
) -> anyhow::Result<()> {
    if control.group_id != session_group_id || control.intent.is_some() {
        return Err(anyhow::anyhow!(
            "announcement acknowledgement does not belong to the client session"
        ));
    }
    let lifecycle = match control.lifecycle {
        sonium_protocol::messages::AnnouncementLifecycle::Scheduled => {
            sonium_control::announcements::AnnouncementLifecycle::Scheduled
        }
        sonium_protocol::messages::AnnouncementLifecycle::Started => {
            sonium_control::announcements::AnnouncementLifecycle::Started
        }
        sonium_protocol::messages::AnnouncementLifecycle::Completed => {
            sonium_control::announcements::AnnouncementLifecycle::Completed
        }
        sonium_protocol::messages::AnnouncementLifecycle::Cancelled => {
            sonium_control::announcements::AnnouncementLifecycle::Cancelled
        }
        sonium_protocol::messages::AnnouncementLifecycle::Failed => {
            sonium_control::announcements::AnnouncementLifecycle::Failed
        }
    };
    state
        .acknowledge_announcement_at(
            &control.announcement_id,
            session_group_id,
            lifecycle,
            now_ms,
        )
        .map_err(Into::into)
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

#[cfg(test)]
mod tests {
    use super::*;
    use sonium_control::announcements::{
        AnnouncementIntent, AnnouncementLifecycle as ControlLifecycle, AnnouncementPriority,
        AnnouncementSource, Ducking, ResumePolicy, ANNOUNCEMENT_INTENT_VERSION,
    };
    use sonium_protocol::messages::HealthReport;
    use sonium_protocol::messages::{AnnouncementControlV1, AnnouncementLifecycle};
    use std::sync::Barrier;
    use std::thread;
    use tokio::sync::Semaphore;

    const BUF_MS: u32 = 1200;

    fn announcement_intent() -> AnnouncementIntent {
        AnnouncementIntent {
            version: ANNOUNCEMENT_INTENT_VERSION,
            idempotency_key: "session-route".into(),
            target_groups: vec!["default".into()],
            priority: AnnouncementPriority::Announcement,
            source: AnnouncementSource::Uri("https://media.example.test/a.ogg".into()),
            duck: Ducking {
                attenuation_db: -18.0,
                attack_ms: 25,
                release_ms: 100,
            },
            max_duration_ms: 1_000,
            expires_at_ms: 20_000,
            resume: ResumePolicy::ResumePrevious,
        }
    }

    #[test]
    fn announcement_controls_are_routed_only_to_the_target_group() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let control = AnnouncementControlV1 {
            version: 1,
            announcement_id: "doorbell".into(),
            group_id: "default".into(),
            lifecycle: AnnouncementLifecycle::Cancelled,
            scheduled_at_ms: 10_250,
            max_duration_ms: 1_000,
            intent: None,
        };

        assert!(!queue_announcement_control(&tx, "bedroom", &control));
        assert!(rx.try_recv().is_err());
        assert!(queue_announcement_control(&tx, "default", &control));

        let bytes = rx.try_recv().unwrap();
        let header = MessageHeader::from_bytes(&bytes[..HEADER_SIZE]).unwrap();
        let decoded = Message::from_payload(&header, &bytes[HEADER_SIZE..]).unwrap();
        assert!(matches!(
            decoded,
            Message::AnnouncementControl(message) if message == control
        ));
    }

    #[test]
    fn announcement_ack_cannot_mutate_a_different_session_group() {
        let state = ServerState::new(
            Arc::new(sonium_control::EventBus::new()),
            None,
            vec![],
            vec![],
        );
        let admitted = state
            .admit_announcement(announcement_intent(), 10_000)
            .unwrap();
        let mut controls = state.subscribe_announcement_controls();
        // The receiver subscribed after admission in this unit fixture; use the
        // timestamp assigned by the deterministic default scheduler.
        let mut ack = AnnouncementControlV1 {
            version: 1,
            announcement_id: admitted.id.clone(),
            group_id: "default".into(),
            lifecycle: AnnouncementLifecycle::Scheduled,
            scheduled_at_ms: 10_250,
            max_duration_ms: 1_000,
            intent: None,
        };
        assert!(handle_announcement_ack(&state, "bedroom", &ack, 10_010).is_err());
        assert_eq!(
            state.all_announcements()[0].lifecycle,
            ControlLifecycle::Scheduled
        );

        handle_announcement_ack(&state, "default", &ack, 10_010).unwrap();
        ack.lifecycle = AnnouncementLifecycle::Started;
        handle_announcement_ack(&state, "default", &ack, 10_250).unwrap();
        assert_eq!(
            state.all_announcements()[0].lifecycle,
            ControlLifecycle::Started
        );
        assert!(controls.try_recv().is_err());
    }

    fn stable_report() -> HealthReport {
        HealthReport::new(0, 0, 0, 800, 0, 0)
    }

    fn underrun_report(prev: &HealthReport) -> HealthReport {
        HealthReport::new(prev.underrun_count + 1, 0, 0, 400, 0, 0)
    }

    fn stale_report(prev: &HealthReport) -> HealthReport {
        HealthReport::new(0, 0, prev.stale_drop_count + 50, 900, 10, 0)
    }

    fn feed(tracker: &mut HealthTransitionTracker, report: &HealthReport) -> AudioHealthState {
        tracker.observe("test-client", "tcp", report, BUF_MS)
    }

    #[test]
    fn client_id_validation_rejects_unsafe_or_oversized_ids() {
        assert!(validate_client_id("bedroom-pi-1").is_ok());

        for invalid in [
            "",
            "living room",
            "living/room",
            "living.room",
            &"a".repeat(97),
        ] {
            assert!(
                validate_client_id(invalid).is_err(),
                "{invalid:?} must not become a state or metric label"
            );
        }
    }

    #[test]
    fn global_session_capacity_releases_after_disconnect_or_error() {
        let capacity = Arc::new(Semaphore::new(1));
        let first =
            ClientSessionPermit::try_acquire(capacity.clone()).expect("first session admitted");

        assert!(
            ClientSessionPermit::try_acquire(capacity.clone()).is_err(),
            "a session over the configured global cap must be rejected"
        );

        drop(first);
        assert!(
            ClientSessionPermit::try_acquire(capacity).is_ok(),
            "dropping a session permit must release capacity for the next client"
        );
    }

    #[test]
    fn stale_disconnect_cleanup_after_reconnect_preserves_new_metric_labels() {
        const ID: &str = "session-generation-race";
        let state = Arc::new(ServerState::new(
            Arc::new(sonium_control::EventBus::new()),
            None,
            vec![],
            vec![],
        ));
        let addr = "127.0.0.1:50000".parse().unwrap();
        let old_generation = state
            .try_client_connected(ID, "old", "Sonium", "linux", "aarch64", addr, 2)
            .expect("old session admitted");
        let report = HealthReport::new(0, 0, 0, 100, 0, 0);
        metrics::register_client_session(ID, old_generation);
        metrics::observe_client_health(ID, "tcp", &report, AudioHealthState::Stable);

        let cleanup_entered = Arc::new(Barrier::new(2));
        let allow_cleanup = Arc::new(Barrier::new(2));
        let hook_entered = cleanup_entered.clone();
        let hook_release = allow_cleanup.clone();
        state.set_client_removal_hook(Arc::new(move |client_id, generation| {
            hook_entered.wait();
            hook_release.wait();
            metrics::forget_client(client_id, generation);
        }));

        let stale_state = state.clone();
        let stale_disconnect = thread::spawn(move || {
            assert!(stale_state.client_disconnected_generation(ID, old_generation));
        });
        cleanup_entered.wait();

        let new_generation = state
            .try_client_connected(ID, "new", "Sonium", "linux", "aarch64", addr, 2)
            .expect("reconnect admitted while old cleanup waits");
        metrics::register_client_session(ID, new_generation);
        metrics::observe_client_health(ID, "tcp", &report, AudioHealthState::Stable);

        allow_cleanup.wait();
        stale_disconnect.join().expect("stale disconnect thread");

        assert!(metrics::gather().contains(&format!("client_id=\"{ID}\"")));
        metrics::forget_client(ID, new_generation);
    }

    #[test]
    fn first_report_stable_is_stable() {
        let mut t = HealthTransitionTracker::default();
        assert_eq!(feed(&mut t, &stable_report()), AudioHealthState::Stable);
    }

    #[test]
    fn underrun_transitions_to_underrun() {
        let mut t = HealthTransitionTracker::default();
        let r0 = stable_report();
        feed(&mut t, &r0);
        let r1 = underrun_report(&r0);
        assert_eq!(feed(&mut t, &r1), AudioHealthState::Underrun);
    }

    #[test]
    fn single_clean_interval_after_underrun_is_recovering_not_stable() {
        let mut t = HealthTransitionTracker::default();
        let r0 = stable_report();
        feed(&mut t, &r0);
        let r1 = underrun_report(&r0);
        feed(&mut t, &r1);
        // One clean interval — must be Recovering, not Stable yet.
        assert_eq!(feed(&mut t, &r1), AudioHealthState::Recovering);
    }

    #[test]
    fn recovering_promotes_to_stable_only_after_full_streak() {
        let mut t = HealthTransitionTracker::default();
        let r0 = stable_report();
        feed(&mut t, &r0);
        let r_bad = underrun_report(&r0);
        feed(&mut t, &r_bad);

        // Feed STABLE_STREAK_REQUIRED - 1 clean intervals; should stay Recovering.
        for _ in 0..STABLE_STREAK_REQUIRED - 1 {
            let s = feed(&mut t, &r_bad);
            assert_eq!(s, AudioHealthState::Recovering);
        }

        // The STABLE_STREAK_REQUIRED-th clean interval promotes to Stable.
        assert_eq!(feed(&mut t, &r_bad), AudioHealthState::Stable);
    }

    #[test]
    fn degradation_during_recovery_resets_streak() {
        let mut t = HealthTransitionTracker::default();
        let r0 = stable_report();
        feed(&mut t, &r0);
        let r_bad = underrun_report(&r0);
        feed(&mut t, &r_bad);

        // Two clean intervals into recovery…
        feed(&mut t, &r_bad);
        feed(&mut t, &r_bad);

        // …then a new stale burst — resets streak, back to Degraded.
        let r_stale = stale_report(&r_bad);
        assert_eq!(feed(&mut t, &r_stale), AudioHealthState::Degraded);

        // Next clean interval restarts recovery (streak = 1).
        assert_eq!(feed(&mut t, &r_stale), AudioHealthState::Recovering);
    }

    #[test]
    fn stable_without_prior_bad_state_does_not_need_streak() {
        let mut t = HealthTransitionTracker::default();
        let r = stable_report();
        feed(&mut t, &r);
        // Multiple stable intervals with no prior degradation stay Stable immediately.
        assert_eq!(feed(&mut t, &r), AudioHealthState::Stable);
        assert_eq!(feed(&mut t, &r), AudioHealthState::Stable);
    }

    #[test]
    fn degraded_after_stale_drops() {
        let mut t = HealthTransitionTracker::default();
        let r0 = stable_report();
        feed(&mut t, &r0);
        let r1 = stale_report(&r0);
        assert_eq!(feed(&mut t, &r1), AudioHealthState::Degraded);
    }

    fn auto_tuner() -> AutoBufferTuner {
        AutoBufferTuner {
            enabled: true,
            min_ms: 1200,
            max_ms: 2400,
            step_up_ms: 200,
            step_down_ms: 50,
            cooldown: Duration::from_secs(4),
            last_adjust: Instant::now() - Duration::from_secs(10),
            last_underrun: 0,
            last_stale: 0,
            last_rtp_gaps: 0,
            last_rtp_concealed: 0,
            last_rtp_decode_errors: 0,
            clean_intervals: 0,
        }
    }

    fn rtp_report(stale_drops: u32, gaps: u32, concealed: u32, jitter_ms: u32) -> HealthReport {
        HealthReport::new(0, 0, stale_drops, 1000, jitter_ms, 0)
            .with_rtp_metrics(10_000, gaps, 0, concealed)
    }

    #[test]
    fn auto_buffer_steps_up_on_stale_delta() {
        let mut t = auto_tuner();
        let report = rtp_report(1, 0, 0, 0);

        assert_eq!(t.on_health(&report, 1200), Some(1400));
    }

    #[test]
    fn auto_buffer_uses_larger_step_for_rtp_burst() {
        let mut t = auto_tuner();
        let report = rtp_report(0, AUTO_BUFFER_RTP_BURST_THRESHOLD + 1, 0, 0);

        assert_eq!(t.on_health(&report, 1200), Some(1600));
    }

    #[test]
    fn auto_buffer_step_up_saturates_at_the_configured_maximum() {
        let mut t = auto_tuner();
        t.max_ms = u32::MAX;
        t.step_up_ms = u32::MAX;
        let report = rtp_report(1, 0, 0, 0);

        assert_eq!(t.on_health(&report, u32::MAX - 1), Some(u32::MAX));
    }

    #[test]
    fn auto_buffer_does_not_step_down_on_recent_rtp_activity() {
        let mut t = auto_tuner();
        t.clean_intervals = AUTO_BUFFER_CLEAN_INTERVALS_BEFORE_STEP_DOWN;
        let report = rtp_report(0, 1, 1, 0);

        assert_eq!(t.on_health(&report, 1600), None);
        assert_eq!(t.clean_intervals, 0);
    }

    #[test]
    fn auto_buffer_steps_down_only_after_sustained_clean_reports() {
        let mut t = auto_tuner();
        let clean = rtp_report(0, 0, 0, 0);

        for _ in 0..AUTO_BUFFER_CLEAN_INTERVALS_BEFORE_STEP_DOWN - 1 {
            t.last_adjust = Instant::now() - Duration::from_secs(10);
            assert_eq!(t.on_health(&clean, 1600), None);
        }

        t.last_adjust = Instant::now() - Duration::from_secs(10);
        assert_eq!(t.on_health(&clean, 1600), Some(1550));
    }
}
