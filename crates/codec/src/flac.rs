//! FLAC encoder and decoder.
//!
//! - **Encoder** (`FlacEncoder`): uses `flacenc` (pure Rust) to compress each
//!   PCM frame into a FLAC frame.  Each call to `encode()` produces one
//!   self-contained FLAC frame suitable for a `WireChunk`.
//!
//! - **Decoder** (`FlacDecoder`): uses `claxon` to decode individual FLAC
//!   frames back to interleaved i16 PCM.

use std::io::Cursor;

use crate::traits::{Decoder, Encoder};
use sonium_common::{SampleFormat, SoniumError, error::Result};
use sonium_protocol::messages::codec_header::{flac_codec_header, parse_flac_codec_header};
use flacenc::error::Verify;
use flacenc::component::BitRepr;

// ── Encoder ──────────────────────────────────────────────────────────────────

/// FLAC encoder — compresses interleaved i16 PCM into FLAC frames.
///
/// Each `encode()` call takes exactly one frame's worth of PCM samples
/// (block_size × channels) and produces one FLAC frame.
///
/// The encoder builds a minimal FLAC stream for each block and extracts
/// just the frame bytes for efficient per-chunk transport over the wire.
pub struct FlacEncoder {
    fmt:        SampleFormat,
    block_size: u32,
    enc_config: flacenc::error::Verified<flacenc::config::Encoder>,
}

impl FlacEncoder {
    /// Create a new FLAC encoder for the given sample format.
    ///
    /// `block_size` is the number of samples per channel per frame.
    /// Typical values: 960 (20 ms @ 48 kHz), 1024.
    pub fn new(fmt: SampleFormat) -> Result<Self> {
        let block_size = fmt.frames_for_ms(20.0) as u32;
        let enc_config = flacenc::config::Encoder::default()
            .into_verified()
            .map_err(|e| SoniumError::Codec(format!("flac config: {e:?}")))?;

        Ok(Self { fmt, block_size, enc_config })
    }
}

impl Encoder for FlacEncoder {
    fn encode(&mut self, pcm: &[i16], output: &mut Vec<u8>) -> Result<()> {
        let channels   = self.fmt.channels as usize;
        let bits       = self.fmt.bits as usize;
        let rate       = self.fmt.rate;
        let block_size = self.block_size as usize;

        // flacenc expects i32 samples in channel-interleaved order.
        let samples_i32: Vec<i32> = pcm.iter().map(|&s| s as i32).collect();

        let source = flacenc::source::MemSource::from_samples(
            &samples_i32,
            channels,
            bits,
            rate as usize,
        );

        let flac_stream = flacenc::encode_with_fixed_block_size(
            &self.enc_config,
            source,
            block_size,
        ).map_err(|e| SoniumError::Codec(format!("flac encode: {e:?}")))?;

        // Serialize the FLAC stream to bytes.
        let mut sink = flacenc::bitsink::ByteSink::new();
        flac_stream.write(&mut sink)
            .map_err(|_| SoniumError::Codec("flac bitstream write failed".into()))?;
        let full_bytes = sink.as_slice();

        // The output is a complete FLAC stream:
        //   4 bytes  "fLaC" magic
        //   4+ bytes STREAMINFO metadata block (4-byte block header + 34-byte body = 38 bytes)
        //   N bytes  FLAC frame(s)
        //
        // We need to extract just the frame data (skip the stream header).
        // STREAMINFO is always the first metadata block.  Its block header's
        // high bit indicates "last metadata block".  Standard layout is 42 bytes
        // header total (4 magic + 38 STREAMINFO).
        let header_end = find_first_frame_offset(full_bytes)
            .ok_or_else(|| SoniumError::Codec("no FLAC frame found in encoded output".into()))?;

        output.extend_from_slice(&full_bytes[header_end..]);
        Ok(())
    }

    fn sample_format(&self) -> SampleFormat { self.fmt }
    fn codec_name(&self) -> &'static str { "flac" }

    fn codec_header(&self) -> Vec<u8> {
        flac_codec_header(self.fmt.rate, self.fmt.bits, self.fmt.channels, self.block_size)
    }
}

