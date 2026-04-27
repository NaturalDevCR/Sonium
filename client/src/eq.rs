//! Per-client biquad peaking-EQ DSP.
//!
//! Implements second-order IIR peaking EQ filters using the Audio EQ Cookbook
//! coefficients by Robert Bristow-Johnson.  Processes interleaved i16 PCM in-place.

use sonium_protocol::messages::EqBand;

/// State for a single-channel biquad section (Direct Form I).
#[derive(Clone)]
struct BiquadState {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32, // already divided by a0
    x1: f32,
    x2: f32, // input delay
    y1: f32,
    y2: f32, // output delay
}

impl BiquadState {
    /// Compute peaking EQ coefficients for the given band and sample rate.
    fn from_band(band: &EqBand, sample_rate: u32) -> Self {
        let f0 = band.freq_hz as f32;
        let gain_db = band.gain_db.clamp(-12.0, 12.0);
        let q = band.q.clamp(0.1, 10.0);
        let sr = sample_rate as f32;

        let a = 10.0_f32.powf(gain_db / 40.0); // sqrt(10^(gain/20))
        let w0 = 2.0 * std::f32::consts::PI * f0 / sr;
        let alpha = w0.sin() / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * w0.cos();
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * w0.cos();
        let a2 = 1.0 - alpha / a;

        // Normalise by a0
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Process one sample and return the filtered output.
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// A single EQ band: one biquad state per channel.
struct BandFilter {
    channels: Vec<BiquadState>,
}

impl BandFilter {
    fn new(band: &EqBand, sample_rate: u32, n_channels: usize) -> Self {
        let proto = BiquadState::from_band(band, sample_rate);
        Self {
            channels: vec![proto; n_channels],
        }
    }

    /// Process one sample on the given channel.
    #[inline]
    fn process(&mut self, ch: usize, x: f32) -> f32 {
        self.channels[ch].process(x)
    }
}

/// Compiled EQ — one `BandFilter` per band.
///
/// Rebuilt whenever the eq_bands or sample-format changes.
pub struct EqProcessor {
    bands: Vec<BandFilter>,
    n_channels: usize,
}

impl EqProcessor {
    /// Build an EQ processor for the given bands, sample rate, and channel count.
    pub fn new(bands: &[EqBand], sample_rate: u32, n_channels: usize) -> Self {
        Self {
            bands: bands
                .iter()
                .map(|b| BandFilter::new(b, sample_rate, n_channels))
                .collect(),
            n_channels,
        }
    }

    /// Apply all EQ bands to interleaved i16 PCM in-place.
    ///
    /// `samples` must be interleaved: `[L0, R0, L1, R1, …]`
    pub fn apply(&mut self, samples: &mut [i16]) {
        if self.bands.is_empty() {
            return;
        }

        for (i, s) in samples.iter_mut().enumerate() {
            let ch = i % self.n_channels;
            let mut x = *s as f32;
            for band in &mut self.bands {
                x = band.process(ch, x);
            }
            *s = x.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }
    }
}

/// Build an [`EqProcessor`] from a slice of `EqBand`, or `None` if the slice is empty.
pub fn build_eq(bands: &[EqBand], sample_rate: u32, n_channels: usize) -> Option<EqProcessor> {
    if bands.is_empty() {
        return None;
    }
    Some(EqProcessor::new(bands, sample_rate, n_channels))
}
