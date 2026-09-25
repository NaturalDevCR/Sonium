use bytes::Bytes;
use std::io;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio::time::Duration;
use tracing::{debug, info, warn};

use sonium_codec::make_encoder;
use sonium_common::config::StreamSource;
use sonium_protocol::{
    messages::{CodecHeader, Message, WireChunk},
    Timestamp,
};

use crate::broadcaster::{Broadcaster, BroadcasterRegistry};
use sonium_control::{state::StreamStatus, ws::Event, ServerState};
use tracing::instrument;

/// Compute RMS level in dBFS for a block of i16 PCM samples.
fn rms_dbfs(pcm: &[i16]) -> f32 {
    if pcm.is_empty() {
        return -90.0;
    }
    let sum: f64 = pcm.iter().map(|s| (*s as f64 / 32768.0).powi(2)).sum();
    let rms = (sum / pcm.len() as f64).sqrt();
    if rms < 1e-9 {
        return -90.0;
    }
    (20.0 * rms.log10()) as f32
}

/// Assigns continuous, real-time paced timestamps to outgoing chunks.
///
/// Mirrors Snapcast's stream readers: each chunk's timestamp is the previous
/// one plus the chunk duration, and a chunk is never published before its
/// timestamp. This throttles sources that deliver faster than real time
/// (plain files, `cat file > fifo`, MPD/Mopidy FIFO outputs, ffmpeg decoding
/// a URL, Icecast burst-on-connect) through normal pipe back-pressure, and
/// removes read-scheduling jitter from the timestamps clients schedule on.
///
/// When the source falls behind (stall, idle gap) or the wall clock jumps by
/// more than `tolerance_us`, the timeline re-anchors to "now".
pub(crate) struct Pacer {
    chunk_us: i64,
    tolerance_us: i64,
    next_ts_us: Option<i64>,
}

impl Pacer {
    pub(crate) fn new(chunk_ms: u32, buffer_ms: u32) -> Self {
        let chunk_us = i64::from(chunk_ms.max(1)) * 1000;
        // Late data is still playable while it arrives within the client
        // buffer, so tolerate up to half of it before re-anchoring.
        let tolerance_us = (i64::from(buffer_ms) * 1000 / 2).clamp(2 * chunk_us, 1_000_000);
        Self {
            chunk_us,
            tolerance_us,
            next_ts_us: None,
        }
    }

    /// Timestamp (µs, server wall clock) for the next chunk given `now_us`.
    pub(crate) fn slot(&self, now_us: i64) -> i64 {
        match self.next_ts_us {
            Some(ts) if now_us - ts <= self.tolerance_us && ts - now_us <= self.tolerance_us => ts,
            _ => now_us,
        }
    }

    /// Mark the slot starting at `ts_us` as used.
    pub(crate) fn advance(&mut self, ts_us: i64) {
        self.next_ts_us = Some(ts_us + self.chunk_us);
    }

    /// Next slot timestamp and how long until it is due.
    pub(crate) fn time_until_slot(&self) -> (i64, Duration) {
        let now = sonium_sync::time_provider::now_us();
        let ts = self.slot(now);
        (ts, Duration::from_micros((ts - now).max(0) as u64))
    }

    /// Wait until the next slot is due and claim it. Returns its timestamp.
    pub(crate) async fn wait_for_slot(&mut self) -> i64 {
        let (ts, wait) = self.time_until_slot();
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        self.advance(ts);
        ts
    }
}