/// Find the byte offset of the first FLAC frame in a raw FLAC stream.
///
/// Scans past the "fLaC" magic and all metadata blocks.
fn find_first_frame_offset(data: &[u8]) -> Option<usize> {
    if data.len() < 4 || &data[0..4] != b"fLaC" {
        return None;
    }
    let mut pos = 4; // skip magic
    loop {
        if pos + 4 > data.len() {
            return None;
        }
        let is_last = (data[pos] & 0x80) != 0;
        let block_len = u32::from_be_bytes([0, data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4 + block_len; // skip block header + body
        if is_last {
            break;
        }
    }
    if pos <= data.len() { Some(pos) } else { None }
}

// ── Decoder ──────────────────────────────────────────────────────────────────

/// FLAC decoder — decodes individual FLAC frames using `claxon`.
///
/// For each `decode()` call, the input bytes are expected to be a single
/// raw FLAC frame (as produced by `FlacEncoder`).  The decoder wraps it
/// in a minimal FLAC stream (magic + STREAMINFO + frame) so that `claxon`
/// can parse it.
pub struct FlacDecoder {
    fmt:        SampleFormat,
    block_size: u32,
}

impl FlacDecoder {
    /// Create from the parsed FLAC codec header.
    pub fn from_header(header_data: &[u8]) -> Result<Self> {
        let (rate, bits, channels, block_size) = parse_flac_codec_header(header_data)?;
        Ok(Self {
            fmt: SampleFormat::new(rate, bits, channels),
            block_size,
        })
    }
}

impl Decoder for FlacDecoder {
    fn decode(&mut self, input: &[u8], output: &mut Vec<i16>) -> Result<()> {
        // Build a minimal FLAC stream: magic + STREAMINFO + frame data.
        let stream_bytes = build_minimal_flac_stream(
            &self.fmt, self.block_size, input,
        );

        let cursor = Cursor::new(&stream_bytes);
        let mut reader = claxon::FlacReader::new(cursor)
            .map_err(|e| SoniumError::Codec(format!("flac reader init: {e}")))?;

        // Decode all samples from the frame.
        let bps      = reader.streaminfo().bits_per_sample;

        // Read all blocks (should be exactly one frame).
        let mut frame_reader = reader.blocks();
        let mut block_buf = claxon::Block::empty();
        while let Some(block) = frame_reader
            .read_next_or_eof(block_buf.into_buffer())
            .map_err(|e| SoniumError::Codec(format!("flac decode: {e}")))?
        {
            let n_frames  = block.duration() as usize;  // per-channel sample count
            let n_channels = block.channels() as u32;
            // Interleave channel samples.
            for i in 0..n_frames {
                for ch in 0..n_channels {
                    let sample = block.sample(ch, i as u32);
                    // claxon returns i32; convert to i16 (samples are bps-bit).
                    let s16 = if bps <= 16 {
                        sample as i16
                    } else {
                        (sample >> (bps - 16)) as i16
                    };
                    output.push(s16);
                }
            }
            block_buf = block;
        }

        Ok(())
    }

    fn sample_format(&self) -> SampleFormat { self.fmt }
}

/// Build a minimal valid FLAC stream (magic + STREAMINFO + raw frame data).
///
/// This lets `claxon::FlacReader` decode individual frames that were
/// transmitted as bare frame bytes over the wire.
fn build_minimal_flac_stream(fmt: &SampleFormat, block_size: u32, frame_data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(42 + frame_data.len());

    // "fLaC" magic
    out.extend_from_slice(b"fLaC");

    // STREAMINFO metadata block header (4 bytes)
    // Bit 7 of first byte = 1 (last metadata block)
    // Bits 6..0 = 0 (STREAMINFO type)
    // Next 3 bytes = 34 (STREAMINFO body length)
    out.push(0x80); // last block, type 0
    out.push(0x00);
    out.push(0x00);
    out.push(34);   // STREAMINFO is always 34 bytes

    // STREAMINFO body (34 bytes)
    // Min/max block size (2 + 2 bytes)
    out.extend_from_slice(&(block_size as u16).to_be_bytes()); // min block size
    out.extend_from_slice(&(block_size as u16).to_be_bytes()); // max block size

    // Min/max frame size (3 + 3 bytes) — 0 = unknown
    out.extend_from_slice(&[0, 0, 0]); // min frame size
    out.extend_from_slice(&[0, 0, 0]); // max frame size

    // Sample rate (20 bits) + channels-1 (3 bits) + bps-1 (5 bits) + total samples (36 bits)
    // = 8 bytes total
    let rate     = fmt.rate;
    let channels = fmt.channels as u32;
    let bps      = fmt.bits as u32;

    // Bits layout (MSB first):
    // [19:0]  sample rate
    // [22:20] channels - 1
    // [27:23] bits per sample - 1
    // [63:28] total samples (0 = unknown for streaming)
    let packed: u64 = ((rate as u64) << 44)
        | (((channels - 1) as u64) << 41)
        | (((bps - 1) as u64) << 36);
    out.extend_from_slice(&packed.to_be_bytes());

    // MD5 signature (16 bytes) — all zeros (not computed for streaming)
    out.extend_from_slice(&[0u8; 16]);

    // Raw FLAC frame data
    out.extend_from_slice(frame_data);

    out
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fmt() -> SampleFormat {
        SampleFormat::new(48_000, 16, 2)
    }

    #[test]
    fn encoder_produces_output() {
        let fmt = make_fmt();
        let mut enc = FlacEncoder::new(fmt).unwrap();
        let block = fmt.frames_for_ms(20.0) * fmt.channels as usize;
        let pcm = vec![0i16; block]; // silence
        let mut out = Vec::new();
        enc.encode(&pcm, &mut out).unwrap();
        assert!(!out.is_empty(), "FLAC encoder should produce output");
    }

    #[test]
    fn round_trip_silence() {
        let fmt = make_fmt();
        let mut enc = FlacEncoder::new(fmt).unwrap();
        let mut dec = FlacDecoder::from_header(&enc.codec_header()).unwrap();

        let block = fmt.frames_for_ms(20.0) * fmt.channels as usize;
        let pcm = vec![0i16; block];

        let mut encoded = Vec::new();
        enc.encode(&pcm, &mut encoded).unwrap();

        let mut decoded = Vec::new();
        dec.decode(&encoded, &mut decoded).unwrap();

        assert_eq!(decoded.len(), pcm.len());
        // FLAC is lossless — all samples should be identical.
        assert_eq!(decoded, pcm);
    }

    #[test]
    fn round_trip_signal() {
        let fmt = make_fmt();
        let mut enc = FlacEncoder::new(fmt).unwrap();
        let mut dec = FlacDecoder::from_header(&enc.codec_header()).unwrap();

        let block = fmt.frames_for_ms(20.0) * fmt.channels as usize;
        let pcm: Vec<i16> = (0..block)
            .map(|i| ((i as f32 * 0.05).sin() * 10000.0) as i16)
            .collect();

        let mut encoded = Vec::new();
        enc.encode(&pcm, &mut encoded).unwrap();

        let mut decoded = Vec::new();
        dec.decode(&encoded, &mut decoded).unwrap();

        // FLAC is lossless.
        assert_eq!(decoded, pcm);
    }

    #[test]
    fn codec_name_is_flac() {
        let enc = FlacEncoder::new(make_fmt()).unwrap();
        assert_eq!(enc.codec_name(), "flac");
    }

    #[test]
    fn codec_header_round_trip() {
        let enc = FlacEncoder::new(make_fmt()).unwrap();
        let hdr = enc.codec_header();
        let (rate, bits, ch, bs) = parse_flac_codec_header(&hdr).unwrap();
        assert_eq!(rate, 48_000);
        assert_eq!(bits, 16);
        assert_eq!(ch, 2);
        assert_eq!(bs, 960); // 20ms @ 48kHz
    }

    #[test]
    fn find_frame_offset_valid_stream() {
        // Build a minimal fLaC stream with an empty STREAMINFO and verify offset.
        let mut data = Vec::new();
        data.extend_from_slice(b"fLaC");
        data.push(0x80); // last block, type 0
        data.push(0);
        data.push(0);
        data.push(34); // body length
        data.extend_from_slice(&[0u8; 34]); // STREAMINFO body
        data.extend_from_slice(b"FRAME");   // fake frame data

        assert_eq!(find_first_frame_offset(&data), Some(42));
    }
}
