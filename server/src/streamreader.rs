use bytes::Bytes;
use std::io;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
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
#[instrument(skip_all, fields(stream_id = %stream.id))]
pub async fn run(
    bc: Arc<Broadcaster>,
    stream: StreamSource,
    state: Arc<ServerState>,
    registry: Arc<BroadcasterRegistry>,
    reopen_cancel: CancellationToken,
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

    if stream.source == "-" {
        let _ = run_reader(
            tokio::io::stdin(),
            &mut *encoder,
            bc,
            &mut pcm_buf,
            &mut enc_buf,
            &stream.id,
            &state,
            idle_timeout,
            silence_on_idle,
            chunk_ms,
        )
        .await;
        Ok(())
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
            chunk_ms,
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
            chunk_ms,
        )
        .await
    } else {
        run_reopening_reader(
            &stream.source,
            &mut *encoder,
            bc,
            &mut pcm_buf,
            &mut enc_buf,
            &stream.id,
            &state,
            idle_timeout,
            silence_on_idle,
            chunk_ms,
            reopen_cancel,
        )
        .await
    }
}

fn stream_chunk_ms(stream: &StreamSource) -> u32 {
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
    chunk_ms: u32,
) -> anyhow::Result<()> {
    match tcp.mode {
        TcpMode::Connect => {
            info!(stream = stream_id, addr = %tcp.addr, "Connecting to TCP source");
            let socket = TcpStream::connect(&tcp.addr)
                .await
                .map_err(|e| anyhow::anyhow!("[{stream_id}] connect {}: {e}", tcp.addr))?;
            let _ = run_reader(
                socket,
                encoder,
                bc,
                pcm_buf,
                enc_buf,
                stream_id,
                state,
                idle_timeout,
                silence_on_idle,
                chunk_ms,
            )
            .await;
            Ok(())
        }
        TcpMode::Listen => {
            let listener = TcpListener::bind(&tcp.addr)
                .await
                .map_err(|e| anyhow::anyhow!("[{stream_id}] bind {}: {e}", tcp.addr))?;
            info!(stream = stream_id, addr = %tcp.addr, "Listening for TCP source");

            loop {
                let (socket, peer) = listener.accept().await?;
                info!(stream = stream_id, %peer, "TCP source connected");
                if let ReaderEnd::Error { error: e, .. } = run_reader(
                    socket,
                    encoder,
                    bc.clone(),
                    pcm_buf,
                    enc_buf,
                    stream_id,
                    state,
                    idle_timeout,
                    silence_on_idle,
                    chunk_ms,
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
    chunk_ms: u32,
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
            chunk_ms,
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

        match result {
            ReaderEnd::Error { error: e, .. } => {
                warn!(
                    stream = stream_id,
                    "Audio source read failed; restarting external process: {e}"
                );
            }
            ReaderEnd::Eof { .. } => {
                warn!(
                    stream = stream_id,
                    "Audio source closed; restarting external process"
                );
            }
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

// ── File/FIFO reopen supervision ─────────────────────────────────────────

const REOPEN_BACKOFF_INITIAL: Duration = Duration::from_millis(50);
const REOPEN_BACKOFF_MAX: Duration = Duration::from_secs(2);

type PathReader = Box<dyn AsyncRead + Send + Unpin>;

struct OpenedPathSource {
    reader: PathReader,
    kind: PathSourceKind,
}

enum PathSourceKind {
    Fifo,
    Regular(FileFingerprint),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
    /// Portable replacement identity for platforms without a Unix inode.
    created: Option<SystemTime>,
    #[cfg(unix)]
    inode: u64,
}

/// Reopen a path-backed source after EOF or a recoverable read/open failure.
///
/// This is intentionally used only for ordinary paths. Stdin, TCP, and
/// `pipe://` sources retain their own source-specific lifecycle semantics.
#[allow(clippy::too_many_arguments)]
async fn run_reopening_reader(
    source: &str,
    encoder: &mut (dyn sonium_codec::Encoder + Send),
    bc: Arc<Broadcaster>,
    pcm_buf: &mut [u8],
    enc_buf: &mut Vec<u8>,
    stream_id: &str,
    state: &Arc<ServerState>,
    idle_timeout: Option<Duration>,
    silence_on_idle: bool,
    chunk_ms: u32,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    if source.trim().is_empty() {
        state.set_stream_status(stream_id, StreamStatus::Error);
        anyhow::bail!("[{stream_id}] file/FIFO source path is empty");
    }

    let mut attempt = 0u32;
    loop {
        let open = tokio::select! {
            result = open_file_source(source) => result,
            _ = cancel.cancelled() => return Ok(()),
        };

        match open {
            Ok(opened) => {
                state.set_stream_status(stream_id, StreamStatus::Playing);
                let end = tokio::select! {
                    end = run_reader(
                        opened.reader,
                        encoder,
                        bc.clone(),
                        pcm_buf,
                        enc_buf,
                        stream_id,
                        state,
                        idle_timeout,
                        silence_on_idle,
                        chunk_ms,
                    ) => end,
                    _ = cancel.cancelled() => return Ok(()),
                };
                let received_frame = end.received_frame();
                match end {
                    ReaderEnd::Eof { .. } => {
                        info!(stream = stream_id, "File/FIFO input closed; reopening");
                    }
                    ReaderEnd::Error { error, .. } if terminal_source_error(&error) => {
                        state.set_stream_status(stream_id, StreamStatus::Error);
                        return Err(anyhow::anyhow!(
                            "[{stream_id}] terminal read error from {source}: {error}"
                        ));
                    }
                    ReaderEnd::Error { error, .. } => {
                        warn!(
                            stream = stream_id,
                            "File/FIFO read error; reopening: {error}"
                        );
                    }
                }

                if received_frame {
                    attempt = 0;
                }

                match opened.kind {
                    PathSourceKind::Fifo => {
                        if !wait_before_reopen(stream_id, state, &mut attempt, &cancel).await {
                            return Ok(());
                        }
                    }
                    PathSourceKind::Regular(fingerprint) => {
                        if !wait_for_regular_source_change(
                            source,
                            &fingerprint,
                            stream_id,
                            state,
                            &mut attempt,
                            &cancel,
                        )
                        .await?
                        {
                            return Ok(());
                        }
                    }
                }
            }
            Err(error) if terminal_source_error(&error) => {
                state.set_stream_status(stream_id, StreamStatus::Error);
                return Err(anyhow::anyhow!(
                    "[{stream_id}] cannot open {source}: {error}"
                ));
            }
            Err(error) => {
                warn!(
                    stream = stream_id,
                    "File/FIFO open failed; reopening: {error}"
                );
            }
        }

        if !wait_before_reopen(stream_id, state, &mut attempt, &cancel).await {
            return Ok(());
        }
    }
}

async fn wait_before_reopen(
    stream_id: &str,
    state: &Arc<ServerState>,
    attempt: &mut u32,
    cancel: &CancellationToken,
) -> bool {
    *attempt = attempt.saturating_add(1);
    let retry = reopen_backoff(*attempt);
    state.set_stream_recovering(stream_id, *attempt, retry.as_millis() as u64);
    info!(
        stream = stream_id,
        attempt = *attempt,
        retry_in_ms = retry.as_millis(),
        "Waiting to reopen file/FIFO input"
    );
    tokio::select! {
        _ = tokio::time::sleep(retry) => true,
        _ = cancel.cancelled() => false,
    }
}

async fn wait_for_regular_source_change(
    source: &str,
    fingerprint: &FileFingerprint,
    stream_id: &str,
    state: &Arc<ServerState>,
    attempt: &mut u32,
    cancel: &CancellationToken,
) -> anyhow::Result<bool> {
    loop {
        if !wait_before_reopen(stream_id, state, attempt, cancel).await {
            return Ok(false);
        }

        match regular_file_fingerprint(source).await {
            Ok(Some(current)) if current != *fingerprint => return Ok(true),
            Ok(Some(_)) => continue,
            // Removal is a source change. The next open will retain its
            // recoverable NotFound behavior until a producer recreates it.
            Ok(None) => return Ok(true),
            Err(error) if terminal_source_error(&error) => {
                state.set_stream_status(stream_id, StreamStatus::Error);
                return Err(anyhow::anyhow!(
                    "[{stream_id}] cannot inspect {source}: {error}"
                ));
            }
            Err(error) => {
                warn!(
                    stream = stream_id,
                    "File change check failed; reopening: {error}"
                );
                return Ok(true);
            }
        }
    }
}

async fn open_file_source(source: &str) -> io::Result<OpenedPathSource> {
    let metadata = tokio::fs::metadata(source).await?;
    if metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::IsADirectory,
            "source path is a directory",
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;

        if metadata.file_type().is_fifo() {
            let reader = tokio::net::unix::pipe::OpenOptions::new().open_receiver(source)?;
            return Ok(OpenedPathSource {
                reader: Box::new(reader),
                kind: PathSourceKind::Fifo,
            });
        }
    }

    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "source path is neither a regular file nor FIFO",
        ));
    }

    let fingerprint = FileFingerprint::from_metadata(&metadata);
    let file = tokio::fs::File::open(source).await?;
    Ok(OpenedPathSource {
        reader: Box::new(file),
        kind: PathSourceKind::Regular(fingerprint),
    })
}

async fn regular_file_fingerprint(source: &str) -> io::Result<Option<FileFingerprint>> {
    match tokio::fs::metadata(source).await {
        Ok(metadata) if metadata.is_file() => Ok(Some(FileFingerprint::from_metadata(&metadata))),
        Ok(metadata) if metadata.is_dir() => Err(io::Error::new(
            io::ErrorKind::IsADirectory,
            "source path is a directory",
        )),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "source path is no longer a regular file",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

impl FileFingerprint {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
            #[cfg(unix)]
            inode: {
                use std::os::unix::fs::MetadataExt;
                metadata.ino()
            },
        }
    }
}

fn terminal_source_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied
            | io::ErrorKind::InvalidInput
            | io::ErrorKind::InvalidData
            | io::ErrorKind::NotADirectory
            | io::ErrorKind::IsADirectory
            | io::ErrorKind::TooManyLinks
            | io::ErrorKind::InvalidFilename
            | io::ErrorKind::Unsupported
    ) || is_symlink_loop(error)
}

