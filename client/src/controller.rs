use socket2::{SockRef, TcpKeepalive};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc as tokio_mpsc;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use sonium_transport::{RtpPacket, RTP_CLOCK_RATE};

use sonium_common::config::ClientConfig;
use sonium_protocol::{
    header::{validate_payload_size, HEADER_SIZE},
    messages::{EqBand, HealthReport, Hello, Message, TimeMsg},
    MessageHeader, MessageType,
};
use sonium_sync::time_provider::now_us;
use sonium_sync::{PcmChunk, SyncBuffer, TimeProvider};

use crate::decoder::ActiveDecoder;
use crate::eq::SmoothedEqProcessor;
use crate::player::Player;

use tokio::sync::mpsc;

enum IncomingFrame {
    Message(MessageHeader, Vec<u8>),
    Closed(String),
}

enum UdpMediaEvent {
    Packet {
        sequence: u16,
        timestamp: u32,
        payload: Vec<u8>,
    },
    DecodeError,
}

const MAX_CONCEALMENT_PACKETS_PER_GAP: u16 = 10;

const READ_TIMEOUT: Duration = Duration::from_secs(20);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, serde::Serialize)]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Ready,
    Disconnected,
    Error(String),
}

/// Main client loop — connects, syncs clock, decodes and plays audio.
/// Auto-reconnects on disconnect with exponential backoff.
pub async fn run(
    server_addr: String,
    cfg: ClientConfig,
    health_tx: Option<mpsc::UnboundedSender<HealthReport>>,
) -> anyhow::Result<()> {
    run_with_status(server_addr, cfg, health_tx, None).await
}

