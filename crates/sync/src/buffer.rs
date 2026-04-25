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

use std::collections::VecDeque;
use sonium_common::SampleFormat;

/// A decoded audio chunk with its scheduled playout timestamp.
#[derive(Debug, Clone)]
pub struct PcmChunk {
    /// Absolute playout time in the **server clock** (µs since UNIX epoch).
    pub playout_us: i64,
    /// Interleaved i16 PCM samples (all channels).
    pub samples:    Vec<i16>,
    /// Format of `samples`.
    pub fmt:        SampleFormat,
    /// Read cursor in samples — allows partial consumption without copying.
    pub read_pos:   usize,
}

impl PcmChunk {
    /// Create a new chunk.
    pub fn new(playout_us: i64, samples: Vec<i16>, fmt: SampleFormat) -> Self {
        Self { playout_us, samples, fmt, read_pos: 0 }
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
        let consumed_us     = (consumed_frames as f64 / self.fmt.rate as f64 * 1_000_000.0) as i64;
        self.playout_us + consumed_us
    }
}

/// Jitter buffer: holds decoded PCM chunks sorted by playout timestamp and
/// releases them at the right time.
///
/// **Drop policy:** chunks whose playout window has already passed (older than
/// 50 ms behind `now`) are silently dropped on the next call to
/// [`pop_ready`][Self::pop_ready].
pub struct SyncBuffer {
    chunks:            VecDeque<PcmChunk>,
    target_latency_us: i64,
    buffered_samples:  usize,
    fmt:               SampleFormat,
}

impl SyncBuffer {
    /// Create a new buffer.
    ///
    /// `target_latency_ms` is the minimum amount of audio (in milliseconds)
    /// the buffer aims to keep queued.  Larger values tolerate more jitter at
    /// the cost of increased end-to-end latency.
    pub fn new(fmt: SampleFormat, target_latency_ms: u32) -> Self {
        Self {
            chunks:            VecDeque::new(),
            target_latency_us: target_latency_ms as i64 * 1_000,
            buffered_samples:  0,
            fmt,
        }
    }

    /// Insert a decoded chunk, maintaining playout-time order.
    pub fn push(&mut self, chunk: PcmChunk) {
        self.buffered_samples += chunk.remaining_samples();
        let pos = self.chunks
            .iter()
            .position(|c| c.playout_us > chunk.playout_us)
            .unwrap_or(self.chunks.len());
        self.chunks.insert(pos, chunk);
    }

    /// Return the next chunk ready to play relative to `now_server_us`, or
    /// `None` if no chunk is due yet.
    ///
    /// Stale chunks (more than 50 ms past their playout window) are dropped
    /// before checking.
    pub fn pop_ready(&mut self, now_server_us: i64) -> Option<PcmChunk> {
        const STALE_THRESHOLD_US: i64 = 50_000;

        while let Some(front) = self.chunks.front() {
            let end_us = front.playout_us + front.remaining_us();
            if end_us < now_server_us - STALE_THRESHOLD_US {
                let dropped = self.chunks.pop_front().unwrap();
                self.buffered_samples =
                    self.buffered_samples.saturating_sub(dropped.remaining_samples());
                continue;
            }
            break;
        }

        let front = self.chunks.front()?;
        if front.playout_us <= now_server_us + self.target_latency_us {
            let chunk = self.chunks.pop_front().unwrap();
            self.buffered_samples =
                self.buffered_samples.saturating_sub(chunk.remaining_samples());
            return Some(chunk);
        }
        None
    }

    /// Depth of buffered audio in microseconds.
    pub fn buffer_depth_us(&self) -> i64 {
        let frames = self.buffered_samples / self.fmt.channels as usize;
        (frames as f64 / self.fmt.rate as f64 * 1_000_000.0) as i64
    }

    /// Number of chunks currently held.
    pub fn len(&self) -> usize { self.chunks.len() }

    /// `true` when no chunks are buffered.
    pub fn is_empty(&self) -> bool { self.chunks.is_empty() }

    /// Discard all buffered audio (e.g. after a reconnect).
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.buffered_samples = 0;
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt() -> SampleFormat { SampleFormat::new(48_000, 16, 2) }

