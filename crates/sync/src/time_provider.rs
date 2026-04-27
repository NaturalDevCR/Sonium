//! NTP-like clock offset estimator.
//!
//! ## Algorithm
//!
//! The client sends a [`TimeMsg`][sonium_protocol::messages::TimeMsg] to the
//! server with `latency` zeroed.  The server fills `latency` with the
//! client→server transit time and echoes the message back.  When the client
//! receives the echo it calls [`TimeProvider::update`] with three values:
//!
//! ```text
//! t_sent_us       — local clock when the request was sent
//! t_recv_us       — local clock when the echo was received
//! server_lat_us   — Δ reported by the server  (= t_server_recv - t_client_sent)
//!
//! rtt  = t_recv  - t_sent          (total round-trip, local clock)
//! c2s  = server_lat                (client-to-server, server-measured)
//! s2c  = rtt - c2s                 (server-to-client)
//! diff = (c2s - s2c) / 2           (signed offset: server ahead if > 0)
//! ```
//!
//! Each `diff` sample is pushed into a 200-entry circular buffer.  The median
//! of that buffer is used as the current offset — this is robust against
//! transient network spikes.
//!
//! ## Thread safety
//!
//! [`TimeProvider`] is `Send + Sync`.  The atomic offset can be read by the
//! audio playback thread without acquiring any lock.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SAMPLE_BUFFER_SIZE: usize = 200;
const STALE_TIMEOUT_SECS: u64 = 60;

/// Estimates the signed offset between the local clock and the server clock.
///
/// A positive offset means the server is *ahead* of the client.
/// Use [`TimeProvider::to_server_time`] to convert local timestamps to server
/// time for scheduling chunk playback.
pub struct TimeProvider {
    /// Median-filtered offset in microseconds (server - local).
    offset_us: Arc<AtomicI64>,
    samples: parking_lot::Mutex<SampleBuffer>,
    last_sync: parking_lot::Mutex<Option<Instant>>,
}

struct SampleBuffer {
    buf: [i64; SAMPLE_BUFFER_SIZE],
    len: usize,
    pos: usize,
}

impl SampleBuffer {
    fn new() -> Self {
        Self {
            buf: [0i64; SAMPLE_BUFFER_SIZE],
            len: 0,
            pos: 0,
        }
    }

    fn push(&mut self, v: i64) {
        self.buf[self.pos] = v;
        self.pos = (self.pos + 1) % SAMPLE_BUFFER_SIZE;
        if self.len < SAMPLE_BUFFER_SIZE {
            self.len += 1;
        }
    }

    fn median(&self) -> i64 {
        if self.len == 0 {
            return 0;
        }
        let mut sorted: Vec<i64> = self.buf[..self.len].to_vec();
        sorted.sort_unstable();
        sorted[self.len / 2]
    }

    fn clear(&mut self) {
        self.len = 0;
        self.pos = 0;
    }

    fn len(&self) -> usize {
        self.len
    }
}

impl TimeProvider {
    /// Create a new `TimeProvider` with a zeroed offset.
    pub fn new() -> Self {
        Self {
            offset_us: Arc::new(AtomicI64::new(0)),
            samples: parking_lot::Mutex::new(SampleBuffer::new()),
            last_sync: parking_lot::Mutex::new(None),
        }
    }

    /// Update the clock offset estimate with one RTT measurement.
    ///
    /// # Arguments
    /// - `t_sent_us`        — local clock when the Time request was sent (µs since epoch)
    /// - `t_recv_us`        — local clock when the server echo was received
    /// - `server_latency_us` — `(t_server_recv - t_client_sent)` as reported by the server
    pub fn update(&self, t_sent_us: i64, t_recv_us: i64, server_latency_us: i64) {
        let rtt_us = t_recv_us - t_sent_us;
        let c2s_us = server_latency_us;
        let s2c_us = rtt_us.saturating_sub(c2s_us);
        let diff_us = (c2s_us - s2c_us) / 2;

        let mut buf = self.samples.lock();
        buf.push(diff_us);
        let median = buf.median();
        drop(buf);

        self.offset_us.store(median, Ordering::Relaxed);
        *self.last_sync.lock() = Some(Instant::now());
    }

    /// Convert a local timestamp (µs since UNIX epoch) to server time.
    pub fn to_server_time(&self, local_us: i64) -> i64 {
        local_us + self.offset_us.load(Ordering::Relaxed)
    }

    /// Convert a server timestamp (µs since UNIX epoch) to local time.
    pub fn to_local_time(&self, server_us: i64) -> i64 {
        server_us - self.offset_us.load(Ordering::Relaxed)
    }

    /// Current estimated offset in microseconds (server − local).
    pub fn offset_us(&self) -> i64 {
        self.offset_us.load(Ordering::Relaxed)
    }

    /// Number of samples collected since the last [`reset`][Self::reset].
    pub fn sample_count(&self) -> usize {
        self.samples.lock().len()
    }

