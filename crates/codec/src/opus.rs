use crate::traits::{Decoder, Encoder};
use audiopus::{
    coder::{Decoder as AudDecoder, Encoder as AudEncoder},
    Application, Channels, SampleRate,
};
use sonium_common::{error::Result, SampleFormat, SoniumError};
use sonium_protocol::messages::codec_header::{opus_codec_header, parse_opus_codec_header};

fn to_channels(n: u16) -> Result<Channels> {
    match n {
        1 => Ok(Channels::Mono),
        2 => Ok(Channels::Stereo),
        c => Err(SoniumError::Codec(format!(
            "unsupported Opus channel count {c}"
        ))),
    }
}

fn to_sample_rate(r: u32) -> Result<SampleRate> {
    match r {
        8000 => Ok(SampleRate::Hz8000),
        12000 => Ok(SampleRate::Hz12000),
        16000 => Ok(SampleRate::Hz16000),
        24000 => Ok(SampleRate::Hz24000),
        48000 => Ok(SampleRate::Hz48000),
        r => Err(SoniumError::Codec(format!(
            "unsupported Opus sample rate {r}"
        ))),
    }
}

pub struct OpusDecoder {
    inner: AudDecoder,
    fmt: SampleFormat,
}

impl OpusDecoder {
    pub fn from_header(header_data: &[u8]) -> Result<Self> {
        let (rate, bits, channels) = parse_opus_codec_header(header_data)?;
        let inner = AudDecoder::new(to_sample_rate(rate)?, to_channels(channels)?)
            .map_err(|e| SoniumError::Codec(format!("opus decoder: {e}")))?;
        Ok(Self {
            inner,
            fmt: SampleFormat::new(rate, bits, channels),
        })
    }
}

impl Decoder for OpusDecoder {
    fn decode(&mut self, input: &[u8], output: &mut Vec<i16>) -> Result<()> {
        // Maximum Opus frame is 120ms @ 48kHz stereo = 5760 samples * 2 channels
        let max_samples = 5760 * self.fmt.channels as usize;
        let start = output.len();
        output.resize(start + max_samples, 0i16);
        let decoded = self
            .inner
            .decode(Some(input), &mut output[start..], false)
            .map_err(|e| SoniumError::Codec(format!("opus decode: {e}")))?;
        let actual = decoded * self.fmt.channels as usize;
        output.truncate(start + actual);
        Ok(())
    }

    fn decode_missing(&mut self, duration_ms: u32, output: &mut Vec<i16>) -> Result<()> {
        let frame_samples = (self.fmt.rate as usize)
            .saturating_mul(duration_ms.clamp(2, 120) as usize)
            .saturating_div(1000);
        let max_samples = frame_samples.saturating_mul(self.fmt.channels as usize);
        let start = output.len();
        output.resize(start + max_samples, 0i16);
        let decoded = self
            .inner
            .decode(None::<&[u8]>, &mut output[start..], false)
            .map_err(|e| SoniumError::Codec(format!("opus plc decode: {e}")))?;
        let actual = decoded * self.fmt.channels as usize;
        output.truncate(start + actual);
        Ok(())
    }

    fn sample_format(&self) -> SampleFormat {
        self.fmt
    }
}

pub struct OpusEncoder {
    inner: AudEncoder,
    fmt: SampleFormat,
}

impl OpusEncoder {
    pub fn new(fmt: SampleFormat) -> Result<Self> {
        let inner = AudEncoder::new(
            to_sample_rate(fmt.rate)?,
            to_channels(fmt.channels)?,
            Application::Audio,
        )
        .map_err(|e| SoniumError::Codec(format!("opus encoder: {e}")))?;
        Ok(Self { inner, fmt })
    }
}

impl Encoder for OpusEncoder {
    fn encode(&mut self, pcm: &[i16], output: &mut Vec<u8>) -> Result<()> {
        // Max Opus packet is 4000 bytes
        let start = output.len();
        output.resize(start + 4000, 0u8);
        let written = self
            .inner
            .encode(pcm, &mut output[start..])
            .map_err(|e| SoniumError::Codec(format!("opus encode: {e}")))?;
        output.truncate(start + written);
        Ok(())
    }

    fn sample_format(&self) -> SampleFormat {
        self.fmt
    }
    fn codec_name(&self) -> &'static str {
        "opus"
    }

    fn codec_header(&self) -> Vec<u8> {
        opus_codec_header(self.fmt.rate, self.fmt.bits, self.fmt.channels)
    }
}