    fn chunk(playout_us: i64, sample_count: usize) -> PcmChunk {
        PcmChunk::new(playout_us, vec![0i16; sample_count], fmt())
    }

    // ── PcmChunk ─────────────────────────────────────────────────────────────

    #[test]
    fn remaining_samples_full() {
        let c = chunk(0, 100);
        assert_eq!(c.remaining_samples(), 100);
    }

    #[test]
    fn remaining_samples_after_partial_consume() {
        let mut c = chunk(0, 100);
        c.read_pos = 40;
        assert_eq!(c.remaining_samples(), 60);
    }

    #[test]
    fn exhausted_when_fully_consumed() {
        let mut c = chunk(0, 100);
        c.read_pos = 100;
        assert!(c.is_exhausted());
    }

    #[test]
    fn remaining_us_stereo() {
        // 960 stereo samples = 480 frames = 10ms @ 48kHz
        let c = chunk(0, 960);
        let us = c.remaining_us();
        assert!((us - 10_000).abs() < 100, "expected ~10000µs got {us}");
    }

    #[test]
    fn current_playout_advances_with_read_pos() {
        let mut c = chunk(1_000_000, 960); // starts at t=1s
        let before = c.current_playout_us();
        c.read_pos = 480; // consumed half (240 frames = 5ms)
        let after = c.current_playout_us();
        assert!(after > before, "playout should advance");
        assert!((after - before - 5_000).abs() < 100);
    }

    // ── SyncBuffer ───────────────────────────────────────────────────────────

    #[test]
    fn push_and_pop_single_chunk() {
        let mut buf = SyncBuffer::new(fmt(), 0);
        let now     = 1_000_000i64;
        buf.push(chunk(now - 1_000, 960));
        assert!(buf.pop_ready(now).is_some());
        assert!(buf.is_empty());
    }

    #[test]
    fn future_chunk_not_released() {
        let mut buf = SyncBuffer::new(fmt(), 0);
        let now     = 1_000_000i64;
        // Chunk scheduled 5 seconds in the future
        buf.push(chunk(now + 5_000_000, 960));
        assert!(buf.pop_ready(now).is_none());
        assert!(!buf.is_empty());
    }

    #[test]
    fn chunks_released_in_playout_order() {
        let mut buf = SyncBuffer::new(fmt(), 0);
        // Push out-of-order
        buf.push(chunk(3_000, 960));
        buf.push(chunk(1_000, 960));
        buf.push(chunk(2_000, 960));

        let c1 = buf.pop_ready(10_000).unwrap();
        let c2 = buf.pop_ready(10_000).unwrap();
        let c3 = buf.pop_ready(10_000).unwrap();
        assert!(c1.playout_us <= c2.playout_us);
        assert!(c2.playout_us <= c3.playout_us);
    }

    #[test]
    fn stale_chunks_dropped_automatically() {
        let mut buf = SyncBuffer::new(fmt(), 0);
        // Chunk 2 seconds in the past
        buf.push(chunk(-2_000_000, 960));
        let now = 0i64;
        // pop_ready should drop the stale chunk, not return it
        assert!(buf.pop_ready(now).is_none());
        assert!(buf.is_empty());
    }

    #[test]
    fn buffer_depth_accounting() {
        let mut buf = SyncBuffer::new(fmt(), 0);
        // 960 stereo samples = 480 frames = 10ms
        buf.push(chunk(0, 960));
        let depth = buf.buffer_depth_us();
        assert!((depth - 10_000).abs() < 100, "expected ~10ms, got {depth}µs");
    }

    #[test]
    fn clear_empties_buffer() {
        let mut buf = SyncBuffer::new(fmt(), 0);
        buf.push(chunk(0, 960));
        buf.push(chunk(10_000, 960));
        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn target_latency_delays_release() {
        let mut buf = SyncBuffer::new(fmt(), 500); // 500ms target latency
        let now     = 0i64;
        // Chunk at t=0 should not be released because target_latency=500ms means
        // we only release chunks at playout_us <= now + 500_000
        buf.push(chunk(600_000, 960)); // at t=0.6s, beyond now+500ms
        assert!(buf.pop_ready(now).is_none());
    }
}