/// Read PCM from stdin, a named FIFO, TCP, or an external process, encode, and broadcast.
///
/// Source format:
/// - `"-"` — reads from stdin
/// - `meta://id1/id2/id3` — virtual stream: forwards from highest-priority active source
/// - `pipe:///path/to/cmd?arg1&arg2` — spawns child process
/// - `tcp://host:port` — connects to a TCP PCM source
/// - `tcp-listen://host:port` — listens for TCP PCM source connections
/// - `tcp://host:port?mode=server` — Snapcast-style TCP listener
/// - anything else — opens path as a file/FIFO
///
/// Input is raw interleaved i16 LE PCM matching `stream.sample_format`.
#[instrument(skip_all, fields(stream_id = %stream.id))]
pub async fn run(
    bc: Arc<Broadcaster>,
    stream: StreamSource,
    state: Arc<ServerState>,
    registry: Arc<BroadcasterRegistry>,
) -> anyhow::Result<()> {
    // Meta streams are a special case — no encoder, just routing.
    if stream.source.starts_with("meta://") {
        return run_meta(stream, bc, state, registry).await;
    }

    let fmt = stream.sample_format;
    let codec = stream.codec.as_str();

    let mut encoder = make_encoder(codec, fmt)
        .map_err(|e| anyhow::anyhow!("[{}] encoder init: {e}", stream.id))?;

    let codec_hdr_msg = Message::CodecHeader(CodecHeader::new(
        encoder.codec_name(),
        encoder.codec_header(),
    ));
    bc.set_codec_header(Bytes::from(codec_hdr_msg.encode()));

    info!(
        id     = %stream.id,
        source = %stream.source,
        codec,
        format = %fmt,
        chunk_ms = stream_chunk_ms(&stream),
        "Stream reader started"
    );

    let frame_samples = fmt.frames_for_ms(stream_chunk_ms(&stream) as f64) * fmt.channels as usize;
    let frame_bytes = frame_samples * 2; // i16 = 2 bytes
    let mut pcm_buf = vec![0u8; frame_bytes];
    let mut enc_buf: Vec<u8> = Vec::new();

    let idle_timeout = stream
        .idle_timeout_ms
        .map(|ms| Duration::from_millis(ms as u64));
    let silence_on_idle = stream.silence_on_idle;

    let chunk_ms = stream_chunk_ms(&stream);
    let mut pacer = Pacer::new(chunk_ms, stream.buffer_ms.unwrap_or(bc.buffer_ms));

    if stream.source == "-" {
        run_reader(
            tokio::io::stdin(),
            &mut *encoder,
            bc,
            &mut pcm_buf,
            &mut enc_buf,
            &stream.id,
            &state,
            idle_timeout,
            silence_on_idle,
            &mut pacer,
        )
        .await
    } else if stream.source.starts_with("pipe://") {
        run_pipe(
            &stream.source,
            &mut *encoder,
            bc,
            &mut pcm_buf,
            &mut enc_buf,
            &stream.id,
            &state,
            idle_timeout,
            silence_on_idle,
            &mut pacer,
        )
        .await
    } else if let Some(tcp) = parse_tcp_source(&stream.source)? {
        run_tcp(
            tcp,
            &mut *encoder,
            bc,
            &mut pcm_buf,
            &mut enc_buf,
            &stream.id,
            &state,
            idle_timeout,
            silence_on_idle,
            &mut pacer,
        )
        .await
    } else {
        let file = tokio::fs::File::open(&stream.source)
            .await
            .map_err(|e| anyhow::anyhow!("[{}] open {}: {e}", stream.id, stream.source))?;
        run_reader(
            file,
            &mut *encoder,
            bc,
            &mut pcm_buf,
            &mut enc_buf,
            &stream.id,
            &state,
            idle_timeout,
            silence_on_idle,
            &mut pacer,
        )
        .await
    }
}

pub(crate) fn stream_chunk_ms(stream: &StreamSource) -> u32 {
    let ms = stream.chunk_ms.unwrap_or(20).clamp(10, 60);
    match stream.codec.as_str() {
        "opus" => match ms {
            10 | 20 | 40 | 60 => ms,
            0..=14 => 10,
            15..=29 => 20,
            30..=49 => 40,
            _ => 60,
        },
        _ => ms,
    }
}

// ── Meta streams ──────────────────────────────────────────────────────────

