use sonium_common::{error::Result, SampleFormat};

/// Decode one encoded frame to interleaved i16 PCM samples.
pub trait Decoder {
    fn decode(&mut self, input: &[u8], output: &mut Vec<i16>) -> Result<()>;

    fn decode_missing(&mut self, duration_ms: u32, output: &mut Vec<i16>) -> Result<()> {
        let fmt = self.sample_format();
        let frames = (fmt.rate as usize)
            .saturating_mul(duration_ms as usize)
            .saturating_div(1000);
        output.resize(
            output
                .len()
                .saturating_add(frames.saturating_mul(fmt.channels as usize)),
            0,
        );
        Ok(())
    }

    fn sample_format(&self) -> SampleFormat;
}

/// Encode a buffer of interleaved i16 PCM samples to one encoded frame.
pub trait Encoder {
    fn encode(&mut self, pcm: &[i16], output: &mut Vec<u8>) -> Result<()>;
    fn sample_format(&self) -> SampleFormat;
    /// Codec identifier string ("opus", "pcm", "flac").
    fn codec_name(&self) -> &'static str;
    /// Codec-specific header bytes to send in CodecHeader message.
    fn codec_header(&self) -> Vec<u8>;
}
