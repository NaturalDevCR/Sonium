use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};

use sonium_common::config::ClientConfig;
use sonium_protocol::{
    MessageHeader, MessageType,
    messages::{Message, Hello, TimeMsg},
    header::HEADER_SIZE,
};
use sonium_sync::{TimeProvider, SyncBuffer, PcmChunk};
use sonium_sync::time_provider::now_us;

use crate::decoder::ActiveDecoder;
use crate::player::Player;

/// Main client loop — connects, syncs clock, decodes and plays audio.
/// Auto-reconnects on disconnect with exponential backoff.
pub async fn run(server_addr: String, cfg: ClientConfig) -> anyhow::Result<()> {
    let mut backoff = Duration::from_millis(500);

    loop {
        match connect_and_run(&server_addr, &cfg).await {
            Ok(()) => {
                info!("Disconnected cleanly");
            }
            Err(e) => {
                warn!("Disconnected with error: {e} — reconnecting in {}ms", backoff.as_millis());
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn connect_and_run(addr: &str, cfg: &ClientConfig) -> anyhow::Result<()> {
    let mut stream = TcpStream::connect(addr).await?;
    info!(%addr, "Connected to server");

    let time_provider = TimeProvider::new();

    // 1. Send Hello
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "sonium-client".into());
    let client_id = format!("{}-1", hostname);

    let hello = Message::Hello(Hello::new(&hostname, &client_id));
    stream.write_all(&hello.encode()).await?;
    info!("Hello sent");

    // 2. Wait for CodecHeader, then ServerSettings
    let mut decoder: Option<ActiveDecoder> = None;
    let mut player:  Option<Player>        = None;
    let mut sync_buf: Option<SyncBuffer>   = None;

    let mut hdr_buf = [0u8; HEADER_SIZE];
    let mut pending_time: Option<(u16, i64)> = None; // (msg_id, sent_us)

    // Start periodic clock sync
    let mut sync_interval = tokio::time::interval(Duration::from_secs(1));
    let mut sync_seq: u16 = 0;

    loop {
        tokio::select! {
            // Read next message from server
            read_result = stream.read_exact(&mut hdr_buf) => {
                read_result?;
                let hdr = MessageHeader::from_bytes(&hdr_buf)?;
                let mut payload = vec![0u8; hdr.payload_size as usize];
                stream.read_exact(&mut payload).await?;

                match hdr.msg_type {
                    MessageType::CodecHeader => {
                        let ch = sonium_protocol::messages::CodecHeader::decode(&payload)?;
                        info!(codec = %ch.codec, "CodecHeader received");
                        let dec = ActiveDecoder::from_codec(&ch.codec, &ch.header_data)?;
                        let fmt = dec.sample_format();
                        let p   = Player::new(fmt)?;
                        let buf = SyncBuffer::new(fmt, cfg.latency_ms.unsigned_abs() + 1000);
                        decoder  = Some(dec);
                        player   = Some(p);
                        sync_buf = Some(buf);
                    }

                    MessageType::ServerSettings => {
                        let ss = sonium_protocol::messages::ServerSettings::decode(&payload)?;
                        debug!(volume = ss.volume, muted = ss.muted, buffer_ms = ss.buffer_ms, "ServerSettings");
                    }

                    MessageType::WireChunk => {
                        let chunk = sonium_protocol::messages::WireChunk::decode(&payload)?;
                        if let (Some(dec), Some(pl), Some(buf)) =
                            (decoder.as_mut(), player.as_mut(), sync_buf.as_mut())
                        {
                            let mut samples = Vec::new();
                            dec.decode(&chunk.data, &mut samples)?;
                            let playout_us = chunk.timestamp.to_micros()
                                + time_provider.to_local_time(0)
                                + cfg.latency_ms as i64 * 1000;

                            buf.push(PcmChunk::new(playout_us, samples, dec.sample_format()));

                            // Drain buffer for any chunks ready to play
                            let now = now_us();
                            while let Some(c) = buf.pop_ready(time_provider.to_server_time(now)) {
                                pl.write(&c.samples)?;
                            }
                        }
                    }

                    MessageType::Time => {
                        // Server echo: compute clock offset
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

            // Send periodic Time sync request
            _ = sync_interval.tick() => {
                let sent_us = now_us();
                sync_seq = sync_seq.wrapping_add(1);
                let mut hdr = MessageHeader::new(MessageType::Time, 8);
                hdr.id = sync_seq;
                let time_req = Message::Time(TimeMsg::zero()).encode_with_header(hdr);
                stream.write_all(&time_req).await?;
                pending_time = Some((sync_seq, sent_us));
            }
        }
    }
}