pub async fn run_with_status(
    server_addr: String,
    cfg: ClientConfig,
    health_tx: Option<mpsc::UnboundedSender<HealthReport>>,
    status_tx: Option<mpsc::UnboundedSender<ConnectionStatus>>,
) -> anyhow::Result<()> {
    let mut backoff = Duration::from_millis(500);

    loop {
        let _ = status_tx
            .as_ref()
            .map(|tx| tx.send(ConnectionStatus::Connecting));

        match connect_and_run(
            &server_addr,
            &cfg,
            health_tx.clone(),
            status_tx.clone(),
            &mut backoff,
        )
        .await
        {
            Ok(()) => {
                let _ = status_tx
                    .as_ref()
                    .map(|tx| tx.send(ConnectionStatus::Disconnected));
                info!("Disconnected cleanly");
                backoff = Duration::from_millis(500);
            }
            Err(e) => {
                let _ = status_tx
                    .as_ref()
                    .map(|tx| tx.send(ConnectionStatus::Error(e.to_string())));
                warn!(
                    "Disconnected with error: {e} — reconnecting in {}ms",
                    backoff.as_millis()
                );
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn connect_and_run(
    addr: &str,
    cfg: &ClientConfig,
    health_tx: Option<mpsc::UnboundedSender<HealthReport>>,
    status_tx: Option<mpsc::UnboundedSender<ConnectionStatus>>,
    backoff: &mut Duration,
) -> anyhow::Result<()> {
    let stream = TcpStream::connect(addr).await?;
    stream.set_nodelay(true)?;
    configure_tcp_stream(&stream);
    *backoff = Duration::from_millis(500);
    let (reader, mut writer) = stream.into_split();
    let _ = status_tx
        .as_ref()
        .map(|tx| tx.send(ConnectionStatus::Connected));
    info!(%addr, "Connected to server (TCP_NODELAY=true)");

    let time_provider = TimeProvider::new();

    // Bind a local UDP socket for RTP media reception.
    // Port 0 lets the OS assign an ephemeral port; we advertise it in Hello.
    let udp_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    let udp_port = udp_socket.local_addr()?.port();
    debug!(udp_port, "UDP media socket bound");

    // 1. Send Hello
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "sonium-client".into());
    let display_name = cfg.client_name.as_deref().unwrap_or(&hostname);
    let client_id = format!("{}-{}", hostname, cfg.instance);

    let mut hello_msg = Hello::new(display_name, &client_id);
    hello_msg.hostname = display_name.to_owned();
    hello_msg.udp_port = udp_port;
    let hello = Message::Hello(hello_msg);
    write_all_with_timeout(&mut writer, &hello.encode()).await?;
    info!(udp_port, "Hello sent");

    // 2. Wait for CodecHeader, then ServerSettings
    let mut decoder: Option<ActiveDecoder> = None;
    let mut player: Option<Player> = None;
    let mut sync_buf: Option<SyncBuffer> = None;
    let mut volume: u8 = 100;
    let mut muted = false;
    let mut eq_bands: Vec<EqBand> = vec![];
    let mut eq_enabled = false;
    let mut eq_processor: Option<SmoothedEqProcessor> = None;
    let mut server_buffer_ms: i32 = cfg.latency_ms + 500; // Default buffer depth
    let mut server_latency_ms: i32 = 0;

    let mut pending_time: Option<(u16, i64)> = None; // (msg_id, sent_us)

    // Channel for WireChunk payload bytes received via UDP RTP path.
    let mut udp_chunk_rx: Option<tokio_mpsc::UnboundedReceiver<UdpMediaEvent>> = None;
    let mut udp_recv_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut rtp_packets_received = 0u32;
    let mut rtp_sequence_gaps = 0u32;
    let mut rtp_decode_error_count = 0u32;
    let mut rtp_concealed_packets = 0u32;
    let mut last_rtp_sequence: Option<u16> = None;
    let mut last_rtp_timestamp: Option<u32> = None;

    // Start periodic tasks
    let (incoming_tx, mut incoming_rx) = mpsc::unbounded_channel();
    let read_task = tokio::spawn(socket_reader(reader, incoming_tx));
    let mut audio_tick = tokio::time::interval(tokio::time::Duration::from_millis(5));
    let mut sync_tick = tokio::time::interval(tokio::time::Duration::from_secs(1));
    let mut health_tick = tokio::time::interval(tokio::time::Duration::from_secs(2));
    let mut sync_seq: u16 = 0;
    send_time_request(&mut writer, &mut sync_seq, &mut pending_time).await?;

    let result = loop {
        tokio::select! {
            // Audio pump: ensure SyncBuffer is drained even if network is quiet
            _ = audio_tick.tick() => {
                if time_provider.sample_count() == 0 {
                    continue;
                }
                if let (Some(pl), Some(buf)) = (player.as_mut(), sync_buf.as_mut()) {
                    let now_server = time_provider.to_server_time(now_us());
                    let target_output_us = output_prefill_us(
                        server_buffer_ms + cfg.latency_ms + server_latency_ms,
                    );
                    while pl.buffered_us() < target_output_us {
                        let sink_ready_at = now_server + pl.buffered_us();
                        let Some(chunk) = buf.pop_ready(sink_ready_at) else {
                            break;
                        };
                        if let Err(e) = pl.write(&chunk.samples) {
                            warn!("Audio pump write error: {e}");
                            break;
                        }
                    }
                }
            }

            // Sync clock with server
            _ = sync_tick.tick() => {
                if let Err(e) = send_time_request(&mut writer, &mut sync_seq, &mut pending_time).await {
                    warn!("Failed to send sync request: {e}");
                    break Ok(());
                }
            }

            // Health report
            _ = health_tick.tick() => {
                let report_msg = if let Some(buf) = sync_buf.as_mut() {
                    let now_server = time_provider.to_server_time(now_us());
                    let mut report = buf.get_report(now_server);
                    let player_health = player.as_ref().map(|p| p.take_health()).unwrap_or_default();
                    report.underrun_count += player_health.underrun_count;

                    let jitter = (buf.jitter_us() / 1000) as u32;
                    let output_buffer_ms = player
                        .as_ref()
                        .map(|p| (p.buffered_us().max(0) / 1000) as u32)
                        .unwrap_or(0);
                    let target_playout_latency_ms =
                        (server_buffer_ms + cfg.latency_ms + server_latency_ms).max(0) as u32;
                    sonium_protocol::messages::HealthReport::new(
                        report.underrun_count,
                        player_health.overrun_count,
                        report.stale_drop_count,
                        report.buffer_depth_ms as u32,
                        jitter,
                        (time_provider.offset_us() / 1000) as i32,
                    )
                    .with_queue_metrics(output_buffer_ms, buf.len() as u32, target_playout_latency_ms)
                    .with_callback_metrics(
                        player_health.callback_starvation_count,
                        player_health.audio_callback_xrun_count,
                    )
                    .with_rtp_metrics(
                        rtp_packets_received,
                        rtp_sequence_gaps,
                        rtp_decode_error_count,
                        rtp_concealed_packets,
                    )
                } else {
                    // Send idle report to keep status "Connected"
                    sonium_protocol::messages::HealthReport::new(
                        0, 0, 0, 0, 0,
                        (time_provider.offset_us() / 1000) as i32,
                    )
                };

                if let Some(tx) = health_tx.as_ref() {
                    let _ = tx.send(report_msg.clone());
                }

                let msg = Message::HealthReport(report_msg).encode();
                if let Err(e) = write_all_with_timeout(&mut writer, &msg).await {
                    warn!("Failed to send health report: {e}");
                    break Ok(());
                }
            }

            // Read next message from server
            incoming = incoming_rx.recv() => {
                let Some(incoming) = incoming else {
                    warn!("Connection reader stopped");
                    break Ok(());
                };
                let (hdr, payload) = match incoming {
                    IncomingFrame::Message(hdr, payload) => (hdr, payload),
                    IncomingFrame::Closed(reason) => {
                        warn!("Connection closed or read error: {reason}");
                        break Ok(());
                    }
                };

                match hdr.msg_type {
                    MessageType::CodecHeader => {
                        let ch = sonium_protocol::messages::CodecHeader::decode(&payload)?;
                        info!(codec = %ch.codec, "CodecHeader received");
                        let dec = ActiveDecoder::from_codec(&ch.codec, &ch.header_data)?;
                        let fmt = dec.sample_format();
                        let p   = Player::new(fmt, cfg.device.as_deref())?;
                        let buf = SyncBuffer::new(fmt);
                        eq_processor = Some(SmoothedEqProcessor::new(eq_enabled, &eq_bands, fmt.rate, fmt.channels as usize));
                        decoder  = Some(dec);
                        player   = Some(p);
                        sync_buf = Some(buf);
                        let _ = status_tx
                            .as_ref()
                            .map(|tx| tx.send(ConnectionStatus::Ready));
                    }

                    MessageType::ServerSettings => {
                        let ss = sonium_protocol::messages::ServerSettings::decode(&payload)?;
                        volume   = ss.volume.min(100);
                        muted    = ss.muted;
                        eq_bands = ss.eq_bands;
                        eq_enabled = ss.eq_enabled;
                        server_buffer_ms = ss.buffer_ms;
                        server_latency_ms = ss.latency;
                        if let Some(buf) = sync_buf.as_mut() {
                            buf.set_target_buffer_ms(server_buffer_ms + cfg.latency_ms + server_latency_ms);
                        }
                        if let Some(pl) = player.as_mut() {
                            pl.set_buffer_limit_ms((server_buffer_ms + cfg.latency_ms + server_latency_ms).max(80));
                        }
                        if let Some(dec) = decoder.as_ref() {
                            let fmt = dec.sample_format();
                            if let Some(eq) = eq_processor.as_mut() {
                                eq.set_config(eq_enabled, &eq_bands);
                            } else {
                                eq_processor = Some(SmoothedEqProcessor::new(
                                    eq_enabled,
                                    &eq_bands,
                                    fmt.rate,
                                    fmt.channels as usize,
                                ));
                            }
                        }
                        debug!(volume = ss.volume, muted = ss.muted, buffer_ms = ss.buffer_ms, latency_ms = ss.latency, "ServerSettings applied");

                        if ss.transport_mode == "rtp_udp" && udp_chunk_rx.is_none() {
                            let (udp_tx, udp_rx) = tokio_mpsc::unbounded_channel::<UdpMediaEvent>();
                            udp_chunk_rx = Some(udp_rx);
                            let sock = udp_socket.clone();
                            udp_recv_task = Some(tokio::spawn(async move {
                                let mut buf = vec![0u8; 65_535];
                                loop {
                                    match sock.recv(&mut buf).await {
                                        Ok(n) => {
                                            match RtpPacket::decode(&buf[..n]) {
                                                Ok(pkt) => {
                                                    if udp_tx
                                                        .send(UdpMediaEvent::Packet {
                                                            sequence: pkt.sequence,
                                                            timestamp: pkt.timestamp,
                                                            payload: pkt.payload,
                                                        })
                                                        .is_err()
                                                    {
                                                        break;
                                                    }
                                                }
                                                Err(e) => {
                                                    debug!("RTP decode error: {e}");
                                                    if udp_tx.send(UdpMediaEvent::DecodeError).is_err() {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            warn!("UDP receiver error: {e}");
                                            break;
                                        }
                                    }
                                }
                            }));
                            info!(transport = "rtp_udp", "UDP media receiver started");
                        } else if ss.transport_mode != "rtp_udp" && udp_chunk_rx.is_some() {
                            udp_chunk_rx = None;
                            if let Some(task) = udp_recv_task.take() {
                                task.abort();
                            }
                            last_rtp_sequence = None;
                            last_rtp_timestamp = None;
                            info!(transport = %ss.transport_mode, "UDP media receiver stopped");
                        }
                    }

                    MessageType::WireChunk => {
                        let chunk = sonium_protocol::messages::WireChunk::decode(&payload)?;
                        if let (Some(dec), Some(_pl), Some(buf)) =
                            (decoder.as_mut(), player.as_mut(), sync_buf.as_mut())
                        {
                            let mut samples = Vec::new();
                            dec.decode(&chunk.data, &mut samples)?;
                            apply_volume(&mut samples, volume, muted);
                            if let Some(ref mut eq) = eq_processor {
                                eq.apply(&mut samples);
                            }

                            // Calculate absolute playout time in server clock
                            let playout_us = chunk.timestamp.to_micros()
                                + (server_buffer_ms as i64 * 1000)
                                + (cfg.latency_ms as i64 * 1000)
                                + (server_latency_ms as i64 * 1000);

                            let now_server = time_provider.to_server_time(now_us());
                            buf.push(PcmChunk::new(playout_us, samples, dec.sample_format()), now_server);
                        }
                    }

                    MessageType::Time => {
                        if let Some((expected_id, sent_us)) = pending_time.take() {
                            if hdr.refers_to == expected_id {
                                let recv_us = now_us();
                                let time_msg = TimeMsg::decode(&payload)?;
                                let server_lat_us = time_msg.latency.to_micros();
                                time_provider.update(sent_us, recv_us, server_lat_us);
                                debug!(
                                    offset_ms = time_provider.offset_us() / 1000,
                                    "Clock sync updated"
                                );
                            }
                        }
                    }

                    other => debug!("Unhandled message type: {other:?}"),
                }
            }

            // RTP/UDP media path: WireChunk payloads received from UDP socket
            udp_event = recv_optional_udp(&mut udp_chunk_rx) => {
                match udp_event {
                    Some(UdpMediaEvent::Packet { sequence, timestamp, payload }) => {
                        rtp_packets_received = rtp_packets_received.saturating_add(1);
                        let mut skipped = 0u16;
                        if let Some(last) = last_rtp_sequence {
                            let diff = sequence.wrapping_sub(last);
                            if diff == 0 {
                                continue;
                            }
                            if diff >= 0x8000 {
                                debug!(
                                    last_sequence = last,
                                    sequence,
                                    "Dropping late or out-of-order RTP packet"
                                );
                                continue;
                            }
                            skipped = diff.saturating_sub(1);
                            if skipped > 0 {
                                rtp_sequence_gaps =
                                    rtp_sequence_gaps.saturating_add(skipped as u32);
                            }
                        }

                        let chunk = sonium_protocol::messages::WireChunk::decode(&payload)?;
                        if let (Some(dec), Some(_pl), Some(buf)) =
                            (decoder.as_mut(), player.as_mut(), sync_buf.as_mut())
                        {
                            if skipped > 0 {
                                let conceal_count = skipped.min(MAX_CONCEALMENT_PACKETS_PER_GAP);
                                let interval_us = last_rtp_timestamp
                                    .and_then(|last_timestamp| {
                                        let timestamp_delta = timestamp.wrapping_sub(last_timestamp);
                                        let packets = u32::from(skipped) + 1;
                                        if packets == 0 {
                                            None
                                        } else {
                                            Some(
                                                ((timestamp_delta as u64)
                                                    .saturating_mul(1_000_000)
                                                    / RTP_CLOCK_RATE
                                                    / packets as u64)
                                                    .clamp(10_000, 60_000)
                                                    as i64,
                                            )
                                        }
                                    })
                                    .unwrap_or(20_000);
                                let current_playout_us = chunk.timestamp.to_micros()
                                    + (server_buffer_ms as i64 * 1000)
                                    + (cfg.latency_ms as i64 * 1000)
                                    + (server_latency_ms as i64 * 1000);
                                let first_missing_back = i64::from(conceal_count);
                                // Snapshot server time once for the whole concealment burst;
                                // also used as the arrival timestamp in SyncBuffer::push.
                                let now_server = time_provider.to_server_time(now_us());
                                // Mirror the stale-drop threshold from SyncBuffer::pop_ready so
                                // we never insert a frame that would be discarded immediately.
                                let stale_threshold_us =
                                    (buf.target_buffer_us() / 2).clamp(100_000, 2_000_000);
                                for i in 0..conceal_count {
                                    let playout_us = current_playout_us
                                        - (first_missing_back - i64::from(i)) * interval_us;
                                    // Always call decode_missing to advance the Opus decoder's
                                    // internal PLC state, even when we will not queue the frame.
                                    let mut samples = Vec::new();
                                    dec.decode_missing((interval_us / 1000) as u32, &mut samples)?;
                                    // Drop frames whose playout window has already passed.
                                    if playout_us + interval_us < now_server - stale_threshold_us {
                                        continue;
                                    }
                                    apply_volume(&mut samples, volume, muted);
                                    if let Some(ref mut eq) = eq_processor {
                                        eq.apply(&mut samples);
                                    }
                                    buf.push(
                                        PcmChunk::new(playout_us, samples, dec.sample_format()),
                                        now_server,
                                    );
                                    rtp_concealed_packets =
                                        rtp_concealed_packets.saturating_add(1);
                                }
                            }

                            let mut samples = Vec::new();
                            dec.decode(&chunk.data, &mut samples)?;
                            apply_volume(&mut samples, volume, muted);
                            if let Some(ref mut eq) = eq_processor {
                                eq.apply(&mut samples);
                            }
                            let playout_us = chunk.timestamp.to_micros()
                                + (server_buffer_ms as i64 * 1000)
                                + (cfg.latency_ms as i64 * 1000)
                                + (server_latency_ms as i64 * 1000);
                            let now_server = time_provider.to_server_time(now_us());
                            buf.push(PcmChunk::new(playout_us, samples, dec.sample_format()), now_server);
                        }
                        last_rtp_sequence = Some(sequence);
                        last_rtp_timestamp = Some(timestamp);
                    }
                    Some(UdpMediaEvent::DecodeError) => {
                        rtp_decode_error_count = rtp_decode_error_count.saturating_add(1);
                    }
                    None => {}
                }
            }
        }
    };

    read_task.abort();
    if let Some(task) = udp_recv_task {
        task.abort();
    }
    result
}

fn output_prefill_us(total_buffer_ms: i32) -> i64 {
    let ms = (total_buffer_ms / 4).clamp(120, 300);
    ms as i64 * 1000
}

fn configure_tcp_stream(stream: &TcpStream) {
    let sock = SockRef::from(stream);
    if let Err(e) = sock.set_keepalive(true) {
        warn!("Could not enable TCP keepalive: {e}");
    }

    // Expedited Forwarding DSCP (46) shifted into the IPv4 TOS byte. Routers
    // may ignore it, but honoring networks can prioritize latency-sensitive audio.
    if let Err(e) = sock.set_tos_v4(46 << 2) {
        warn!("Could not set TCP DSCP/TOS priority: {e}");
    }

    let keepalive = TcpKeepalive::new()
        .with_time(Duration::from_secs(30))
        .with_interval(Duration::from_secs(10));
    if let Err(e) = sock.set_tcp_keepalive(&keepalive) {
        warn!("Could not configure TCP keepalive: {e}");
    }
}

async fn send_time_request(
    stream: &mut OwnedWriteHalf,
    sync_seq: &mut u16,
    pending_time: &mut Option<(u16, i64)>,
) -> anyhow::Result<()> {
    *sync_seq = sync_seq.wrapping_add(1);
    let mut hdr = MessageHeader::new(MessageType::Time, 8);
    hdr.id = *sync_seq;
    let sent_us = hdr.sent.to_micros();
    let msg = Message::Time(TimeMsg::zero()).encode_with_header(hdr);
    write_all_with_timeout(stream, &msg).await?;
    *pending_time = Some((*sync_seq, sent_us));
    Ok(())
}

async fn read_exact_with_timeout(reader: &mut OwnedReadHalf, buf: &mut [u8]) -> anyhow::Result<()> {
    match timeout(READ_TIMEOUT, reader.read_exact(buf)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => Err(anyhow::anyhow!("read timed out after {:?}", READ_TIMEOUT)),
    }
}

async fn write_all_with_timeout(writer: &mut OwnedWriteHalf, buf: &[u8]) -> anyhow::Result<()> {
    match timeout(WRITE_TIMEOUT, writer.write_all(buf)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => Err(anyhow::anyhow!("write timed out after {:?}", WRITE_TIMEOUT)),
    }
}

async fn socket_reader(mut reader: OwnedReadHalf, tx: mpsc::UnboundedSender<IncomingFrame>) {
    loop {
        let mut hdr_buf = [0u8; HEADER_SIZE];
        if let Err(e) = read_exact_with_timeout(&mut reader, &mut hdr_buf).await {
            let _ = tx.send(IncomingFrame::Closed(e.to_string()));
            break;
        }

        let hdr = match MessageHeader::from_bytes(&hdr_buf) {
            Ok(hdr) => hdr,
            Err(e) => {
                let _ = tx.send(IncomingFrame::Closed(format!("invalid header: {e}")));
                break;
            }
        };

        let payload_size = match validate_payload_size(&hdr) {
            Ok(size) => size,
            Err(e) => {
                let _ = tx.send(IncomingFrame::Closed(e.to_string()));
                break;
            }
        };

        let mut payload = vec![0u8; payload_size];
        if let Err(e) = read_exact_with_timeout(&mut reader, &mut payload).await {
            let _ = tx.send(IncomingFrame::Closed(format!("error reading payload: {e}")));
            break;
        }

        if tx.send(IncomingFrame::Message(hdr, payload)).is_err() {
            break;
        }
    }
}

async fn recv_optional_udp(
    rx: &mut Option<tokio_mpsc::UnboundedReceiver<UdpMediaEvent>>,
) -> Option<UdpMediaEvent> {
    match rx.as_mut() {
        Some(r) => r.recv().await,
        None => std::future::pending().await,
    }
}

fn apply_volume(samples: &mut [i16], volume: u8, muted: bool) {
    if muted {
        samples.fill(0);
        return;
    }

    if volume >= 100 {
        return;
    }

    let gain = volume as f32 / 100.0;
    for sample in samples {
        *sample = (*sample as f32 * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    }
}