fn is_symlink_loop(error: &io::Error) -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const ELOOP: i32 = 40;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    const ELOOP: i32 = 62;
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )))]
    const ELOOP: i32 = -1;

    error.raw_os_error() == Some(ELOOP)
}

fn reopen_backoff(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(5);
    REOPEN_BACKOFF_INITIAL
        .checked_mul(1_u32 << shift)
        .unwrap_or(REOPEN_BACKOFF_MAX)
        .min(REOPEN_BACKOFF_MAX)
}

// ── Core read loop (with idle detection) ─────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_reader<R: AsyncRead + Unpin>(
    mut src: R,
    encoder: &mut (dyn sonium_codec::Encoder + Send),
    bc: Arc<Broadcaster>,
    pcm_buf: &mut [u8],
    enc_buf: &mut Vec<u8>,
    stream_id: &str,
    state: &Arc<ServerState>,
    idle_timeout: Option<Duration>,
    silence_on_idle: bool,
    chunk_ms: u32,
) -> ReaderEnd {
    let silence_pcm: Vec<i16> = vec![0i16; pcm_buf.len() / 2];
    let mut is_idle = false;
    let level_interval = tokio::time::Duration::from_millis(100);
    let mut last_level = tokio::time::Instant::now()
        .checked_sub(level_interval)
        .unwrap_or_else(tokio::time::Instant::now);
    let mut pcm_filled = 0usize;
    let mut received_frame = false;

    loop {
        // ── Try to read one frame ─────────────────────────────────────────
        let read_ok: bool = if let Some(dur) = idle_timeout {
            match read_pcm_frame(&mut src, pcm_buf, &mut pcm_filled, Some(dur)).await {
                FrameRead::Frame => true,
                FrameRead::Eof => {
                    info!(stream = stream_id, "Audio input closed");
                    return ReaderEnd::Eof { received_frame };
                }
                FrameRead::Error(e) => {
                    warn!(stream = stream_id, "Audio input read error: {e}");
                    return ReaderEnd::Error {
                        error: e,
                        received_frame,
                    };
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
                        // Emit silence frames at chunk_ms intervals until data returns.
                        let mut tick =
                            tokio::time::interval(Duration::from_millis(chunk_ms as u64));
                        tick.tick().await; // discard immediate first tick
                        loop {
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
                                            return ReaderEnd::Eof { received_frame };
                                        }
                                        FrameRead::Error(e) => {
                                            warn!(stream = stream_id, "Audio input read error while idle: {e}");
                                            return ReaderEnd::Error {
                                                error: e,
                                                received_frame,
                                            };
                                        }
                                        FrameRead::Idle => unreachable!("idle is disabled while waiting for resumed audio"),
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
            match read_pcm_frame(&mut src, pcm_buf, &mut pcm_filled, None).await {
                FrameRead::Frame => true,
                FrameRead::Eof => {
                    info!(stream = stream_id, "Audio input closed");
                    return ReaderEnd::Eof { received_frame };
                }
                FrameRead::Error(e) => {
                    warn!(stream = stream_id, "Audio input read error: {e}");
                    return ReaderEnd::Error {
                        error: e,
                        received_frame,
                    };
                }
                FrameRead::Idle => unreachable!("idle is disabled for blocking reads"),
            }
        };

        if !read_ok {
            continue;
        }
        received_frame = true;

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
}

enum ReaderEnd {
    Eof {
        received_frame: bool,
    },
    Error {
        error: io::Error,
        received_frame: bool,
    },
}

impl ReaderEnd {
    fn received_frame(&self) -> bool {
        match self {
            Self::Eof { received_frame } | Self::Error { received_frame, .. } => *received_frame,
        }
    }
}

enum FrameRead {
    Frame,
    Idle,
    Eof,
    Error(io::Error),
}

async fn read_pcm_frame<R: AsyncReadExt + Unpin>(
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
    use std::path::Path;
    use tokio::io::AsyncWriteExt;
    use tokio::time::timeout;
    use tokio_util::sync::CancellationToken;

    #[cfg(unix)]
    fn create_fifo(path: &Path) {
        let status = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .expect("mkfifo must be available on Unix test hosts");
        assert!(status.success());
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

    #[tokio::test]
    async fn reopening_file_reader_waits_for_a_change_before_replaying() {
        // This catches reopening a regular file at byte offset zero after EOF.
        // The test waits for a second recovery attempt, rather than racing the
        // first 50 ms delay, and asserts no duplicate audio was broadcast.
        let path = std::env::temp_dir().join(format!("sonium-reopen-{}", uuid::Uuid::new_v4()));
        let replacement_path = path.with_extension("replacement");
        let first = vec![0x11; 16];
        let replacement = vec![0x22; 16];
        std::fs::write(&path, &first).unwrap();

        let state = Arc::new(ServerState::new(
            Arc::new(sonium_control::EventBus::new()),
            None,
            vec![],
            vec![],
        ));
        state.register_stream(
            "reopen-test",
            None,
            "pcm",
            "800Hz/16bit/1ch",
            path.to_string_lossy(),
            1000,
            false,
            10,
            false,
            None,
            false,
        );

        let bc = Arc::new(Broadcaster::new("reopen-test", 1000));
        let mut audio = bc.subscribe();
        let mut events = state.events().subscribe();
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task_state = state.clone();
        let task_bc = bc.clone();
        let task_path = path.to_string_lossy().to_string();

        let task = tokio::spawn(async move {
            let mut encoder =
                make_encoder("pcm", sonium_common::SampleFormat::new(800, 16, 1)).unwrap();
            let mut pcm_buf = vec![0u8; 16];
            let mut enc_buf = Vec::new();
            run_reopening_reader(
                &task_path,
                &mut *encoder,
                task_bc,
                &mut pcm_buf,
                &mut enc_buf,
                "reopen-test",
                &task_state,
                None,
                false,
                10,
                task_cancel,
            )
            .await
        });

        let first_frame = timeout(Duration::from_secs(1), audio.recv())
            .await
            .expect("initial file frame should be published")
            .unwrap();
        assert!(first_frame.wire_bytes.ends_with(&first));

        loop {
            match timeout(Duration::from_secs(1), events.recv())
                .await
                .expect("EOF should publish a recovery status")
                .unwrap()
            {
                Event::StreamStatus {
                    status: StreamStatus::Recovering,
                    ..
                } => break,
                _ => continue,
            }
        }

        let stream = state
            .all_streams()
            .into_iter()
            .find(|stream| stream.id == "reopen-test")
            .unwrap();
        assert_eq!(stream.status, StreamStatus::Recovering);
        assert_eq!(
            stream.recovery,
            Some(sonium_control::state::StreamRecovery {
                attempt: 1,
                retry_in_ms: 50,
            })
        );

        loop {
            match timeout(Duration::from_secs(1), events.recv())
                .await
                .expect("unchanged file should schedule another recovery attempt")
                .unwrap()
            {
                Event::StreamStatus {
                    status: StreamStatus::Recovering,
                    ..
                } => {
                    let stream = state
                        .all_streams()
                        .into_iter()
                        .find(|stream| stream.id == "reopen-test")
                        .unwrap();
                    if stream
                        .recovery
                        .as_ref()
                        .is_some_and(|recovery| recovery.attempt >= 2)
                    {
                        break;
                    }
                }
                _ => continue,
            }
        }
        assert!(
            audio.try_recv().is_err(),
            "unchanged regular files must not be replayed from byte offset zero"
        );

        std::fs::write(&replacement_path, &replacement).unwrap();
        let original_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let preserved_mtime = std::fs::File::open(&replacement_path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(original_mtime))
            .is_ok();
        // Windows cannot rename over an existing destination. The supervisor
        // cannot observe this synchronous remove+rename pair until we await.
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&replacement_path, &path).unwrap();
        if preserved_mtime {
            assert_eq!(
                std::fs::metadata(&path).unwrap().modified().unwrap(),
                original_mtime
            );
        }

        let replacement_frame = timeout(Duration::from_secs(1), audio.recv())
            .await
            .expect("replacement file frame should be published")
            .unwrap();
        assert!(replacement_frame.wire_bytes.ends_with(&replacement));

        loop {
            match timeout(Duration::from_secs(1), events.recv())
                .await
                .expect("replacement EOF should begin a fresh recovery sequence")
                .unwrap()
            {
                Event::StreamStatus {
                    status: StreamStatus::Recovering,
                    ..
                } => {
                    let stream = state
                        .all_streams()
                        .into_iter()
                        .find(|stream| stream.id == "reopen-test")
                        .unwrap();
                    if stream
                        .recovery
                        .as_ref()
                        .is_some_and(|recovery| recovery.attempt == 1)
                    {
                        break;
                    }
                }
                _ => continue,
            }
        }

        cancel.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .expect("cancellation should interrupt reopening backoff")
            .unwrap()
            .unwrap();

        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fifo_open_without_a_writer_is_nonblocking_and_reader_cancellation_is_prompt() {
        // This catches tokio::fs::File::open: it uses spawn_blocking and blocks
        // in open(2) until a FIFO writer exists, even after its future is dropped.
        let path = std::env::temp_dir().join(format!("sonium-fifo-{}", uuid::Uuid::new_v4()));
        create_fifo(&path);

        let fifo_source = path.to_string_lossy().into_owned();
        let reader = timeout(Duration::from_secs(1), open_file_source(&fifo_source))
            .await
            .expect("opening a FIFO reader must not wait for a writer")
            .unwrap();
        drop(reader);

        let state = Arc::new(ServerState::new(
            Arc::new(sonium_control::EventBus::new()),
            None,
            vec![],
            vec![],
        ));
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task_path = path.to_string_lossy().to_string();
        let task = tokio::spawn(async move {
            let mut encoder =
                make_encoder("pcm", sonium_common::SampleFormat::new(800, 16, 1)).unwrap();
            let mut pcm_buf = vec![0u8; 16];
            let mut enc_buf = Vec::new();
            run_reopening_reader(
                &task_path,
                &mut *encoder,
                Arc::new(Broadcaster::new("fifo-cancel-test", 1000)),
                &mut pcm_buf,
                &mut enc_buf,
                "fifo-cancel-test",
                &state,
                None,
                false,
                10,
                task_cancel,
            )
            .await
        });

        tokio::task::yield_now().await;
        cancel.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .expect("cancellation must not wait for a FIFO writer")
            .unwrap()
            .unwrap();

        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fifo_reader_reopens_after_a_writer_disconnects() {
        // This catches losing FIFO recovery after switching to nonblocking
        // receivers: separate writers must publish frames across an EOF.
        use tokio::net::unix::pipe;

        let path =
            std::env::temp_dir().join(format!("sonium-fifo-reopen-{}", uuid::Uuid::new_v4()));
        create_fifo(&path);
        let first = vec![0x33; 16];
        let second = vec![0x44; 16];
        let state = Arc::new(ServerState::new(
            Arc::new(sonium_control::EventBus::new()),
            None,
            vec![],
            vec![],
        ));
        state.register_stream(
            "fifo-reopen-test",
            None,
            "pcm",
            "800Hz/16bit/1ch",
            path.to_string_lossy(),
            1000,
            false,
            10,
            false,
            None,
            false,
        );
        let bc = Arc::new(Broadcaster::new("fifo-reopen-test", 1000));
        let mut audio = bc.subscribe();
        let mut events = state.events().subscribe();
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task_state = state.clone();
        let task_bc = bc.clone();
        let task_path = path.to_string_lossy().to_string();
        let task = tokio::spawn(async move {
            let mut encoder =
                make_encoder("pcm", sonium_common::SampleFormat::new(800, 16, 1)).unwrap();
            let mut pcm_buf = vec![0u8; 16];
            let mut enc_buf = Vec::new();
            run_reopening_reader(
                &task_path,
                &mut *encoder,
                task_bc,
                &mut pcm_buf,
                &mut enc_buf,
                "fifo-reopen-test",
                &task_state,
                None,
                false,
                10,
                task_cancel,
            )
            .await
        });

        wait_for_stream_status(&mut events, StreamStatus::Playing).await;
        let mut writer = pipe::OpenOptions::new().open_sender(&path).unwrap();
        writer.write_all(&first).await.unwrap();
        drop(writer);
        let first_frame = timeout(Duration::from_secs(1), audio.recv())
            .await
            .expect("first FIFO writer should publish a frame")
            .unwrap();
        assert!(first_frame.wire_bytes.ends_with(&first));

        wait_for_stream_status(&mut events, StreamStatus::Recovering).await;
        wait_for_stream_status(&mut events, StreamStatus::Playing).await;
        let mut writer = pipe::OpenOptions::new().open_sender(&path).unwrap();
        writer.write_all(&second).await.unwrap();
        drop(writer);
        let second_frame = timeout(Duration::from_secs(1), audio.recv())
            .await
            .expect("second FIFO writer should publish after reopening")
            .unwrap();
        assert!(second_frame.wire_bytes.ends_with(&second));

        cancel.cancel();
        timeout(Duration::from_secs(1), task)
            .await
            .expect("FIFO reader cancellation should finish")
            .unwrap()
            .unwrap();
        std::fs::remove_file(path).unwrap();
    }

    async fn wait_for_stream_status(
        events: &mut tokio::sync::broadcast::Receiver<Event>,
        expected: StreamStatus,
    ) {
        loop {
            match timeout(Duration::from_secs(1), events.recv())
                .await
                .expect("stream status transition should arrive")
                .unwrap()
            {
                Event::StreamStatus { status, .. } if status == expected => return,
                _ => continue,
            }
        }
    }

    #[test]
    fn file_fingerprint_keeps_creation_identity_when_available() {
        // This catches non-Unix fingerprints that only use size and mtime,
        // which can miss a same-sized replacement with preserved timestamps.
        let path =
            std::env::temp_dir().join(format!("sonium-fingerprint-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"fingerprint").unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        let fingerprint = FileFingerprint::from_metadata(&metadata);
        assert_eq!(fingerprint.created, metadata.created().ok());
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn not_a_directory_source_is_terminal_error_not_recovery() {
        // This catches treating a malformed path as a transient missing producer.
        let parent = std::env::temp_dir().join(format!("sonium-notadir-{}", uuid::Uuid::new_v4()));
        std::fs::write(&parent, b"not a directory").unwrap();
        let source = parent.join("child");
        let state = Arc::new(ServerState::new(
            Arc::new(sonium_control::EventBus::new()),
            None,
            vec![],
            vec![],
        ));
        state.register_stream(
            "notadir-test",
            None,
            "pcm",
            "800Hz/16bit/1ch",
            source.to_string_lossy(),
            1000,
            false,
            10,
            false,
            None,
            false,
        );
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task_state = state.clone();
        let task_source = source.to_string_lossy().to_string();
        let mut task = tokio::spawn(async move {
            let mut encoder =
                make_encoder("pcm", sonium_common::SampleFormat::new(800, 16, 1)).unwrap();
            let mut pcm_buf = vec![0u8; 16];
            let mut enc_buf = Vec::new();
            run_reopening_reader(
                &task_source,
                &mut *encoder,
                Arc::new(Broadcaster::new("notadir-test", 1000)),
                &mut pcm_buf,
                &mut enc_buf,
                "notadir-test",
                &task_state,
                None,
                false,
                10,
                task_cancel,
            )
            .await
        });

        let completed = timeout(Duration::from_secs(1), &mut task).await;
        let result = match completed {
            Ok(result) => result.unwrap(),
            Err(_) => {
                cancel.cancel();
                task.await.unwrap()
            }
        };
        assert!(result.is_err(), "NotADirectory must not retry forever");
        let stream = state
            .all_streams()
            .into_iter()
            .find(|stream| stream.id == "notadir-test")
            .unwrap();
        assert_eq!(stream.status, StreamStatus::Error);
        assert_eq!(stream.recovery, None);

        std::fs::remove_file(parent).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_loop_source_is_terminal_error_not_recovery() {
        // This catches a permanent path configuration error that must not spin
        // forever under the same retry policy as a missing producer.
        use std::os::unix::fs::symlink;

        let source = std::env::temp_dir().join(format!("sonium-loop-{}", uuid::Uuid::new_v4()));
        symlink(&source, &source).unwrap();
        let state = Arc::new(ServerState::new(
            Arc::new(sonium_control::EventBus::new()),
            None,
            vec![],
            vec![],
        ));
        state.register_stream(
            "loop-test",
            None,
            "pcm",
            "800Hz/16bit/1ch",
            source.to_string_lossy(),
            1000,
            false,
            10,
            false,
            None,
            false,
        );
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task_state = state.clone();
        let task_source = source.to_string_lossy().to_string();
        let mut task = tokio::spawn(async move {
            let mut encoder =
                make_encoder("pcm", sonium_common::SampleFormat::new(800, 16, 1)).unwrap();
            let mut pcm_buf = vec![0u8; 16];
            let mut enc_buf = Vec::new();
            run_reopening_reader(
                &task_source,
                &mut *encoder,
                Arc::new(Broadcaster::new("loop-test", 1000)),
                &mut pcm_buf,
                &mut enc_buf,
                "loop-test",
                &task_state,
                None,
                false,
                10,
                task_cancel,
            )
            .await
        });

        let completed = timeout(Duration::from_secs(1), &mut task).await;
        let result = match completed {
            Ok(result) => result.unwrap(),
            Err(_) => {
                cancel.cancel();
                task.await.unwrap()
            }
        };
        assert!(result.is_err(), "symlink loops must not retry forever");
        let stream = state
            .all_streams()
            .into_iter()
            .find(|stream| stream.id == "loop-test")
            .unwrap();
        assert_eq!(stream.status, StreamStatus::Error);
        assert_eq!(stream.recovery, None);

        std::fs::remove_file(source).unwrap();
    }
}
