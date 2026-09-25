//! One-shot media worker: announcements, TTS and `play_media`.
//!
//! For each request the worker creates a temporary broadcaster, routes the
//! target clients to it via a runtime stream override, decodes the URL with
//! ffmpeg into paced PCM chunks and, once the audio (plus the clients' buffer)
//! has played out, releases the override so every client returns to its
//! group's stream.

use bytes::Bytes;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{debug, info, warn};

use sonium_codec::make_encoder;
use sonium_common::config::ServerConfig;
use sonium_common::SampleFormat;
use sonium_control::media::{MediaSession, PlayMediaRequest};
use sonium_control::ServerState;
use sonium_protocol::{
    messages::{CodecHeader, Message, WireChunk},
    Timestamp,
};

use crate::broadcaster::{self, Broadcaster, BroadcasterRegistry};
use crate::streamreader::{read_pcm_frame, stream_chunk_ms, FrameRead, Pacer};

/// Silence published before the media starts, so every client has switched
/// streams and re-opened its output before the first real sample is due.
const LEAD_IN_MS: u64 = 300;
/// Extra silence after the end, on top of the stream buffer, before clients
/// are switched back (switching resets the client's jitter buffer).
const TAIL_MARGIN_MS: u64 = 300;
/// Give up if the source produces no audio at all for this long.
const FIRST_AUDIO_TIMEOUT: Duration = Duration::from_secs(20);
/// Give up if an already-playing source stalls for this long.
const STALL_TIMEOUT: Duration = Duration::from_secs(10);

/// Spawn the media worker and return the request channel for
/// [`ServerState::set_media_backend`].
pub fn spawn_worker(
    registry: Arc<BroadcasterRegistry>,
    state: Arc<ServerState>,
    cfg: ServerConfig,
) -> mpsc::Sender<PlayMediaRequest> {
    let (tx, mut rx) = mpsc::channel::<PlayMediaRequest>(16);
    tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            let result = start(req.url, req.client_ids, req.volume, &registry, &state, &cfg);
            let _ = req.respond_to.send(result);
        }
    });
    tx
}

/// Codec and format for the temporary stream: reuse the first target's
/// current stream settings so decoders stay the same, else Opus 48 kHz.
fn media_codec(
    state: &ServerState,
    cfg: &ServerConfig,
    client_ids: &[String],
) -> (String, SampleFormat, u32, u32) {
    let base = client_ids
        .iter()
        .find_map(|cid| state.client_group_stream_id(cid))
        .and_then(|sid| cfg.streams.iter().find(|s| s.id == sid))
        .filter(|s| !s.source.starts_with("meta://"));

    let (codec, fmt) = match base {
        Some(s)
            if matches!(s.codec.as_str(), "opus" | "flac" | "pcm")
                && s.sample_format.bits == 16
                && (1..=2).contains(&s.sample_format.channels) =>
        {
            (s.codec.clone(), s.sample_format)
        }
        _ => ("opus".to_owned(), SampleFormat::new(48_000, 16, 2)),
    };
    let (buffer_ms, chunk_ms) = match base {
        Some(s) => (cfg.effective_buffer_ms(s), cfg.effective_chunk_ms(s)),
        None => (cfg.server.audio.buffer_ms, cfg.server.audio.chunk_ms),
    };
    (codec, fmt, buffer_ms, chunk_ms)
}

