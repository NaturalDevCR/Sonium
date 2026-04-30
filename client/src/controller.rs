use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};

use sonium_common::config::ClientConfig;
use sonium_protocol::{
    header::HEADER_SIZE,
    messages::{EqBand, HealthReport, Hello, Message, TimeMsg},
    MessageHeader, MessageType,
};
use sonium_sync::time_provider::now_us;
use sonium_sync::{PcmChunk, SyncBuffer, TimeProvider};

use crate::decoder::ActiveDecoder;
use crate::eq::SmoothedEqProcessor;
use crate::player::Player;

use tokio::sync::mpsc;

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

        match connect_and_run(&server_addr, &cfg, health_tx.clone(), status_tx.clone()).await {
            Ok(()) => {
                let _ = status_tx
                    .as_ref()
                    .map(|tx| tx.send(ConnectionStatus::Disconnected));
                info!("Disconnected cleanly");
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
) -> anyhow::Result<()> {
    let mut stream = TcpStream::connect(addr).await?;
    stream.set_nodelay(true)?;
    let _ = status_tx
        .as_ref()
        .map(|tx| tx.send(ConnectionStatus::Connected));
    info!(%addr, "Connected to server (TCP_NODELAY=true)");

    let time_provider = TimeProvider::new();

    // 1. Send Hello
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "sonium-client".into());
    let display_name = cfg.client_name.as_deref().unwrap_or(&hostname);
    let client_id = format!("{}-{}", hostname, cfg.instance);

    let mut hello_msg = Hello::new(display_name, &client_id);
    hello_msg.hostname = display_name.to_owned();
    let hello = Message::Hello(hello_msg);
    stream.write_all(&hello.encode()).await?;
    info!("Hello sent");

    // 2. Wait for CodecHeader, then ServerSettings
    let mut decoder: Option<ActiveDecoder> = None;
    let mut player: Option<Player> = None;
    let mut sync_buf: Option<SyncBuffer> = None;
    let mut volume: u8 = 100;
    let mut muted = false;
    let mut eq_bands: Vec<EqBand> = vec![];
    let mut eq_enabled = false;
    let mut eq_processor: Option<SmoothedEqProcessor> = None;
    let mut observability_enabled = false;
    let mut server_buffer_ms: i32 = cfg.latency_ms + 500; // Default buffer depth
    let mut server_latency_ms: i32 = 0;

    let mut hdr_buf = [0u8; HEADER_SIZE];
    let mut pending_time: Option<(u16, i64)> = None; // (msg_id, sent_us)

    // Start periodic tasks
    let mut audio_tick = tokio::time::interval(tokio::time::Duration::from_millis(20));
    let mut sync_tick = tokio::time::interval(tokio::time::Duration::from_secs(1));
    let mut health_tick = tokio::time::interval(tokio::time::Duration::from_secs(2));
    let mut sync_seq: u16 = 0;
    send_time_request(&mut stream, &mut sync_seq, &mut pending_time).await?;

    loop {
        tokio::select! {
            // Audio pump: ensure SyncBuffer is drained even if network is quiet
            _ = audio_tick.tick() => {
                if time_provider.sample_count() == 0 {
                    continue;
                }
                if let (Some(pl), Some(buf)) = (player.as_mut(), sync_buf.as_mut()) {
                    let now_server = time_provider.to_server_time(now_us());
                    while let Some(chunk) = buf.pop_ready(now_server) {
                        if let Err(e) = pl.write(&chunk.samples) {
                            warn!("Audio pump write error: {e}");
                        }
                    }
                }
            }

            // Sync clock with server
            _ = sync_tick.tick() => {
                if let Err(e) = send_time_request(&mut stream, &mut sync_seq, &mut pending_time).await {
                    warn!("Failed to send sync request: {e}");
                    break Ok(());
                }
            }

            // Health report
            _ = health_tick.tick() => {
                let report_msg = if let Some(buf) = sync_buf.as_mut() {
                    let now_server = time_provider.to_server_time(now_us());
                    let mut report = buf.get_report(now_server);
                    let (u, o) = player.as_ref().map(|p| p.take_health()).unwrap_or((0, 0));
                    report.underrun_count += u;

                    let jitter = (buf.jitter_us() / 1000) as u32;
                    sonium_protocol::messages::HealthReport::new(
                        report.underrun_count,
                        o, // overrun_count from player
                        report.stale_drop_count,
                        report.buffer_depth_ms as u32,
                        jitter,
                        (time_provider.offset_us() / 1000) as i32,
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

                if observability_enabled {
                    let msg = Message::HealthReport(report_msg).encode();
                    if let Err(e) = stream.write_all(&msg).await {
                        warn!("Failed to send health report: {e}");
                        break Ok(());
                    }
                }
            }

            // Read next message from server
            read_result = stream.read_exact(&mut hdr_buf) => {
                if let Err(e) = read_result {
                    warn!("Connection closed or read error: {e}");
                    break Ok(());
                }

                let hdr = match MessageHeader::from_bytes(&hdr_buf) {
                    Ok(h) => h,
                    Err(e) => {
                        warn!("Invalid header: {e}");
                        break Ok(());
                    }
                };

                let mut payload = vec![0u8; hdr.payload_size as usize];
                if let Err(e) = stream.read_exact(&mut payload).await {
                    warn!("Error reading payload: {e}");
                    break Ok(());
                }

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
                        observability_enabled = ss.observability_enabled;
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
        }
    }
}

async fn send_time_request(
    stream: &mut TcpStream,
    sync_seq: &mut u16,
    pending_time: &mut Option<(u16, i64)>,
) -> anyhow::Result<()> {
    *sync_seq = sync_seq.wrapping_add(1);
    let mut hdr = MessageHeader::new(MessageType::Time, 8);
    hdr.id = *sync_seq;
    let sent_us = hdr.sent.to_micros();
    let msg = Message::Time(TimeMsg::zero()).encode_with_header(hdr);
    stream.write_all(&msg).await?;
    *pending_time = Some((*sync_seq, sent_us));
    Ok(())
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
