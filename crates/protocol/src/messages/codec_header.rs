//! `CodecHeader` message — codec initialisation data sent once per stream.
//!
//! ## Payload encoding
//!
//! ```text
//! u32  codec_name_length
//! u8[] codec_name[codec_name_length]  ("opus", "flac", "pcm")
//! u32  header_data_length
//! u8[] header_data[header_data_length]
//! ```
//!
//! ## Opus header format (12 bytes)
//!
//! When `codec == "opus"` (or `"pcm"`), `header_data` is:
//!
//! ```text
//! u32  magic = 0x4F50_5553  ('O','P','U','S' as little-endian u32)
//! u32  sample_rate
//! u16  bits_per_sample
//! u16  channel_count
//! ```

use crate::wire::{WireRead, WireWrite};
use sonium_common::error::Result;

/// Codec initialisation message — the first message a client receives after
/// sending [`super::Hello`].
///
/// The client must parse `header_data` according to the `codec` string and
/// instantiate the appropriate decoder before it can decode
/// [`super::WireChunk`] frames.
#[derive(Debug, Clone, PartialEq)]
pub struct CodecHeader {
    /// Codec identifier: `"opus"`, `"flac"`, or `"pcm"`.
    pub codec:       String,
    /// Codec-specific initialisation bytes.
    pub header_data: Vec<u8>,
}

impl CodecHeader {
    /// Construct a new codec header.
    pub fn new(codec: impl Into<String>, header_data: Vec<u8>) -> Self {
        Self { codec: codec.into(), header_data }
    }

    /// Deserialise from a wire payload slice.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = WireRead::new(payload);
        let codec       = r.read_str()?;
        let header_data = r.read_blob()?;
        Ok(Self { codec, header_data })
    }

    /// Serialise to a wire payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = WireWrite::with_capacity(8 + self.codec.len() + self.header_data.len());
        w.write_str(&self.codec);
        w.write_blob(&self.header_data);
        w.finish()
    }
}

/// Build the 12-byte Opus/PCM codec header.
///
/// The same format is used for both `"opus"` and `"pcm"` streams — only the
/// magic bytes allow the receiver to verify it is reading the correct struct.
pub fn opus_codec_header(rate: u32, bits: u16, channels: u16) -> Vec<u8> {
    let mut h = Vec::with_capacity(12);
    // 0x4F50_5553 == "OPUS" in memory on a little-endian machine
    h.extend_from_slice(&0x4F50_5553u32.to_le_bytes());
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&bits.to_le_bytes());
    h.extend_from_slice(&channels.to_le_bytes());
    h
}

/// Parse an Opus/PCM codec header and return `(rate, bits, channels)`.
///
/// Returns [`sonium_common::SoniumError::Protocol`] if the slice is too short
/// or the magic bytes are wrong.
pub fn parse_opus_codec_header(data: &[u8]) -> Result<(u32, u16, u16)> {
    if data.len() < 12 {
        return Err(sonium_common::SoniumError::Protocol("opus header too short".into()));
    }
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != 0x4F50_5553 {
        return Err(sonium_common::SoniumError::Protocol(
            format!("bad opus magic: {magic:#010x}"),
        ));
    }
    let rate     = u32::from_le_bytes([data[4],  data[5],  data[6],  data[7]]);
    let bits     = u16::from_le_bytes([data[8],  data[9]]);
    let channels = u16::from_le_bytes([data[10], data[11]]);
    Ok((rate, bits, channels))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_header_round_trip_opus() {
        let hdr     = CodecHeader::new("opus", opus_codec_header(48_000, 16, 2));
        let encoded = hdr.encode();
        let decoded = CodecHeader::decode(&encoded).unwrap();
        assert_eq!(decoded, hdr);
    }

    #[test]
    fn codec_header_round_trip_pcm() {
        let hdr     = CodecHeader::new("pcm", opus_codec_header(44_100, 16, 1));
        let decoded = CodecHeader::decode(&hdr.encode()).unwrap();
        assert_eq!(decoded.codec, "pcm");
    }

    #[test]
    fn codec_header_empty_data() {
        let hdr     = CodecHeader::new("flac", vec![]);
        let decoded = CodecHeader::decode(&hdr.encode()).unwrap();
        assert_eq!(decoded.header_data, Vec::<u8>::new());
    }

    #[test]
    fn opus_header_encode_decode() {
        let raw              = opus_codec_header(48_000, 16, 2);
        let (rate, bits, ch) = parse_opus_codec_header(&raw).unwrap();
        assert_eq!((rate, bits, ch), (48_000, 16, 2));
    }

    #[test]
    fn opus_header_mono_44100() {
        let raw              = opus_codec_header(44_100, 16, 1);
        let (rate, bits, ch) = parse_opus_codec_header(&raw).unwrap();
        assert_eq!((rate, bits, ch), (44_100, 16, 1));
    }

    #[test]
    fn opus_header_too_short_returns_error() {
        assert!(parse_opus_codec_header(&[0u8; 11]).is_err());
    }

    #[test]
    fn opus_header_bad_magic_returns_error() {
        let mut raw = opus_codec_header(48_000, 16, 2);
        raw[0] = 0xFF; // corrupt magic
        assert!(parse_opus_codec_header(&raw).is_err());
    }

    #[test]
    fn codec_header_truncated_payload_returns_error() {
        assert!(CodecHeader::decode(&[0u8; 3]).is_err());
    }
}
