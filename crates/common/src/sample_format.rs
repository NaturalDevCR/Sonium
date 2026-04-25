//! PCM stream descriptor.
//!
//! A [`SampleFormat`] captures the three parameters that fully describe a
//! linear PCM stream: sample rate (Hz), bit depth, and channel count.  All
//! arithmetic helpers derive from these three values so there is one source
//! of truth across encoder, decoder, jitter buffer, and player.
//!
//! # Example
//!
//! ```rust
//! use sonium_common::SampleFormat;
//!
//! let fmt = SampleFormat::new(48_000, 16, 2);
//!
//! // One 20 ms Opus frame at 48 kHz stereo = 960 frames × 4 bytes = 3840 bytes
//! assert_eq!(fmt.frames_for_ms(20.0), 960);
//! assert_eq!(fmt.frame_size(), 4);
//! ```

use serde::{Deserialize, Serialize};

/// Fully describes a linear PCM stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleFormat {
    /// Samples per second (e.g. 44100, 48000).
    pub rate: u32,
    /// Bits per sample per channel (e.g. 16, 24, 32).
    pub bits: u16,
    /// Number of interleaved channels (1 = mono, 2 = stereo).
    pub channels: u16,
}

impl SampleFormat {
    /// Create a new sample format.
    pub fn new(rate: u32, bits: u16, channels: u16) -> Self {
        Self { rate, bits, channels }
    }

    /// Bytes per sample per channel (`bits / 8`).
    pub fn sample_size(&self) -> usize {
        self.bits as usize / 8
    }

    /// Bytes for one interleaved frame (all channels combined).
    ///
    /// A *frame* contains one sample from every channel.  For 16-bit stereo
    /// this is 4 bytes.
    pub fn frame_size(&self) -> usize {
        self.sample_size() * self.channels as usize
    }

    /// Number of frames that fit in `byte_len` bytes.
    pub fn frame_count(&self, byte_len: usize) -> usize {
        if self.frame_size() == 0 { 0 } else { byte_len / self.frame_size() }
    }

    /// Wall-clock duration in milliseconds for `frames` PCM frames.
    pub fn duration_ms(&self, frames: usize) -> f64 {
        frames as f64 / self.rate as f64 * 1000.0
    }

    /// How many PCM frames represent `ms` milliseconds at this sample rate.
    ///
    /// Result is rounded towards zero.
    pub fn frames_for_ms(&self, ms: f64) -> usize {
        (ms / 1000.0 * self.rate as f64) as usize
    }
}

impl std::fmt::Display for SampleFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}Hz/{}bit/{}ch", self.rate, self.bits, self.channels)
    }
}

impl Default for SampleFormat {
    /// 48 kHz / 16-bit / stereo — the Opus default and most common configuration.
    fn default() -> Self {
        Self { rate: 48_000, bits: 16, channels: 2 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_size_stereo_16bit() {
        let fmt = SampleFormat::new(48_000, 16, 2);
        assert_eq!(fmt.sample_size(), 2);
        assert_eq!(fmt.frame_size(), 4);
    }

    #[test]
    fn frame_size_mono_24bit() {
        let fmt = SampleFormat::new(44_100, 24, 1);
        assert_eq!(fmt.sample_size(), 3);
        assert_eq!(fmt.frame_size(), 3);
    }

    #[test]
    fn frame_count() {
        let fmt = SampleFormat::new(48_000, 16, 2);
        // 3840 bytes / 4 bytes-per-frame = 960 frames (20 ms @ 48 kHz)
        assert_eq!(fmt.frame_count(3840), 960);
        // Partial frame is truncated
        assert_eq!(fmt.frame_count(3841), 960);
    }

    #[test]
    fn frames_for_ms_round_trip() {
        let fmt = SampleFormat::new(48_000, 16, 2);
        let frames = fmt.frames_for_ms(20.0);
        assert_eq!(frames, 960);
        let ms = fmt.duration_ms(960);
        assert!((ms - 20.0).abs() < 0.001, "expected ~20ms, got {ms}");
    }

    #[test]
    fn frames_for_ms_44100() {
        let fmt = SampleFormat::new(44_100, 16, 2);
        // 44100 × 0.020 = 882 frames
        assert_eq!(fmt.frames_for_ms(20.0), 882);
    }

    #[test]
    fn display() {
        let fmt = SampleFormat::new(48_000, 16, 2);
        assert_eq!(fmt.to_string(), "48000Hz/16bit/2ch");
    }

    #[test]
    fn zero_frame_size_does_not_panic() {
        let fmt = SampleFormat { rate: 48_000, bits: 0, channels: 0 };
        assert_eq!(fmt.frame_count(1024), 0);
    }
}
