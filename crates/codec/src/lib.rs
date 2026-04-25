pub mod traits;
pub mod pcm;
pub mod opus;

pub use traits::{Decoder, Encoder};
pub use pcm::{PcmDecoder, PcmEncoder};
pub use opus::{OpusDecoder, OpusEncoder};

use sonium_common::SoniumError;

pub fn make_decoder(codec: &str, header_data: &[u8]) -> Result<Box<dyn Decoder + Send>, SoniumError> {
    match codec {
        "pcm"  => Ok(Box::new(PcmDecoder::new())),
        "opus" => {
            let dec = OpusDecoder::from_header(header_data)?;
            Ok(Box::new(dec))
        }
        other  => Err(SoniumError::UnsupportedCodec(other.into())),
    }
}

pub fn make_encoder(codec: &str, fmt: sonium_common::SampleFormat) -> Result<Box<dyn Encoder + Send>, SoniumError> {
    match codec {
        "pcm"  => Ok(Box::new(PcmEncoder::new(fmt))),
        "opus" => {
            let enc = OpusEncoder::new(fmt)?;
            Ok(Box::new(enc))
        }
        other  => Err(SoniumError::UnsupportedCodec(other.into())),
    }
}
