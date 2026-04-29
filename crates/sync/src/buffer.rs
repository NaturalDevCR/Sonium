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

    /// Playout timestamp of the first unconsumed sample.
    pub fn current_playout_us(&self) -> i64 {
        let consumed_frames = self.read_pos / self.fmt.channels as usize;
        let consumed_us = (consumed_frames as f64 / self.fmt.rate as f64 * 1_000_000.0) as i64;
        self.playout_us + consumed_us
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
        }
    }

    /// Insert a decoded chunk, maintaining playout-time order.
    ///
    /// `arrival_us` is the local monotonic time (µs) when the chunk was received,
    /// used for jitter estimation.
    pub fn push(&mut self, chunk: PcmChunk, arrival_us: i64) {
        // Estimate jitter using RFC 3550 algorithm:
        // D(i,j) = (Rj - Sj) - (Ri - Si)
        // J = J + (|D(i,j)| - J)/16
        if let Some((last_r, last_s)) = self.last_arrival_info {
            let transit_diff = (arrival_us - chunk.playout_us) - (last_r - last_s);
            let d = transit_diff.abs() as f64;
            self.jitter_us += (d - self.jitter_us) / 16.0;
        }
        self.last_arrival_info = Some((arrival_us, chunk.playout_us));

        self.buffered_samples += chunk.remaining_samples();
        let pos = self
            .chunks
            .iter()
            .position(|c| c.playout_us > chunk.playout_us)
            .unwrap_or(self.chunks.len());
        self.chunks.insert(pos, chunk);
    }

    /// Return the next chunk ready to play relative to `now_server_us`, or
    /// `None` if no chunk is due yet.
    ///
    /// Stale chunks (more than 100 ms past their playout window) are dropped
    /// before checking.
    pub fn pop_ready(&mut self, now_server_us: i64) -> Option<PcmChunk> {
        const STALE_THRESHOLD_US: i64 = 100_000; // 100ms leniency
        const TARGET_BUFFER_US: i64 = 50_000; // 50ms hardware buffer target

        while let Some(front) = self.chunks.front() {
            let end_us = front.playout_us + front.remaining_us();
            if end_us < now_server_us - STALE_THRESHOLD_US {
                let dropped = self.chunks.pop_front().unwrap();
                self.buffered_samples = self
                    .buffered_samples
                    .saturating_sub(dropped.remaining_samples());
                self.stale_drop_count += 1;
                continue;
            }
            break;
        }

        if self.chunks.is_empty() {
            if self.state == State::Playing {
                self.underrun_count += 1;
                self.state = State::Buffering;
            }
            return None;
        }

        let front = self.chunks.front()?;

        match self.state {
            State::Buffering => {
                if front.playout_us <= now_server_us + TARGET_BUFFER_US {
                    self.state = State::Playing;
                    let frames =
                        (TARGET_BUFFER_US as f64 / 1_000_000.0 * self.fmt.rate as f64) as usize;
                    let samples = frames * self.fmt.channels as usize;
                    return Some(PcmChunk::new(0, vec![0; samples], self.fmt));
                }
            }
            State::Playing => {
                if front.playout_us <= now_server_us + TARGET_BUFFER_US {
                    let chunk = self.chunks.pop_front().unwrap();
                    self.buffered_samples = self
                        .buffered_samples
                        .saturating_sub(chunk.remaining_samples());
                    return Some(chunk);
                }
            }
        }
        None
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

    /// Current estimated jitter in microseconds.
    pub fn jitter_us(&self) -> i64 {
        self.jitter_us as i64
    }

    pub fn get_report(&self, _now_server_us: i64) -> SyncReport {
        SyncReport {
            underrun_count: self.underrun_count,
            stale_drop_count: self.stale_drop_count,
            buffer_depth_ms: (self.buffer_depth_us() / 1000) as i32,
            jitter_ms: (self.jitter_us / 1000.0) as i32,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub underrun_count: u32,
    pub stale_drop_count: u32,
    pub buffer_depth_ms: i32,
    pub jitter_ms: i32,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt() -> SampleFormat {
        SampleFormat::new(48_000, 16, 2)
    }

    fn chunk(playout_us: i64, sample_count: usize) -> PcmChunk {
        PcmChunk::new(playout_us, vec![0i16; sample_count], fmt())
    }

    #[test]
    fn push_and_pop_single_chunk() {
        let mut buf = SyncBuffer::new(fmt());
        let now = 1_000_000i64;
        // Chunk scheduled to play at exactly 'now'
        buf.push(chunk(now, 960), now);

        let silence = buf.pop_ready(now).unwrap();
        assert_eq!(silence.samples.len(), 4800); // 50ms at 48kHz stereo
        assert_eq!(silence.playout_us, 0);

        assert!(buf.pop_ready(now).is_some());
        assert!(buf.is_empty());
    }

    #[test]
    fn future_chunk_not_released() {
        let mut buf = SyncBuffer::new(fmt());
        let now = 1_000_000i64;
        // Chunk scheduled 5 seconds in the future
        buf.push(chunk(now + 5_000_000, 960), now);
        assert!(buf.pop_ready(now).is_none());
        assert!(!buf.is_empty());
    }

    #[test]
    fn chunks_released_in_playout_order() {
        let mut buf = SyncBuffer::new(fmt());
        // Push out-of-order
        buf.push(chunk(3_000, 960), 0);
        buf.push(chunk(1_000, 960), 0);
        buf.push(chunk(2_000, 960), 0);

        let c1 = buf.pop_ready(10_000).unwrap();
        let c2 = buf.pop_ready(10_000).unwrap();
        let c3 = buf.pop_ready(10_000).unwrap();
        assert!(c1.playout_us <= c2.playout_us);
        assert!(c2.playout_us <= c3.playout_us);
    }

    #[test]
    fn stale_chunks_dropped_automatically() {
        let mut buf = SyncBuffer::new(fmt());
        // Current time is 1,000,000us
        let now = 1_000_000i64;
        // Chunk finished at 800,000us (stale by 200ms, threshold is 100ms)
        buf.push(chunk(800_000 - 10_000, 960), now);

        // pop_ready should drop the stale chunk
        assert!(buf.pop_ready(now).is_none());
        assert!(buf.is_empty());
        assert_eq!(buf.get_report(now).stale_drop_count, 1);
    }

    #[test]
    fn buffer_depth_accounting() {
        let mut buf = SyncBuffer::new(fmt());
        // 960 stereo samples = 10ms
        buf.push(chunk(100_000, 960), 0);
        let depth = buf.buffer_depth_us();
        assert!((depth - 10_000).abs() < 100);
    }

    #[test]
    fn clear_empties_buffer() {
        let mut buf = SyncBuffer::new(fmt());
        buf.push(chunk(0, 960), 0);
        buf.push(chunk(10_000, 960), 0);
        buf.clear();
        assert!(buf.is_empty());
    }
}