fn start(
    url: String,
    client_ids: Vec<String>,
    volume: Option<u8>,
    registry: &Arc<BroadcasterRegistry>,
    state: &Arc<ServerState>,
    cfg: &ServerConfig,
) -> Result<MediaSession, String> {
    let (codec, fmt, buffer_ms, chunk_ms) = media_codec(state, cfg, &client_ids);

    let id = format!("media-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let mut stream_cfg = sonium_common::config::StreamSource {
        id: id.clone(),
        codec: codec.clone(),
        sample_format: fmt,
        chunk_ms: Some(chunk_ms),
        buffer_ms: Some(buffer_ms),
        ..Default::default()
    };
    let chunk_ms = stream_chunk_ms(&stream_cfg);
    stream_cfg.chunk_ms = Some(chunk_ms);

    let encoder = make_encoder(&codec, fmt).map_err(|e| format!("encoder init failed: {e}"))?;
    let bc = Arc::new(Broadcaster::new(&id, buffer_ms));
    let header = Message::CodecHeader(CodecHeader::new(
        encoder.codec_name(),
        encoder.codec_header(),
    ));
    bc.set_codec_header(Bytes::from(header.encode()));
    broadcaster::register(registry, bc.clone());

    let session = MediaSession {
        id: id.clone(),
        url: url.clone(),
        client_ids,
        volume,
        started_at: chrono::Utc::now(),
    };
    info!(media = %id, url = %url, clients = ?session.client_ids, codec = %codec, "Starting media playback");

    let registry = registry.clone();
    let state = state.clone();
    let pending = session.clone();
    tokio::spawn(async move {
        if let Err(e) = play(
            &url, pending, encoder, fmt, chunk_ms, buffer_ms, &bc, &state,
        )
        .await
        {
            warn!(media = %id, "Media playback failed: {e}");
        }
        state.end_media(&id);
        broadcaster::unregister(&registry, &id);
        info!(media = %id, "Media playback finished");
    });

    Ok(session)
}

