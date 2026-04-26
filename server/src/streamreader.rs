use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{info, warn, debug};
use bytes::Bytes;

use sonium_common::config::StreamSource;
use sonium_protocol::{
    Timestamp,
    messages::{Message, CodecHeader, WireChunk},
};
use sonium_codec::make_encoder;

use crate::broadcaster::Broadcaster;
use tracing::instrument;

/// Read PCM from stdin, a named FIFO, or an external process, encode, and broadcast.
///
/// Source format:
/// - `"-"` — reads from stdin
/// - `pipe:///path/to/cmd?arg1&arg2` — spawns child process
/// - anything else — opens path as a file/FIFO
///
/// Input is raw interleaved i16 LE PCM matching `stream.sample_format`.
#[instrument(skip(bc), fields(stream_id = %stream.id, source = %stream.source, codec = %stream.codec))]
pub async fn run(bc: Arc<Broadcaster>, stream: StreamSource) -> anyhow::Result<()> {
    let fmt   = stream.sample_format;
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
    let frame_bytes   = frame_samples * 2; // i16 = 2 bytes
    let mut pcm_buf   = vec![0u8; frame_bytes];
    let mut enc_buf: Vec<u8> = Vec::new();

    if stream.source == "-" {
        run_reader(tokio::io::stdin(), &mut *encoder, bc, &mut pcm_buf, &mut enc_buf, &stream.id).await
    } else if stream.source.starts_with("pipe://") {
        run_pipe(&stream.source, &mut *encoder, bc, &mut pcm_buf, &mut enc_buf, &stream.id).await
    } else {
        let file = tokio::fs::File::open(&stream.source).await
            .map_err(|e| anyhow::anyhow!("[{}] open {}: {e}", stream.id, stream.source))?;
        run_reader(file, &mut *encoder, bc, &mut pcm_buf, &mut enc_buf, &stream.id).await
    }
}

/// Parse a `pipe://` URI and spawn the child process.
///
/// Format: `pipe:///absolute/path/to/command?arg1&arg2&arg3`
/// - The path component is the executable.
/// - Query parameters (split by `&`) are the arguments.
///
/// Examples:
///   `pipe:///usr/bin/ffmpeg?-re&-i&/music/song.flac&-f&s16le&-ar&48000&-ac&2&-`
///   `pipe:///usr/bin/arecord?-f&S16_LE&-r&48000&-c&2&-t&raw`
async fn run_pipe(
    uri:       &str,
    encoder:   &mut (dyn sonium_codec::Encoder + Send),
    bc:        Arc<Broadcaster>,
    pcm_buf:   &mut Vec<u8>,
    enc_buf:   &mut Vec<u8>,
    stream_id: &str,
) -> anyhow::Result<()> {
    let (cmd, args) = parse_pipe_uri(uri)?;

    info!(
        stream = stream_id,
        command = %cmd,
        args = ?args,
        "Spawning external process"
    );

    let mut child = Command::new(&cmd)
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow::anyhow!("[{stream_id}] spawn `{cmd}`: {e}"))?;

    let stdout = child.stdout.take()
        .ok_or_else(|| anyhow::anyhow!("[{stream_id}] no stdout from child"))?;

    let result = run_reader(stdout, encoder, bc, pcm_buf, enc_buf, stream_id).await;

    // Ensure the child is cleaned up
    match child.try_wait() {
        Ok(Some(status)) => {
            info!("[{stream_id}] Process exited with {status}");
        }
        Ok(None) => {
            info!("[{stream_id}] Killing child process");
            let _ = child.kill().await;
        }
        Err(e) => {
            warn!("[{stream_id}] Error checking child status: {e}");
        }
    }

    result
}

/// Parse `pipe:///path/to/cmd?arg1&arg2` into (command, args).
fn parse_pipe_uri(uri: &str) -> anyhow::Result<(String, Vec<String>)> {
    let rest = uri.strip_prefix("pipe://")
        .ok_or_else(|| anyhow::anyhow!("not a pipe:// URI: {uri}"))?;

    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None         => (rest, None),
    };

    if path.is_empty() {
        anyhow::bail!("pipe:// URI has empty command path: {uri}");
    }

    let args: Vec<String> = query
        .map(|q| q.split('&').map(String::from).collect())
        .unwrap_or_default();

    Ok((path.to_owned(), args))
}

async fn run_reader<R: AsyncReadExt + Unpin>(
    mut src:     R,
    encoder:     &mut (dyn sonium_codec::Encoder + Send),
    bc:          Arc<Broadcaster>,
    pcm_buf:     &mut Vec<u8>,
    enc_buf:     &mut Vec<u8>,
    stream_id:   &str,
) -> anyhow::Result<()> {
    loop {
        match src.read_exact(pcm_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                info!("[{stream_id}] Input closed — reader stopping");
                break;
            }
            Err(e) => {
                warn!("[{stream_id}] Read error: {e}");
                break;
            }
        }

        let pcm: Vec<i16> = pcm_buf
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();

        enc_buf.clear();
        if let Err(e) = encoder.encode(&pcm, enc_buf) {
            warn!("[{stream_id}] Encode error: {e}");
            continue;
        }

        let ts    = Timestamp::now();
        let chunk = WireChunk::new(ts, enc_buf.clone());
        let wire  = Message::WireChunk(chunk).encode();

        debug!(stream = stream_id, bytes = wire.len(), "Broadcasting frame");
        bc.publish(Bytes::from(wire));
    }
    Ok(())
}
