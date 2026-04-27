//! AAC audio codec — decoder only (pure Rust via `symphonia-codec-aac`).
//!
//! ## Wire header format for AAC
//!
//! ```text
//! u32  magic       = 0x41414320  ('AAC ')
//! u32  sample_rate
//! u16  bits_per_sample
//! u16  channel_count
//! ```
//! Total: 12 bytes
//!
//! AAC *encoding* is not supported; the server should use Opus or FLAC.
//! The decoder exists for Sonium clients connecting to servers that stream AAC.

use symphonia_core::codecs::{CodecParameters, Decoder as SymphDecoder, DecoderOptions, CODEC_TYPE_AAC};
use symphonia_core::audio::AudioBufferRef;
use symphonia_core::formats::Packet;
use symphonia_core::errors::Error as SymphError;
use symphonia_codec_aac::AacDecoder as SymphAacDecoder;

use sonium_common::{SampleFormat, SoniumError, error::Result};

use crate::traits::Decoder;

const AAC_MAGIC: u32 = 0x4141_4320; // 'AAC '

// ── Wire header helpers ────────────────────────────────────────────────────────

/// Build the 12-byte AAC codec header.
pub fn aac_codec_header(rate: u32, bits: u16, channels: u16) -> Vec<u8> {
    let mut h = Vec::with_capacity(12);
    h.extend_from_slice(&AAC_MAGIC.to_le_bytes());
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&bits.to_le_bytes());
    h.extend_from_slice(&channels.to_le_bytes());
    h
}

/// Parse a 12-byte AAC codec header → `(rate, bits, channels)`.
pub fn parse_aac_codec_header(data: &[u8]) -> Result<(u32, u16, u16)> {
    if data.len() < 12 {
        return Err(SoniumError::Protocol("aac header too short".into()));
    }
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != AAC_MAGIC {
        return Err(SoniumError::Protocol(format!("bad aac magic: {magic:#010x}")));
    }
    let rate     = u32::from_le_bytes([data[4],  data[5],  data[6],  data[7]]);
    let bits     = u16::from_le_bytes([data[8],  data[9]]);
    let channels = u16::from_le_bytes([data[10], data[11]]);
    Ok((rate, bits, channels))
}

// ── Decoder ───────────────────────────────────────────────────────────────────

pub struct AacDecoder {
    inner: SymphAacDecoder,
    fmt:   SampleFormat,
    ts:    u64,
}

fn symp_err(e: SymphError) -> SoniumError {
    SoniumError::Codec(format!("aac decode: {e}"))
}

impl AacDecoder {
    pub fn from_header(data: &[u8]) -> Result<Self> {
        let (rate, bits, channels) = parse_aac_codec_header(data)?;

        let mut params = CodecParameters::new();
        params.for_codec(CODEC_TYPE_AAC)
              .with_sample_rate(rate)
              .with_max_frames_per_packet(1024);

        // Map channel count to symphonia Channels bitmask.
        use symphonia_core::audio::Channels;
        let ch_mask = match channels {
            1 => Channels::FRONT_LEFT,
            _ => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
        };
        params.with_channels(ch_mask);

        let inner = SymphAacDecoder::try_new(&params, &DecoderOptions::default())
            .map_err(symp_err)?;

        Ok(Self { inner, fmt: SampleFormat::new(rate, bits, channels), ts: 0 })
    }
}

impl Decoder for AacDecoder {
    fn decode(&mut self, input: &[u8], output: &mut Vec<i16>) -> Result<()> {
        let packet = Packet::new_from_boxed_slice(
            0,
            self.ts,
            1024,                           // typical AAC-LC frame size
            input.to_vec().into_boxed_slice(),
        );
        self.ts += 1024;

        let buf = self.inner.decode(&packet).map_err(symp_err)?;

        // Extract samples and convert to interleaved i16.
        samples_to_i16(buf, output);
        Ok(())
    }

    fn sample_format(&self) -> SampleFormat { self.fmt }
}

fn samples_to_i16(buf: AudioBufferRef<'_>, output: &mut Vec<i16>) {
    match buf {
        AudioBufferRef::S16(b) => {
            use symphonia_core::audio::Signal;
            let frames = b.frames();
            let n_ch   = b.spec().channels.count();
            output.reserve(frames * n_ch);
            for i in 0..frames {
                for ch in 0..n_ch {
                    output.push(b.chan(ch)[i]);
                }
            }
        }
        AudioBufferRef::F32(b) => {
            use symphonia_core::audio::Signal;
            let frames = b.frames();
            let n_ch   = b.spec().channels.count();
            output.reserve(frames * n_ch);
            for i in 0..frames {
                for ch in 0..n_ch {
                    let s = (b.chan(ch)[i] * 32767.0).clamp(-32768.0, 32767.0) as i16;
                    output.push(s);
                }
            }
        }
        AudioBufferRef::S32(b) => {
            use symphonia_core::audio::Signal;
            let frames = b.frames();
            let n_ch   = b.spec().channels.count();
            output.reserve(frames * n_ch);
            for i in 0..frames {
                for ch in 0..n_ch {
                    output.push((b.chan(ch)[i] >> 16) as i16);
                }
            }
        }
        // For any other format, use the S16 planes if available via conversion
        _ => {
            // Unsupported buffer format — emit silence rather than panic
        }
    }
}
