pub mod aac;
pub mod flac;
pub mod opus;
pub mod pcm;
pub mod traits;
pub mod vorbis;

pub use aac::AacDecoder;
pub use flac::{FlacDecoder, FlacEncoder};
pub use opus::{OpusDecoder, OpusEncoder};
pub use pcm::{PcmDecoder, PcmEncoder};
pub use traits::{Decoder, Encoder};
pub use vorbis::VorbisDecoder;

use sonium_common::SoniumError;

pub fn make_decoder(
    codec: &str,
    header_data: &[u8],
) -> Result<Box<dyn Decoder + Send>, SoniumError> {
    match codec {
        "pcm" => Ok(Box::new(PcmDecoder::new())),
        "opus" => {
            let dec = OpusDecoder::from_header(header_data)?;
            Ok(Box::new(dec))
        }
        "flac" => {
            let dec = FlacDecoder::from_header(header_data)?;
            Ok(Box::new(dec))
        }
        "vorbis" => {
            let dec = VorbisDecoder::from_header(header_data)?;
            Ok(Box::new(dec))
        }
        "aac" => {
            let dec = AacDecoder::from_header(header_data)?;
            Ok(Box::new(dec))
        }
        other => Err(SoniumError::UnsupportedCodec(other.into())),
    }
}

pub fn make_encoder(
    codec: &str,
    fmt: sonium_common::SampleFormat,
) -> Result<Box<dyn Encoder + Send>, SoniumError> {
    match codec {
        "pcm" => Ok(Box::new(PcmEncoder::new(fmt))),
        "opus" => {
            let enc = OpusEncoder::new(fmt)?;
            Ok(Box::new(enc))
        }
        "flac" => {
            let enc = FlacEncoder::new(fmt)?;
            Ok(Box::new(enc))
        }
        other => Err(SoniumError::UnsupportedCodec(other.into())),
    }
}