    /// `true` if no sync has been received in the last 60 seconds.
    pub fn is_stale(&self) -> bool {
        match *self.last_sync.lock() {
            None => true,
            Some(t) => t.elapsed() > Duration::from_secs(STALE_TIMEOUT_SECS),
        }
    }

    /// Clear all samples and reset the offset to zero.
    ///
    /// Call this after a reconnect to avoid using stale measurements.
    pub fn reset(&self) {
        self.samples.lock().clear();
        self.offset_us.store(0, Ordering::Relaxed);
        *self.last_sync.lock() = None;
    }

    /// Clone the underlying atomic for cheap lock-free reads from the audio
    /// playback thread.
    pub fn offset_handle(&self) -> Arc<AtomicI64> {
        self.offset_us.clone()
    }
}

impl Default for TimeProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Current wall-clock time in microseconds since the UNIX epoch.
pub fn now_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as i64
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Symmetric RTT: c2s == s2c → offset is zero.
    #[test]
    fn symmetric_rtt_gives_zero_offset() {
        let tp = TimeProvider::new();
        for _ in 0..200 {
            // rtt=10ms, c2s=5ms, s2c=5ms → diff=0
            tp.update(0, 10_000, 5_000);
        }
        assert_eq!(tp.offset_us(), 0);
    }

    /// Server consistently ahead of client.
    #[test]
    fn server_ahead_gives_positive_offset() {
        let tp = TimeProvider::new();
        for _ in 0..200 {
            // rtt=10ms, c2s=7.5ms, s2c=2.5ms → diff = (7500-2500)/2 = 2500µs
            tp.update(0, 10_000, 7_500);
        }
        assert_eq!(tp.offset_us(), 2_500);
    }

    /// Server behind client.
    #[test]
    fn server_behind_gives_negative_offset() {
        let tp = TimeProvider::new();
        for _ in 0..200 {
            // rtt=10ms, c2s=2.5ms, s2c=7.5ms → diff = (2500-7500)/2 = -2500µs
            tp.update(0, 10_000, 2_500);
        }
        assert_eq!(tp.offset_us(), -2_500);
    }

    /// Outlier spike is suppressed by the median filter.
    #[test]
    fn outlier_suppressed_by_median() {
        let tp = TimeProvider::new();
        // 199 clean measurements: zero offset
        for _ in 0..199 {
            tp.update(0, 10_000, 5_000);
        }
        // One massive outlier
        tp.update(0, 10_000, 9_999);
        // Median of 200 samples still very close to 0
        assert!(
            tp.offset_us().abs() < 500,
            "outlier leaked: {}",
            tp.offset_us()
        );
    }

    /// Offset converges within 100 samples (not just at 200).
    #[test]
    fn convergence_within_100_samples() {
        let tp = TimeProvider::new();
        for i in 0..100 {
            tp.update(0, 10_000, 7_500); // target offset = 2500µs
            if i >= 10 {
                // By sample 11 the median should already be non-zero
            }
        }
        // After 100 samples the median of the first 100 should be 2500
        assert_eq!(tp.offset_us(), 2_500);
    }

    #[test]
    fn reset_clears_offset_and_samples() {
        let tp = TimeProvider::new();
        tp.update(0, 10_000, 7_500);
        assert_ne!(tp.offset_us(), 0);
        tp.reset();
        assert_eq!(tp.offset_us(), 0);
        assert_eq!(tp.sample_count(), 0);
    }

    #[test]
    fn to_server_time() {
        let tp = TimeProvider::new();
        for _ in 0..200 {
            tp.update(0, 10_000, 7_500); // offset = +2500µs
        }
        let local = 1_000_000_000_000i64;
        assert_eq!(tp.to_server_time(local), local + 2_500);
    }

    #[test]
    fn to_local_time() {
        let tp = TimeProvider::new();
        for _ in 0..200 {
            tp.update(0, 10_000, 7_500); // offset = +2500µs
        }
        let server = 1_000_000_002_500i64;
        assert_eq!(tp.to_local_time(server), server - 2_500);
    }

    #[test]
    fn stale_before_any_update() {
        let tp = TimeProvider::new();
        assert!(tp.is_stale());
    }

    #[test]
    fn not_stale_after_update() {
        let tp = TimeProvider::new();
        tp.update(0, 1_000, 500);
        assert!(!tp.is_stale());
    }

    #[test]
    fn circular_buffer_wraps_correctly() {
        let tp = TimeProvider::new();
        // Push 400 samples — should not panic and buffer stays at 200 entries
        for i in 0..400 {
            tp.update(0, 10_000, 5_000 + (i % 2) as i64 * 100);
        }
        assert_eq!(tp.sample_count(), 200);
    }

    #[test]
    fn now_us_is_reasonable() {
        let t = now_us();
        // Any value after 2024-01-01 in microseconds
        assert!(t > 1_704_067_200_000_000, "clock looks wrong: {t}");
    }
}
