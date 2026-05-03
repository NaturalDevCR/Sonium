//! Jitter buffer for decoded PCM chunks.
//!
//! [`SyncBuffer`] holds [`PcmChunk`]s sorted by playout timestamp and
//! releases them when their scheduled playout time arrives.  The buffer
//! absorbs network jitter by keeping a configurable amount of audio queued
//! ahead of real-time.
//!
//! ## Lifecycle
//!
//! ```text
//! decoder  ─► PcmChunk  ─► SyncBuffer::push
//!                                │
//!                        every audio callback
//!                                │
//!                     SyncBuffer::pop_ready(now_server_us)
//!                                │
//!                         PCM samples ─► speaker
//! ```

use sonium_common::SampleFormat;
use std::collections::VecDeque;

/// Tracks drift between local audio hardware and server stream timing.
#[derive(Debug, Default, Clone, Copy)]
pub struct DriftCorrector {
    pub ticks_since_last_correction: u32,
}

impl DriftCorrector {
    pub fn should_drop_frame(&mut self, age_us: i64) -> bool {
        self.ticks_since_last_correction = self.ticks_since_last_correction.saturating_add(1);
        if age_us > 2_000 && self.ticks_since_last_correction >= 2 {
            self.ticks_since_last_correction = 0;
            true
        } else {
            false
        }
    }

    pub fn should_duplicate_frame(&mut self, age_us: i64) -> bool {
        self.ticks_since_last_correction = self.ticks_since_last_correction.saturating_add(1);
        if age_us < -2_000 && self.ticks_since_last_correction >= 2 {
            self.ticks_since_last_correction = 0;
            true
        } else {
            false
        }
    }
}

/// A decoded audio chunk with its scheduled playout timestamp.
#[derive(Debug, Clone)]
pub struct PcmChunk {
    /// Absolute playout time in the **server clock** (µs since UNIX epoch).
    pub playout_us: i64,
    /// Interleaved i16 PCM samples (all channels).
    pub samples: Vec<i16>,
    /// Format of `samples`.
    pub fmt: SampleFormat,
    /// Read cursor in samples — allows partial consumption without copying.
    pub read_pos: usize,
}

impl PcmChunk {
    /// Create a new chunk.
    pub fn new(playout_us: i64, samples: Vec<i16>, fmt: SampleFormat) -> Self {
        Self {
            playout_us,
            samples,
            fmt,
            read_pos: 0,
        }
    }

    /// Number of samples (all channels) not yet consumed.
    pub fn remaining_samples(&self) -> usize {
        self.samples.len().saturating_sub(self.read_pos)
    }

    /// `true` when all samples have been consumed.
    pub fn is_exhausted(&self) -> bool {
        self.read_pos >= self.samples.len()
    }

    /// Duration of the remaining audio in microseconds.
    pub fn remaining_us(&self) -> i64 {
        let frames = self.remaining_samples() / self.fmt.channels as usize;
        (frames as f64 / self.fmt.rate as f64 * 1_000_000.0) as i64
    }

