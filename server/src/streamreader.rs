use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tracing::{debug, info, warn};
use bytes::Bytes;

use sonium_common::config::ServerConfig;
use sonium_protocol::{
    Timestamp,
    messages::{Message, CodecHeader, WireChunk},
};
use sonium_codec::make_encoder;

use crate::broadcaster::Broadcaster;

/// Read PCM from stdin (or a named pipe), encode, and broadcast to all sessions.
///
/// Input format is raw interleaved i16 LE PCM matching the configured sample format.
/// This mirrors `snapfifo` — pipe `ffmpeg`, `spotifyd`, or any PCM source into stdin.
pub async fn run(bc: Arc<Broadcaster>, cfg: ServerConfig) -> anyhow::Result<()> {
    let fmt    = cfg.stream.sample_format;
    let codec  = cfg.stream.codec.as_str();

    let mut encoder = make_encoder(codec, fmt)
        .map_err(|e| anyhow::anyhow!("encoder init: {e}"))?;

    // Build and cache the CodecHeader message for late-joining clients
    let codec_hdr_msg = Message::CodecHeader(CodecHeader::new(
        encoder.codec_name(),
        encoder.codec_header(),
    ));
    bc.set_codec_header(Bytes::from(codec_hdr_msg.encode()));

    info!(codec, format = %fmt, "Stream reader started — reading PCM from stdin");

    // Frame size: 20ms of PCM samples (interleaved i16)
    let frame_samples = fmt.frames_for_ms(20.0) * fmt.channels as usize;
    let frame_bytes   = frame_samples * 2;

    let mut stdin   = tokio::io::stdin();
    let mut pcm_buf = vec![0u8; frame_bytes];
    let mut enc_buf: Vec<u8> = Vec::new();

    loop {
        // Read exactly one PCM frame from input
        match stdin.read_exact(&mut pcm_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                info!("Input stream closed — stream reader stopping");
                break;
            }
            Err(e) => {
                warn!("Read error: {e}");
                break;
            }
        }

        // Convert bytes → i16 samples
        let pcm: Vec<i16> = pcm_buf
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();

        // Encode
        enc_buf.clear();
        if let Err(e) = encoder.encode(&pcm, &mut enc_buf) {
            warn!("Encode error: {e}");
            continue;
        }

        // Build WireChunk and serialize
        let ts    = Timestamp::now();
        let chunk = WireChunk::new(ts, enc_buf.clone());
        let wire  = Message::WireChunk(chunk).encode();

        debug!(bytes = wire.len(), "Broadcasting frame");
        bc.publish(Bytes::from(wire));
    }

    Ok(())
}
