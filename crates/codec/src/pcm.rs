//! Raw PCM passthrough codec.
//!
//! The PCM "codec" does no compression — it reinterprets bytes as little-endian
//! i16 samples on decode and writes them back as little-endian bytes on encode.
//! Useful for testing and for LAN scenarios where bandwidth is not a concern.

use crate::traits::{Decoder, Encoder};
use sonium_common::{SampleFormat, error::Result};

/// PCM decoder — converts raw i16 LE bytes to samples.
pub struct PcmDecoder {
    fmt: SampleFormat,
}

impl PcmDecoder {
    /// Create a decoder with the default 48 kHz / 16-bit / stereo format.
    pub fn new() -> Self {
        Self { fmt: SampleFormat::default() }
    }
}

impl Default for PcmDecoder {
    fn default() -> Self { Self::new() }
}

impl Decoder for PcmDecoder {
    fn decode(&mut self, input: &[u8], output: &mut Vec<i16>) -> Result<()> {
        output.reserve(input.len() / 2);
        for chunk in input.chunks_exact(2) {
            output.push(i16::from_le_bytes([chunk[0], chunk[1]]));
        }
        Ok(())
    }

    fn sample_format(&self) -> SampleFormat { self.fmt }
}

/// PCM encoder — serialises i16 samples as little-endian bytes.
pub struct PcmEncoder {
    fmt: SampleFormat,
}

impl PcmEncoder {
    /// Create an encoder for the given sample format.
    pub fn new(fmt: SampleFormat) -> Self {
        Self { fmt }
    }
}

impl Encoder for PcmEncoder {
    fn encode(&mut self, pcm: &[i16], output: &mut Vec<u8>) -> Result<()> {
        output.reserve(pcm.len() * 2);
        for &s in pcm {
            output.extend_from_slice(&s.to_le_bytes());
        }
        Ok(())
    }

    fn sample_format(&self) -> SampleFormat { self.fmt }
    fn codec_name(&self) -> &'static str { "pcm" }

    fn codec_header(&self) -> Vec<u8> {
        sonium_protocol::messages::codec_header::opus_codec_header(
            self.fmt.rate, self.fmt.bits, self.fmt.channels,
        )
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(samples: &[i16]) -> Vec<i16> {
        let fmt = SampleFormat::default();
        let mut enc = PcmEncoder::new(fmt);
        let mut dec = PcmDecoder::new();

        let mut encoded = Vec::new();
        enc.encode(samples, &mut encoded).unwrap();

        let mut decoded = Vec::new();
        dec.decode(&encoded, &mut decoded).unwrap();
        decoded
    }

    #[test]
    fn round_trip_silence() {
        let samples = vec![0i16; 960 * 2]; // 20ms stereo
        assert_eq!(round_trip(&samples), samples);
    }

    #[test]
    fn round_trip_sine_like() {
        let samples: Vec<i16> = (0..100)
            .map(|i| ((i as f32 * 0.1).sin() * i16::MAX as f32) as i16)
            .collect();
        assert_eq!(round_trip(&samples), samples);
    }

    #[test]
    fn round_trip_min_max() {
        let samples = vec![i16::MIN, i16::MAX, 0, -1, 1];
        assert_eq!(round_trip(&samples), samples);
    }

    #[test]
    fn round_trip_empty() {
        assert_eq!(round_trip(&[]), Vec::<i16>::new());
    }

    #[test]
    fn encode_is_little_endian() {
        let mut enc = PcmEncoder::new(SampleFormat::default());
        let mut out = Vec::new();
        enc.encode(&[0x0102i16], &mut out).unwrap();
        // i16 0x0102 in little-endian → [0x02, 0x01]
        assert_eq!(out, vec![0x02, 0x01]);
    }

    #[test]
    fn odd_byte_input_truncated_silently() {
        // An odd number of input bytes means the last byte is ignored
        let mut dec = PcmDecoder::new();
        let mut out = Vec::new();
        dec.decode(&[0x01, 0x00, 0xFF], &mut out).unwrap(); // 3 bytes → 1 sample
        assert_eq!(out, vec![0x0001i16]);
    }

    #[test]
    fn codec_name() {
        let enc = PcmEncoder::new(SampleFormat::default());
        assert_eq!(enc.codec_name(), "pcm");
    }

    #[test]
    fn codec_header_parseable() {
        use sonium_protocol::messages::codec_header::parse_opus_codec_header;
        let enc    = PcmEncoder::new(SampleFormat::new(48_000, 16, 2));
        let header = enc.codec_header();
        let (rate, bits, ch) = parse_opus_codec_header(&header).unwrap();
        assert_eq!((rate, bits, ch), (48_000, 16, 2));
    }
}