    /// Absolute playout time of the sample at the current `read_pos`.
    pub fn current_playout_us(&self) -> i64 {
        let frames = self.read_pos / self.fmt.channels as usize;
        let us = (frames as f64 / self.fmt.rate as f64 * 1_000_000.0) as i64;
        self.playout_us + us
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum State {
    Buffering,
    Playing,
}

/// Jitter buffer: holds decoded PCM chunks sorted by playout timestamp and
/// releases them at the right time.
///
/// **Drop policy:** chunks whose playout window has already passed (older than
/// 100 ms behind `now`) are silently dropped on the next call to
/// [`pop_ready`][Self::pop_ready].
pub struct SyncBuffer {
    chunks: VecDeque<PcmChunk>,
    buffered_samples: usize,
    fmt: SampleFormat,
    /// Health metrics: chunks dropped because they arrived after their playout window.
    stale_drop_count: u32,
    pub underrun_count: u32,
    /// Estimated jitter in microseconds.
    jitter_us: f64,
    /// Last arrival info for jitter calculation: (arrival_us, playout_us).
    last_arrival_info: Option<(i64, i64)>,
    state: State,
    target_buffer_us: i64,
    lead_us: i64,
    drift_drop_count: u64,
    drift_dup_count: u64,
}

impl SyncBuffer {
    /// Create a new buffer.
    ///
    /// `target_latency_ms` is the minimum amount of audio (in milliseconds)
    /// the buffer aims to keep queued.  Larger values tolerate more jitter at
    /// the cost of increased end-to-end latency.
    pub fn new(fmt: SampleFormat) -> Self {
        Self {
            chunks: VecDeque::new(),
            buffered_samples: 0,
            fmt,
            stale_drop_count: 0,
            underrun_count: 0,
            jitter_us: 0.0,
            last_arrival_info: None,
            state: State::Buffering,
            target_buffer_us: 1_000_000,
            lead_us: 40_000,
            drift_drop_count: 0,
            drift_dup_count: 0,
        }
    }

    /// Add a new chunk to the buffer.
    ///
    /// The chunk is inserted in timestamp order.  Jitter is estimated based on
    /// the arrival time of this chunk relative to its scheduled playout time.
    pub fn push(&mut self, chunk: PcmChunk, arrival_us: i64) {
        // Calculate jitter (server playout - local arrival)
        // We use relative variation in this difference.
        let current_diff = chunk.playout_us - arrival_us;
        if let Some((last_arrival, last_playout)) = self.last_arrival_info {
            let last_diff = last_playout - last_arrival;
            let variation = (current_diff - last_diff).abs() as f64;
            // EWMA with alpha=0.1
            self.jitter_us = self.jitter_us * 0.9 + variation * 0.1;
        }
        self.last_arrival_info = Some((arrival_us, chunk.playout_us));

        // Insert in order
        let mut insert_at = self.chunks.len();
        for (i, c) in self.chunks.iter().enumerate() {
            if c.playout_us > chunk.playout_us {
                insert_at = i;
                break;
            }
        }

        self.buffered_samples += chunk.remaining_samples();
        self.chunks.insert(insert_at, chunk);

        self.drop_excess_buffered_audio();
    }

    /// Return the next chunk only when its timestamp is actually due.
    ///
    /// This is intended for audio-callback-driven playback, where the callback
    /// already knows the server-clock time at which the first requested output
    /// frame will hit the DAC. Unlike [`pop_ready`][Self::pop_ready], this does
    /// not apply the jitter-buffer `lead_us` lookahead.
    pub fn pop_due_exact(&mut self, now_server_us: i64) -> Option<PcmChunk> {
        self.drop_stale(now_server_us);
        let front = self.chunks.front()?;
        if front.playout_us <= now_server_us {
            self.state = State::Playing;
            let chunk = self.chunks.pop_front().unwrap();
            self.buffered_samples = self
                .buffered_samples
                .saturating_sub(chunk.remaining_samples());
            Some(chunk)
        } else {
            None
        }
    }

    /// Playout timestamp for the next queued chunk.
    pub fn next_playout_us(&self) -> Option<i64> {
        self.chunks.front().map(|chunk| chunk.playout_us)
    }

    fn drop_stale(&mut self, now_server_us: i64) {
        let stale_threshold_us = (self.target_buffer_us / 2).clamp(10_000, 2_000_000);

        while let Some(front) = self.chunks.front() {
            let end_us = front.playout_us + front.remaining_us();
            if end_us < now_server_us - stale_threshold_us {
                let dropped = self.chunks.pop_front().unwrap();
                self.buffered_samples = self
                    .buffered_samples
                    .saturating_sub(dropped.remaining_samples());
                self.stale_drop_count += 1;
                continue;
            }
            break;
        }
    }

    fn drop_excess_buffered_audio(&mut self) {
        let max_buffer_us = self.target_buffer_us.saturating_mul(2).max(100_000);
        while self.buffer_depth_us() > max_buffer_us {
            let Some(dropped) = self.chunks.pop_front() else {
                break;
            };
            self.buffered_samples = self
                .buffered_samples
                .saturating_sub(dropped.remaining_samples());
            self.stale_drop_count += 1;
        }
    }

    /// Depth of buffered audio in microseconds.
    pub fn buffer_depth_us(&self) -> i64 {
        let frames = self.buffered_samples / self.fmt.channels as usize;
        (frames as f64 / self.fmt.rate as f64 * 1_000_000.0) as i64
    }

    /// Number of chunks currently held.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// `true` when no chunks are buffered.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Discard all buffered audio (e.g. after a reconnect).
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.buffered_samples = 0;
        self.stale_drop_count = 0;
        self.underrun_count = 0;
        self.state = State::Buffering;
    }

    /// Return and reset the accumulated stale drop counter.
    pub fn take_stale_drops(&mut self) -> u32 {
        let count = self.stale_drop_count;
        self.stale_drop_count = 0;
        count
    }

    /// Target buffer depth in microseconds as configured by the server.
    pub fn target_buffer_us(&self) -> i64 {
        self.target_buffer_us
    }

    pub fn take_drift_drop_count(&mut self) -> u64 {
        let count = self.drift_drop_count;
        self.drift_drop_count = 0;
        count
    }

    pub fn take_drift_dup_count(&mut self) -> u64 {
        let count = self.drift_dup_count;
        self.drift_dup_count = 0;
        count
    }

    /// Current estimated jitter in microseconds.
    pub fn jitter_us(&self) -> i64 {
        self.jitter_us as i64
    }

    /// Target buffer depth in milliseconds.
    pub fn set_target_buffer_ms(&mut self, ms: i32) {
        self.target_buffer_us = (ms as i64 * 1000).max(20_000);
        // Rescale lead_us to stay at 25% of target, minimum 5ms.
        self.lead_us = (self.target_buffer_us / 4).clamp(5_000, 100_000);
    }

    /// Set the lead time (lookahead) for `pop_ready`.
    pub fn set_lead_us(&mut self, us: i64) {
        self.lead_us = us;
    }

    pub fn take_underruns(&mut self) -> u32 {
        let count = self.underrun_count;
        self.underrun_count = 0;
        count
    }

    pub fn get_report(&mut self, now_server_us: i64) -> sonium_protocol::messages::HealthReport {
        let latency_ms = if let Some(next_us) = self.next_playout_us() {
            ((next_us - now_server_us) / 1000) as i32
        } else {
            0
        };

        sonium_protocol::messages::HealthReport::new(
            self.take_underruns(),
            0,
            self.take_stale_drops(),
            (self.buffer_depth_us() / 1000) as u32,
            (self.jitter_us / 1000.0) as u32,
            latency_ms,
        )
        .with_queue_metrics(
            0,
            self.len() as u32,
            (self.target_buffer_us / 1000) as u32,
        )
    }

    /// Pull audio that is due to be played at `now_server_us + lead_us`.
    pub fn pop_ready(&mut self, now_server_us: i64) -> Option<PcmChunk> {
        self.drop_stale(now_server_us);

        if self.state == State::Buffering {
            if self.buffer_depth_us() >= self.target_buffer_us {
                self.state = State::Playing;
            } else {
                return None;
            }
        }

        let front = self.chunks.front()?;
        if front.playout_us <= now_server_us + self.lead_us {
            let chunk = self.chunks.pop_front().unwrap();
            self.buffered_samples = self
                .buffered_samples
                .saturating_sub(chunk.remaining_samples());
            Some(chunk)
        } else {
            self.underrun_count += 1;
            None
        }
    }
}
