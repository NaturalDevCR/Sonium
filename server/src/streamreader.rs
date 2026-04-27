use bytes::Bytes;
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
#[instrument(skip(bc, state, registry),
             fields(stream_id = %stream.id, source = %stream.source, codec = %stream.codec))]
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
        "Stream reader started"
    );

    let frame_samples = fmt.frames_for_ms(20.0) * fmt.channels as usize;
    let frame_bytes = frame_samples * 2; // i16 = 2 bytes
    let mut pcm_buf = vec![0u8; frame_bytes];
    let mut enc_buf: Vec<u8> = Vec::new();

    let idle_timeout = stream
        .idle_timeout_ms
        .map(|ms| Duration::from_millis(ms as u64));
    let silence_on_idle = stream.silence_on_idle;

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
        )
        .await
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

        // Borrow codec header from the first (highest-priority) source.
        if idx == 0 {
            let mut attempts = 0;
            while source_bc.codec_header().is_none() && attempts < 50 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                attempts += 1;
            }
            if let Some(hdr) = source_bc.codec_header() {
                bc.set_codec_header(hdr);
            } else {
                warn!(meta = %stream.id, "Primary source has no codec header yet — clients may connect without one");
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
) -> anyhow::Result<()> {
    let (cmd, args) = parse_pipe_uri(uri)?;

    info!(stream = stream_id, command = %cmd, args = ?args, "Spawning external process");

    let mut child = Command::new(&cmd)
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow::anyhow!("[{stream_id}] spawn `{cmd}`: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("[{stream_id}] no stdout from child"))?;

    let result = run_reader(
        stdout,
        encoder,
        bc,
        pcm_buf,
        enc_buf,
        stream_id,
        state,
        idle_timeout,
        silence_on_idle,
    )
    .await;

    match child.try_wait() {
        Ok(Some(status)) => info!("[{stream_id}] Process exited: {status}"),
        Ok(None) => {
            info!("[{stream_id}] Killing child process");
            let _ = child.kill().await;
        }
        Err(e) => warn!("[{stream_id}] Error checking child: {e}"),
    }

    result
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
) -> anyhow::Result<()> {
    let silence_pcm: Vec<i16> = vec![0i16; pcm_buf.len() / 2];
    let mut is_idle = false;
    let level_interval = tokio::time::Duration::from_millis(100);
    let mut last_level = tokio::time::Instant::now()
        .checked_sub(level_interval)
        .unwrap_or_else(tokio::time::Instant::now);

    'read: loop {
        // ── Try to read one frame ─────────────────────────────────────────
        let read_ok: bool = if let Some(dur) = idle_timeout {
            match tokio::time::timeout(dur, src.read_exact(pcm_buf)).await {
                Ok(Ok(_)) => true,
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    info!("[{stream_id}] Input closed — reader stopping");
                    break 'read;
                }
                Ok(Err(e)) => {
                    warn!("[{stream_id}] Read error: {e}");
                    break 'read;
                }
                Err(_timeout) => {
                    // No data within idle_timeout → go idle.
                    if !is_idle {
                        is_idle = true;
                        state.set_stream_status(stream_id, StreamStatus::Idle);
                        info!("[{stream_id}] No audio data for {dur:?} — stream idle");
                    }

                    if silence_on_idle {
                        // Emit silence frames at 20 ms intervals until data returns.
                        let mut tick = tokio::time::interval(Duration::from_millis(20));
                        tick.tick().await; // discard immediate first tick
                        loop {
                            tokio::select! {
                                biased;
                                result = src.read_exact(pcm_buf) => {
                                    match result {
                                        Ok(_) => {
                                            // Data resumed — break out of silence loop,
                                            // fall through to encode below.
                                        }
                                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                                            info!("[{stream_id}] Input closed during idle");
                                            break 'read;
                                        }
                                        Err(e) => {
                                            warn!("[{stream_id}] Read error during idle: {e}");
                                            break 'read;
                                        }
                                    }
                                    break; // exit silence loop, encode the received frame
                                }
                                _ = tick.tick() => {
                                    enc_buf.clear();
                                    if encoder.encode(&silence_pcm, enc_buf).is_ok() {
                                        let chunk = WireChunk::new(Timestamp::now(), enc_buf.clone());
                                        bc.publish(Bytes::from(Message::WireChunk(chunk).encode()));
                                    }
                                }
                            }
                        }
                    }
                    // (If silence_on_idle is false, we simply looped back and try read again.)
                    true
                }
            }
        } else {
            match src.read_exact(pcm_buf).await {
                Ok(_) => true,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    info!("[{stream_id}] Input closed — reader stopping");
                    break 'read;
                }
                Err(e) => {
                    warn!("[{stream_id}] Read error: {e}");
                    break 'read;
                }
            }
        };

        if !read_ok {
            continue;
        }

        // ── Transition idle → playing ─────────────────────────────────────
        if is_idle {
            is_idle = false;
            state.set_stream_status(stream_id, StreamStatus::Playing);
            info!("[{stream_id}] Audio data resumed — stream playing");
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

        let chunk = WireChunk::new(Timestamp::now(), enc_buf.clone());
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