async fn run_meta(
    stream: StreamSource,
    bc: Arc<Broadcaster>,
    state: Arc<ServerState>,
    registry: Arc<BroadcasterRegistry>,
) -> anyhow::Result<()> {
    let source_ids: Vec<String> = stream
        .source
        .strip_prefix("meta://")
        .unwrap_or("")
        .split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    if source_ids.is_empty() {
        anyhow::bail!("[{}] meta:// source has no stream IDs", stream.id);
    }

    info!(id = %stream.id, sources = ?source_ids, "Starting meta stream");

    // Each source stream forwards its frames into a shared channel, tagged with its priority index.
    struct Tagged {
        idx: usize,
        frame: crate::broadcaster::AudioFrame,
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Tagged>(1024);
    let mut primary_header: Option<Bytes> = None;

    for (idx, source_id) in source_ids.iter().enumerate() {
        // Wait up to 5 s for each source broadcaster to register.
        let source_bc = {
            let mut source_bc = None;
            for _ in 0..50 {
                if let Some(bc) = crate::broadcaster::lookup(&registry, source_id) {
                    source_bc = Some(bc);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            match source_bc {
                Some(bc) => bc,
                None => {
                    warn!(meta = %stream.id, source = %source_id, "Source not found — skipping");
                    continue;
                }
            }
        };

        let mut attempts = 0;
        while source_bc.codec_header().is_none() && attempts < 50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            attempts += 1;
        }

        // Borrow the codec header from the first (highest-priority) source.
        // Encoded frames are forwarded as-is, so every other source must use
        // exactly the same codec and sample format — clients only receive a new
        // header when their stream changes, not when the meta stream fails over.
        if idx == 0 {
            if let Some(hdr) = source_bc.codec_header() {
                bc.set_codec_header(hdr.clone());
                primary_header = Some(hdr);
            } else {
                warn!(meta = %stream.id, "Primary source has no codec header yet — clients may connect without one");
            }
        } else if let (Some(primary), Some(hdr)) = (&primary_header, source_bc.codec_header()) {
            if *primary != hdr {
                warn!(
                    meta = %stream.id,
                    source = %source_id,
                    "Source uses a different codec or sample format than the primary source — skipping it (use the same codec/sample_format for all meta sources)"
                );
                continue;
            }
        }

        let tx = tx.clone();
        let meta_id = stream.id.clone();
        let source_id = source_id.clone();
        tokio::spawn(async move {
            let mut sub = source_bc.subscribe();
            loop {
                match sub.recv().await {
                    Ok(frame) => {
                        if tx.send(Tagged { idx, frame }).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        debug!(meta = %meta_id, source = %source_id, "Lagged, dropped {n} frames");
                    }
                    Err(_) => break,
                }
            }
        });
    }
    drop(tx); // Once all source tasks exit, rx.recv() returns None.

    // "Active" threshold: a source is considered live if it sent a frame
    // within idle_timeout_ms (default 3 s).
    let active_threshold = Duration::from_millis(stream.idle_timeout_ms.unwrap_or(3_000) as u64);
    let mut last_seen: Vec<tokio::time::Instant> = {
        let long_ago = tokio::time::Instant::now()
            .checked_sub(Duration::from_secs(3600))
            .unwrap_or_else(tokio::time::Instant::now);
        vec![long_ago; source_ids.len()]
    };

    while let Some(tagged) = rx.recv().await {
        let now = tokio::time::Instant::now();
        last_seen[tagged.idx] = now;

        // Find the highest-priority (lowest index) source that is still "live".
        let active_idx = last_seen
            .iter()
            .enumerate()
            .find(|(_, t)| now.duration_since(**t) < active_threshold)
            .map(|(i, _)| i);

        if active_idx == Some(tagged.idx) {
            bc.publish(tagged.frame.wire_bytes);
        }
    }

    state.set_stream_status(&stream.id, StreamStatus::Idle);
    Ok(())
}

// ── TCP helpers ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum TcpMode {
    Connect,
    Listen,
}

#[derive(Debug, Clone)]
struct TcpSource {
    mode: TcpMode,
    addr: String,
}

fn parse_tcp_source(source: &str) -> anyhow::Result<Option<TcpSource>> {
    if let Some(rest) = source.strip_prefix("tcp-listen://") {
        return Ok(Some(TcpSource {
            mode: TcpMode::Listen,
            addr: strip_query(rest).to_owned(),
        }));
    }

    let Some(rest) = source.strip_prefix("tcp://") else {
        return Ok(None);
    };

    let (addr, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mode = if query
        .split('&')
        .any(|p| matches!(p, "mode=server" | "listen" | "listen=1" | "server=1"))
    {
        TcpMode::Listen
    } else {
        TcpMode::Connect
    };

    if addr.is_empty() {
        anyhow::bail!("TCP source has empty address: {source}");
    }

    Ok(Some(TcpSource {
        mode,
        addr: addr.to_owned(),
    }))
}

fn strip_query(value: &str) -> &str {
    value.split_once('?').map(|(a, _)| a).unwrap_or(value)
}

#[allow(clippy::too_many_arguments)]
async fn run_tcp(
    tcp: TcpSource,
    encoder: &mut (dyn sonium_codec::Encoder + Send),
    bc: Arc<Broadcaster>,
    pcm_buf: &mut [u8],
    enc_buf: &mut Vec<u8>,
    stream_id: &str,
    state: &Arc<ServerState>,
    idle_timeout: Option<Duration>,
    silence_on_idle: bool,
    pacer: &mut Pacer,
) -> anyhow::Result<()> {
    match tcp.mode {
        TcpMode::Connect => {
            info!(stream = stream_id, addr = %tcp.addr, "Connecting to TCP source");
            let socket = TcpStream::connect(&tcp.addr)
                .await
                .map_err(|e| anyhow::anyhow!("[{stream_id}] connect {}: {e}", tcp.addr))?;
            run_reader(
                socket,
                encoder,
                bc,
                pcm_buf,
                enc_buf,
                stream_id,
                state,
                idle_timeout,
                silence_on_idle,
                pacer,
            )
            .await
        }
        TcpMode::Listen => {
            let listener = TcpListener::bind(&tcp.addr)
                .await
                .map_err(|e| anyhow::anyhow!("[{stream_id}] bind {}: {e}", tcp.addr))?;
            info!(stream = stream_id, addr = %tcp.addr, "Listening for TCP source");

            loop {
                let (socket, peer) = listener.accept().await?;
                info!(stream = stream_id, %peer, "TCP source connected");
                if let Err(e) = run_reader(
                    socket,
                    encoder,
                    bc.clone(),
                    pcm_buf,
                    enc_buf,
                    stream_id,
                    state,
                    idle_timeout,
                    silence_on_idle,
                    pacer,
                )
                .await
                {
                    warn!(stream = stream_id, %peer, "TCP source ended: {e}");
                }
                info!(stream = stream_id, %peer, "TCP source disconnected; waiting for next sender");
            }
        }
    }
}

// ── Pipe (child process) ──────────────────────────────────────────────────

/// Format: `pipe:///absolute/path/to/command?arg1&arg2&arg3`
#[allow(clippy::too_many_arguments)]
async fn run_pipe(
    uri: &str,
    encoder: &mut (dyn sonium_codec::Encoder + Send),
    bc: Arc<Broadcaster>,
    pcm_buf: &mut [u8],
    enc_buf: &mut Vec<u8>,
    stream_id: &str,
    state: &Arc<ServerState>,
    idle_timeout: Option<Duration>,
    silence_on_idle: bool,
    pacer: &mut Pacer,
) -> anyhow::Result<()> {
    let (cmd, args) = parse_pipe_uri(uri)?;

    let mut restart_count: u64 = 0;

    loop {
        info!(stream = stream_id, command = %cmd, args = ?args, restart_count, "Starting external audio source");

        let mut child = Command::new(&cmd)
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow::anyhow!("[{stream_id}] spawn `{cmd}`: {e}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("[{stream_id}] no stdout from child"))?;

        // Drain stderr in a background task (bounded to 8 KiB) so it never blocks the child.
        let stderr_task = {
            let mut se = child.stderr.take();
            tokio::spawn(async move {
                let mut buf = Vec::with_capacity(8192);
                if let Some(ref mut reader) = se {
                    let _ = AsyncReadExt::take(reader, 8192).read_to_end(&mut buf).await;
                }
                buf
            })
        };

        let result = run_reader(
            stdout,
            encoder,
            bc.clone(),
            pcm_buf,
            enc_buf,
            stream_id,
            state,
            idle_timeout,
            silence_on_idle,
            pacer,
        )
        .await;

        match child.try_wait() {
            Ok(Some(status)) if !status.success() => {
                let stderr_bytes = stderr_task.await.unwrap_or_default();
                let stderr_str = String::from_utf8_lossy(&stderr_bytes);
                let tail = stderr_str.trim();
                if tail.is_empty() {
                    warn!(stream = stream_id, %status, "External audio source exited");
                } else {
                    warn!(stream = stream_id, %status, stderr = %tail, "External audio source exited");
                }
            }
            Ok(Some(_status)) => {
                let _ = stderr_task.await;
            }
            Ok(None) => {
                info!(
                    stream = stream_id,
                    "Stopping external audio source after input ended"
                );
                let _ = child.kill().await;
                let _ = stderr_task.await;
            }
            Err(e) => {
                let _ = stderr_task.await;
                warn!(
                    stream = stream_id,
                    "Error checking external audio source: {e}"
                );
            }
        }

        if let Err(e) = result {
            warn!(
                stream = stream_id,
                "Audio source read failed; restarting external process: {e}"
            );
        } else {
            warn!(
                stream = stream_id,
                "Audio source closed; restarting external process"
            );
        }

        state.set_stream_status(stream_id, StreamStatus::Idle);
        restart_count = restart_count.saturating_add(1);
        let restart_delay = Duration::from_secs((restart_count * 2).min(30));
        info!(
            stream = stream_id,
            restart_in_ms = restart_delay.as_millis(),
            "Waiting before restarting external audio source"
        );
        tokio::time::sleep(restart_delay).await;
    }
}

fn parse_pipe_uri(uri: &str) -> anyhow::Result<(String, Vec<String>)> {
    let rest = uri
        .strip_prefix("pipe://")
        .ok_or_else(|| anyhow::anyhow!("not a pipe:// URI: {uri}"))?;

    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };

    if path.is_empty() {
        anyhow::bail!("pipe:// URI has empty command path: {uri}");
    }

    let args: Vec<String> = query
        .map(|q| q.split('&').map(String::from).collect())
        .unwrap_or_default();

    Ok((path.to_owned(), args))
}

// ── Core read loop (with idle detection) ─────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_reader<R: AsyncReadExt + Unpin>(
    mut src: R,
    encoder: &mut (dyn sonium_codec::Encoder + Send),
    bc: Arc<Broadcaster>,
    pcm_buf: &mut [u8],
    enc_buf: &mut Vec<u8>,
    stream_id: &str,
    state: &Arc<ServerState>,
    idle_timeout: Option<Duration>,
    silence_on_idle: bool,
    pacer: &mut Pacer,
) -> anyhow::Result<()> {
    let silence_pcm: Vec<i16> = vec![0i16; pcm_buf.len() / 2];
    let mut is_idle = false;
    let level_interval = tokio::time::Duration::from_millis(100);
    let mut last_level = tokio::time::Instant::now()
        .checked_sub(level_interval)
        .unwrap_or_else(tokio::time::Instant::now);
    let mut pcm_filled = 0usize;

    'read: loop {
        // ── Try to read one frame ─────────────────────────────────────────
        let read_ok: bool = if let Some(dur) = idle_timeout {
            match read_pcm_frame(&mut src, pcm_buf, &mut pcm_filled, Some(dur)).await {
                FrameRead::Frame => true,
                FrameRead::Eof => {
                    info!(stream = stream_id, "Audio input closed");
                    break 'read;
                }
                FrameRead::Error(e) => {
                    warn!(stream = stream_id, "Audio input read error: {e}");
                    break 'read;
                }
                FrameRead::Idle => {
                    // No data within idle_timeout → go idle.
                    if !is_idle {
                        is_idle = true;
                        state.set_stream_status(stream_id, StreamStatus::Idle);
                        info!(
                            stream = stream_id,
                            idle_after_ms = dur.as_millis(),
                            "No audio data received; stream idle"
                        );
                    }

                    if silence_on_idle {
                        // Emit paced silence frames until data returns.
                        loop {
                            let (silence_ts, silence_wait) = pacer.time_until_slot();
                            tokio::select! {
                                biased;
                                result = read_pcm_frame(&mut src, pcm_buf, &mut pcm_filled, None) => {
                                    match result {
                                        FrameRead::Frame => {
                                            // Data resumed — break out of silence loop,
                                            // fall through to encode below.
                                        }
                                        FrameRead::Eof => {
                                            info!(stream = stream_id, "Audio input closed while idle");
                                            break 'read;
                                        }
                                        FrameRead::Error(e) => {
                                            warn!(stream = stream_id, "Audio input read error while idle: {e}");
                                            break 'read;
                                        }
                                        FrameRead::Idle => unreachable!("idle is disabled while waiting for resumed audio"),
                                    }
                                    break; // exit silence loop, encode the received frame
                                }
                                _ = tokio::time::sleep(silence_wait) => {
                                    pacer.advance(silence_ts);
                                    enc_buf.clear();
                                    if encoder.encode(&silence_pcm, enc_buf).is_ok() {
                                        let chunk = WireChunk::new(
                                            Timestamp::from_micros(silence_ts),
                                            enc_buf.clone(),
                                        );
                                        bc.publish(Bytes::from(Message::WireChunk(chunk).encode()));
                                    }
                                }
                            }
                        }
                        // The silence loop only exits with a freshly read frame.
                        true
                    } else {
                        // No complete frame yet; keep waiting (partial data is preserved).
                        false
                    }
                }
            }
        } else {
            match read_pcm_frame(&mut src, pcm_buf, &mut pcm_filled, None).await {
                FrameRead::Frame => true,
                FrameRead::Eof => {
                    info!(stream = stream_id, "Audio input closed");
                    break 'read;
                }
                FrameRead::Error(e) => {
                    warn!(stream = stream_id, "Audio input read error: {e}");
                    break 'read;
                }
                FrameRead::Idle => unreachable!("idle is disabled for blocking reads"),
            }
        };

        if !read_ok {
            continue;
        }

        // ── Transition idle → playing ─────────────────────────────────────
        if is_idle {
            is_idle = false;
            state.set_stream_status(stream_id, StreamStatus::Playing);
            info!(stream = stream_id, "Audio data resumed; stream playing");
        }

        // ── Encode and broadcast ──────────────────────────────────────────
        let pcm: Vec<i16> = pcm_buf
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();

        enc_buf.clear();
        if let Err(e) = encoder.encode(&pcm, enc_buf) {
            warn!("[{stream_id}] Encode error: {e}");
            continue;
        }

        // Pace to real time and stamp with the continuous timeline.
        let ts_us = pacer.wait_for_slot().await;
        let chunk = WireChunk::new(Timestamp::from_micros(ts_us), enc_buf.clone());
        debug!(
            stream = stream_id,
            bytes = enc_buf.len(),
            "Broadcasting frame"
        );
        bc.publish(Bytes::from(Message::WireChunk(chunk).encode()));

        // ── VU meter: emit StreamLevel ~10×/s ────────────────────────────
        let now = tokio::time::Instant::now();
        if now.duration_since(last_level) >= level_interval {
            last_level = now;
            let rms_db = rms_dbfs(&pcm);
            state.events().emit(Event::StreamLevel {
                stream_id: stream_id.to_owned(),
                rms_db,
            });
        }
    }

    Ok(())
}