#[allow(clippy::too_many_arguments)]
async fn play(
    url: &str,
    session: MediaSession,
    mut encoder: Box<dyn sonium_codec::Encoder + Send>,
    fmt: SampleFormat,
    chunk_ms: u32,
    buffer_ms: u32,
    bc: &Broadcaster,
    state: &ServerState,
) -> anyhow::Result<()> {
    let frame_samples = fmt.frames_for_ms(f64::from(chunk_ms)) * fmt.channels as usize;
    let mut pcm_buf = vec![0u8; frame_samples * 2];
    let silence = vec![0i16; frame_samples];
    let mut enc_buf = Vec::new();
    let mut pacer = Pacer::new(chunk_ms, buffer_ms);

    let publish = |encoder: &mut Box<dyn sonium_codec::Encoder + Send>,
                   enc_buf: &mut Vec<u8>,
                   pcm: &[i16],
                   ts_us: i64|
     -> anyhow::Result<()> {
        enc_buf.clear();
        encoder
            .encode(pcm, enc_buf)
            .map_err(|e| anyhow::anyhow!("encode: {e}"))?;
        let chunk = WireChunk::new(Timestamp::from_micros(ts_us), enc_buf.clone());
        bc.publish(Bytes::from(Message::WireChunk(chunk).encode()));
        Ok(())
    };

    // ffmpeg decodes any common format (mp3, wav, flac, ogg, aac, HLS...) to
    // raw PCM. Only network protocols are allowed so a URL cannot read local
    // files through playlists or special ffmpeg protocols.
    let mut child = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-protocol_whitelist",
            "http,https,tcp,tls,crypto",
            "-i",
            url,
            "-vn",
            "-ac",
            &fmt.channels.to_string(),
            "-ar",
            &fmt.rate.to_string(),
            "-f",
            "s16le",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow::anyhow!("cannot start ffmpeg (is it installed?): {e}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("ffmpeg has no stdout"))?;
    let stderr_task = {
        let mut stderr = child.stderr.take();
        tokio::spawn(async move {
            let mut buf = Vec::with_capacity(4096);
            if let Some(ref mut reader) = stderr {
                let _ = AsyncReadExt::take(reader, 4096).read_to_end(&mut buf).await;
            }
            buf
        })
    };

    let media_id = session.id.clone();
    let media_id = media_id.as_str();
    let still_wanted = || state.media_client_count(media_id) > 0;
    let mut filled = 0usize;

    // Wait for the first decoded audio *before* switching any client, so a
    // broken URL or slow TTS generation never interrupts what is playing.
    let first = tokio::time::timeout(
        FIRST_AUDIO_TIMEOUT,
        read_pcm_frame(&mut stdout, &mut pcm_buf, &mut filled, None),
    )
    .await;
    let first_ok = matches!(first, Ok(FrameRead::Frame));
    if !first_ok {
        let _ = child.kill().await;
        let stderr = stderr_task.await.unwrap_or_default();
        let stderr = String::from_utf8_lossy(&stderr);
        let reason = match first {
            Err(_) => "timed out waiting for audio".to_owned(),
            Ok(_) if stderr.trim().is_empty() => "no audio decoded".to_owned(),
            Ok(_) => stderr.trim().to_owned(),
        };
        anyhow::bail!("{url}: {reason}");
    }
    let mut pending_frame = Some(pcm_buf.clone());

    state.begin_media(session.clone());

    // Lead-in silence while clients switch over and re-open their output.
    let lead_in_chunks = LEAD_IN_MS.div_ceil(u64::from(chunk_ms));
    for _ in 0..lead_in_chunks {
        let ts = pacer.wait_for_slot().await;
        publish(&mut encoder, &mut enc_buf, &silence, ts)?;
    }

    let mut last_audio = Instant::now();
    let mut frames = 0u64;
    let mut stopped_early = false;

    loop {
        if !still_wanted() {
            stopped_early = true;
            break;
        }
        if let Some(frame) = pending_frame.take() {
            frames += 1;
            let pcm: Vec<i16> = frame
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            let ts = pacer.wait_for_slot().await;
            publish(&mut encoder, &mut enc_buf, &pcm, ts)?;
            continue;
        }
        let (slot_ts, wait) = pacer.time_until_slot();
        // Give a late read a little grace before filling the slot with
        // silence (the slot stays on the timeline either way).
        let grace = Duration::from_millis(50);
        tokio::select! {
            biased;
            read = read_pcm_frame(&mut stdout, &mut pcm_buf, &mut filled, None) => match read {
                FrameRead::Frame => {
                    last_audio = Instant::now();
                    frames += 1;
                    let pcm: Vec<i16> = pcm_buf
                        .chunks_exact(2)
                        .map(|c| i16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    let ts = pacer.wait_for_slot().await;
                    publish(&mut encoder, &mut enc_buf, &pcm, ts)?;
                }
                FrameRead::Eof | FrameRead::Idle => break,
                FrameRead::Error(e) => {
                    warn!(media = %media_id, "Media read error: {e}");
                    break;
                }
            },
            _ = tokio::time::sleep(wait + grace) => {
                let waited = last_audio.elapsed();
                if waited > STALL_TIMEOUT {
                    warn!(media = %media_id, waited_ms = waited.as_millis(), "Media source stalled; giving up");
                    break;
                }
                pacer.advance(slot_ts);
                publish(&mut encoder, &mut enc_buf, &silence, slot_ts)?;
            }
        }
    }

    let _ = child.kill().await;
    let stderr = stderr_task.await.unwrap_or_default();
    let stderr = String::from_utf8_lossy(&stderr);
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        debug!(media = %media_id, stderr, "ffmpeg reported");
    }
    info!(
        media = %media_id,
        duration_ms = frames * u64::from(chunk_ms),
        "Media decoded"
    );

    if stopped_early {
        return Ok(());
    }

    // Let the clients play out their buffer before they switch back.
    let tail_chunks = (u64::from(buffer_ms) + TAIL_MARGIN_MS).div_ceil(u64::from(chunk_ms));
    for _ in 0..tail_chunks {
        if !still_wanted() {
            break;
        }
        let ts = pacer.wait_for_slot().await;
        publish(&mut encoder, &mut enc_buf, &silence, ts)?;
    }
    Ok(())
}
