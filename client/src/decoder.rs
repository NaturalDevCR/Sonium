use sonium_codec::{make_decoder, traits::Decoder};
use sonium_common::{SampleFormat, SoniumError};

pub struct ActiveDecoder {
    inner: Box<dyn Decoder + Send>,
}

impl ActiveDecoder {
    pub fn from_codec(codec: &str, header_data: &[u8]) -> Result<Self, SoniumError> {
        Ok(Self {
            inner: make_decoder(codec, header_data)?,
        })
    }

    pub fn decode(&mut self, input: &[u8], output: &mut Vec<i16>) -> Result<(), SoniumError> {
        self.inner.decode(input, output)
    }

    pub fn decode_missing(
        &mut self,
        duration_ms: u32,
        output: &mut Vec<i16>,
    ) -> Result<(), SoniumError> {
        self.inner.decode_missing(duration_ms, output)
    }

    pub fn sample_format(&self) -> SampleFormat {
        self.inner.sample_format()
    }
}
