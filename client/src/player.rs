// Audio output abstraction over CPAL.
// Implemented as a stub for Fase 0 — the real CPAL integration is Fase 4.

use sonium_common::{SampleFormat, SoniumError};
use tracing::debug;

pub struct Player {
    fmt: SampleFormat,
}

impl Player {
    pub fn new(fmt: SampleFormat) -> Result<Self, SoniumError> {
        debug!(format = %fmt, "Player initialized (stub)");
        Ok(Self { fmt })
    }

    /// Write interleaved i16 PCM samples to the audio device.
    pub fn write(&mut self, samples: &[i16]) -> Result<(), SoniumError> {
        // Stub: just discards audio.
        // Will be replaced with CPAL ring-buffer write in Fase 4.
        debug!(samples = samples.len(), "Player::write (stub — audio discarded)");
        Ok(())
    }

    pub fn sample_format(&self) -> SampleFormat {
        self.fmt
    }
}