pub(crate) enum FrameRead {
    Frame,
    Idle,
    Eof,
    Error(io::Error),
}

pub(crate) async fn read_pcm_frame<R: AsyncReadExt + Unpin>(
    src: &mut R,
    pcm_buf: &mut [u8],
    filled: &mut usize,
    idle_timeout: Option<Duration>,
) -> FrameRead {
    while *filled < pcm_buf.len() {
        let read = src.read(&mut pcm_buf[*filled..]);
        let result = if let Some(timeout) = idle_timeout {
            match tokio::time::timeout(timeout, read).await {
                Ok(result) => result,
                Err(_) => return FrameRead::Idle,
            }
        } else {
            read.await
        };

        match result {
            Ok(0) => {
                *filled = 0;
                return FrameRead::Eof;
            }
            Ok(n) => *filled += n,
            Err(e) => {
                *filled = 0;
                return FrameRead::Error(e);
            }
        }
    }

    *filled = 0;
    FrameRead::Frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn pacer_produces_continuous_timeline() {
        let mut pacer = Pacer::new(20, 200);
        let t0 = 1_000_000_000;
        assert_eq!(pacer.slot(t0), t0);
        pacer.advance(t0);
        // Source delivered early: keep the timeline (caller waits).
        assert_eq!(pacer.slot(t0 + 5_000), t0 + 20_000);
        pacer.advance(t0 + 20_000);
        // Slightly late data stays on the timeline (within tolerance).
        assert_eq!(pacer.slot(t0 + 90_000), t0 + 40_000);
    }

    #[test]
    fn pacer_reanchors_after_stall_or_clock_jump() {
        let mut pacer = Pacer::new(20, 200);
        let t0 = 1_000_000_000;
        pacer.advance(t0);
        // 100 ms tolerance (half of 200 ms buffer) exceeded -> re-anchor.
        assert_eq!(pacer.slot(t0 + 500_000), t0 + 500_000);
        // Wall clock stepped backwards -> re-anchor instead of sleeping.
        assert_eq!(pacer.slot(t0 - 10_000_000), t0 - 10_000_000);
    }

    #[test]
    fn pacer_tolerance_is_bounded() {
        assert_eq!(Pacer::new(20, 10).tolerance_us, 40_000);
        assert_eq!(Pacer::new(20, 60_000).tolerance_us, 1_000_000);
    }

    #[tokio::test]
    async fn pacer_throttles_fast_source_to_real_time() {
        let mut pacer = Pacer::new(10, 200);
        let started = std::time::Instant::now();
        let mut stamps = Vec::new();
        for _ in 0..10 {
            stamps.push(pacer.wait_for_slot().await);
        }
        // 10 chunks of 10 ms: the last one is due ~90 ms after the first.
        assert!(started.elapsed() >= Duration::from_millis(85));
        for pair in stamps.windows(2) {
            assert_eq!(pair[1] - pair[0], 10_000);
        }
    }

    #[tokio::test]
    async fn read_pcm_frame_preserves_partial_data_after_idle() {
        let (mut source, mut sink) = tokio::io::duplex(16);
        let mut pcm = [0u8; 8];
        let mut filled = 0usize;

        sink.write_all(&[1, 2, 3, 4]).await.unwrap();
        match read_pcm_frame(
            &mut source,
            &mut pcm,
            &mut filled,
            Some(Duration::from_millis(5)),
        )
        .await
        {
            FrameRead::Idle => {}
            _ => panic!("expected idle with a partial frame"),
        }

        assert_eq!(filled, 4);

        sink.write_all(&[5, 6, 7, 8]).await.unwrap();
        match read_pcm_frame(&mut source, &mut pcm, &mut filled, None).await {
            FrameRead::Frame => {}
            _ => panic!("expected complete frame"),
        }

        assert_eq!(filled, 0);
        assert_eq!(pcm, [1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
