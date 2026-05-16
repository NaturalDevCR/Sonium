//! Clock offset estimator using a 2-state Kalman filter.
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
//! Each `diff` sample is fed into a 2-state Kalman filter tracking
//! `[offset_µs, drift_µs/s]`.  The filter converges in ~5–10 samples (vs 200
//! for the old median) and tracks slow clock drift automatically.
//!
//! ## Thread safety
//!
//! [`TimeProvider`] is `Send + Sync`.  The atomic offset can be read by the
//! audio playback thread without acquiring any lock.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const STALE_TIMEOUT_SECS: u64 = 60;

// ── Kalman filter ────────────────────────────────────────────────────────────

/// 2-state Kalman filter for clock offset estimation.
///
/// State vector: `[offset_µs, drift_µs/s]`
/// - `offset` — signed clock difference (server − local) in microseconds.
/// - `drift`  — relative rate at which the offset changes (µs per second).
///
/// Measurement: each NTP-style `diff` sample (one scalar).
/// Measurement noise R = (700 µs)² ≈ 500 000 µs² (conservative for LAN TCP).
///
/// Convergence: typically < 10 samples vs 200 for the old median filter.
struct KalmanClock {
    offset: f64,
    drift: f64,
    /// 2×2 error-covariance matrix stored row-major.
    p: [[f64; 2]; 2],
    last_update: Option<Instant>,
    count: usize,
}

impl KalmanClock {
    fn new() -> Self {
        Self {
            offset: 0.0,
            drift: 0.0,
            // High initial uncertainty → first measurement dominates.
            p: [[1e12, 0.0], [0.0, 1e6]],
            last_update: None,
            count: 0,
        }
    }

    /// Feed one measurement and return the updated offset estimate (µs).
    fn update(&mut self, measured_offset_us: f64) -> f64 {
        let dt = self
            .last_update
            .map(|t| t.elapsed().as_secs_f64().clamp(0.001, 60.0))
            .unwrap_or(0.1);
        self.last_update = Some(Instant::now());

        // ── Prediction step (state transition F = [[1, dt], [0, 1]]) ────────
        let pred_offset = self.offset + self.drift * dt;
        let pred_drift = self.drift;

        // P_pred = F * P * F^T + Q
        let [p00, p01] = self.p[0];
        let [p10, p11] = self.p[1];

        // Process noise Q per update:
        //   offset: 1 µs² (driven by drift, tiny residual)
        //   drift:  0.01 µs²/s² (very stable crystal oscillators)
        let pp00 = p00 + dt * (p10 + p01) + dt * dt * p11 + 1.0;
        let pp01 = p01 + dt * p11;
        let pp10 = p10 + dt * p11;
        let pp11 = p11 + 0.01;

        // ── Innovation gating: reject network spikes ─────────────────────────
        // After a few samples the filter is stable; reject measurements that
        // differ from the prediction by more than 100 ms.
        let innovation = measured_offset_us - pred_offset;
        if self.count > 20 && innovation.abs() > 100_000.0 {
            // Spike: advance covariance but skip the measurement update.
            self.offset = pred_offset;
            self.drift = pred_drift;
            self.p = [[pp00, pp01], [pp10, pp11]];
            return self.offset;
        }

        // ── Update step (H = [1, 0], R = measurement noise) ─────────────────
        // Use tighter R for UDP probes (called with is_udp=true elsewhere), but
        // the same formula applies; callers can pass a pre-scaled diff.
        const R: f64 = 500_000.0; // (700 µs)²
        let s = pp00 + R;
        let k0 = pp00 / s;
        let k1 = pp10 / s;

        self.offset = pred_offset + k0 * innovation;
        self.drift = pred_drift + k1 * innovation;

        // P = (I − K·H) · P_pred
        self.p[0][0] = pp00 * (1.0 - k0);
        self.p[0][1] = pp01 * (1.0 - k0);
        self.p[1][0] = pp10 - k1 * pp00;
        self.p[1][1] = pp11 - k1 * pp01;

        self.count += 1;
        self.offset
    }

    fn clear(&mut self) {
        *self = Self::new();
    }

    fn len(&self) -> usize {
        self.count
    }

    /// Current variance of the offset estimate (µs²).
    fn variance(&self) -> f64 {
        self.p[0][0]
    }
}

// ── TimeProvider ─────────────────────────────────────────────────────────────

/// Estimates the signed offset between the local clock and the server clock.
///
/// A positive offset means the server is *ahead* of the client.
/// Use [`TimeProvider::to_server_time`] to convert local timestamps to server
/// time for scheduling chunk playback.
pub struct TimeProvider {
    /// Kalman-filtered offset in microseconds (server − local).
    offset_us: Arc<AtomicI64>,
    /// Additional group-wide offset applied on top of the Kalman estimate.
    group_offset_us: Arc<AtomicI64>,
    kalman: parking_lot::Mutex<KalmanClock>,
    last_sync: parking_lot::Mutex<Option<Instant>>,
    /// When true, client and server are on the same machine — skip network sync.
    on_server: bool,
}

impl TimeProvider {
    /// Create a new `TimeProvider` with a zeroed offset.
    pub fn new() -> Self {
        Self {
            offset_us: Arc::new(AtomicI64::new(0)),
            group_offset_us: Arc::new(AtomicI64::new(0)),
            kalman: parking_lot::Mutex::new(KalmanClock::new()),
            last_sync: parking_lot::Mutex::new(None),
            on_server: false,
        }
    }

    /// Mark this provider as "same machine" — skips network time sync.
    pub fn set_on_server(&mut self, on_server: bool) {
        self.on_server = on_server;
    }

    /// No-op kept for API compatibility.  The Kalman filter self-tunes.
    pub fn set_window_size(&self, _size: usize) {}

    /// Update the clock offset estimate with one RTT measurement.
    ///
    /// # Arguments
    /// - `t_sent_us`         — local clock when the Time request was sent (µs since epoch)
    /// - `t_recv_us`         — local clock when the server echo was received
    /// - `server_latency_us` — `(t_server_recv − t_client_sent)` as reported by the server
    pub fn update(&self, t_sent_us: i64, t_recv_us: i64, server_latency_us: i64) {
        if self.on_server {
            return;
        }
        let rtt_us = t_recv_us - t_sent_us;
        let diff_us = server_latency_us - (rtt_us / 2);

        let mut kf = self.kalman.lock();
        let new_offset = kf.update(diff_us as f64);
        drop(kf);

        self.offset_us.store(new_offset as i64, Ordering::Relaxed);
        *self.last_sync.lock() = Some(Instant::now());
    }

    /// Same as [`update`] but with a tighter measurement noise assumption
    /// appropriate for UDP time probes (lower transport jitter than TCP).
    ///
    /// Scales the diff by weighting it more aggressively — implemented by
    /// injecting two coincident samples so the Kalman gain is higher.
    pub fn update_udp(&self, t_sent_us: i64, t_recv_us: i64, server_latency_us: i64) {
        if self.on_server {
            return;
        }
        let rtt_us = t_recv_us - t_sent_us;
        let diff_us = server_latency_us - (rtt_us / 2);

        let mut kf = self.kalman.lock();
        // Feed the measurement twice: halves effective R, giving UDP probes
        // roughly 2× the weight of equivalent TCP probes.
        kf.update(diff_us as f64);
        let new_offset = kf.update(diff_us as f64);
        drop(kf);

        self.offset_us.store(new_offset as i64, Ordering::Relaxed);
        *self.last_sync.lock() = Some(Instant::now());
    }

    /// Convert a local timestamp (µs since UNIX epoch) to server time.
    pub fn to_server_time(&self, local_us: i64) -> i64 {
        local_us
            + self.offset_us.load(Ordering::Relaxed)
            + self.group_offset_us.load(Ordering::Relaxed)
    }

    /// Convert a server timestamp (µs since UNIX epoch) to local time.
    pub fn to_local_time(&self, server_us: i64) -> i64 {
        server_us
            - self.offset_us.load(Ordering::Relaxed)
            - self.group_offset_us.load(Ordering::Relaxed)
    }

    /// Current estimated offset in microseconds (server − local), excluding group offset.
    pub fn offset_us(&self) -> i64 {
        self.offset_us.load(Ordering::Relaxed)
    }

    /// Total offset including both Kalman and group correction.
    pub fn total_offset_us(&self) -> i64 {
        self.offset_us.load(Ordering::Relaxed) + self.group_offset_us.load(Ordering::Relaxed)
    }

    /// Current group offset in microseconds.
    pub fn group_offset_us(&self) -> i64 {
        self.group_offset_us.load(Ordering::Relaxed)
    }

    /// Number of Kalman updates since the last [`reset`][Self::reset].
    pub fn sample_count(&self) -> usize {
        if self.on_server {
            return 1;
        }
        self.kalman.lock().len()
    }

    /// Estimated variance of the offset estimate in µs².
    ///
    /// Decreases rapidly from ~1e12 (unknown) to ~500 000 (converged).
    /// Useful for weighting multiple sync sources.
    pub fn offset_variance_us2(&self) -> f64 {
        self.kalman.lock().variance()
    }

    /// `true` if no sync has been received in the last 60 seconds.
    pub fn is_stale(&self) -> bool {
        if self.on_server {
            return false;
        }
        match *self.last_sync.lock() {
            None => true,
            Some(t) => t.elapsed() > Duration::from_secs(STALE_TIMEOUT_SECS),
        }
    }

    /// Clear all samples and reset the offset to zero.
    ///
    /// Call this after a reconnect to avoid using stale measurements.
    pub fn reset(&self) {
        self.kalman.lock().clear();
        self.offset_us.store(0, Ordering::Relaxed);
        self.group_offset_us.store(0, Ordering::Relaxed);
        *self.last_sync.lock() = None;
    }

    /// Apply an adaptive correction to the group offset.
    ///
    /// `diff_us` is `(offset_us + group_offset_us) − target_group_offset_us`.
    /// A positive diff means the local total offset is *ahead* of the group
    /// target, so we subtract from group_offset to slow down local playout.
    ///
    /// The damping factor is adaptive based on |diff|:
    /// - |diff| > 20 ms → fast convergence (~300 ms at 100 ms GroupSync interval)
    /// - |diff| > 5 ms  → medium (~1 s)
    /// - otherwise      → smooth, inaudible correction (~10 s)
    pub fn nudge_group_offset(&self, diff_us: i64) {
        const MAX_GROUP_OFFSET_US: i64 = 50_000;
        let damping = match diff_us.unsigned_abs() {
            d if d > 20_000 => 3.0,
            d if d > 5_000 => 10.0,
            _ => 50.0,
        };
        let correction = (diff_us as f64 / damping) as i64;
        let current = self.group_offset_us.load(Ordering::Relaxed);
        let new = (current - correction).clamp(-MAX_GROUP_OFFSET_US, MAX_GROUP_OFFSET_US);
        self.group_offset_us.store(new, Ordering::Relaxed);
    }

    /// Overwrite the group offset directly (e.g. on reconnect).
    pub fn set_group_offset(&self, us: i64) {
        const MAX_GROUP_OFFSET_US: i64 = 50_000;
        self.group_offset_us.store(
            us.clamp(-MAX_GROUP_OFFSET_US, MAX_GROUP_OFFSET_US),
            Ordering::Relaxed,
        );
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

    /// Symmetric RTT: c2s == s2c → offset converges to zero.
    #[test]
    fn symmetric_rtt_gives_zero_offset() {
        let tp = TimeProvider::new();
        for _ in 0..20 {
            // rtt=10ms, c2s=5ms, s2c=5ms → diff=0
            tp.update(0, 10_000, 5_000);
        }
        assert!(
            tp.offset_us().abs() < 100,
            "offset should converge to ~0, got {}",
            tp.offset_us()
        );
    }

    /// Asymmetric RTT: server 3 ms ahead of client.
    #[test]
    fn asymmetric_rtt_converges_to_offset() {
        let tp = TimeProvider::new();
        for _ in 0..30 {
            // rtt=10ms, c2s=8ms, s2c=2ms → diff = 8 - 5 = 3 ms = 3000 µs
            tp.update(0, 10_000, 8_000);
        }
        let off = tp.offset_us();
        assert!(
            (off - 3_000).abs() < 200,
            "offset should converge to 3000 µs, got {off}"
        );
    }

    /// Kalman filter converges much faster than the old 200-sample median.
    #[test]
    fn fast_convergence() {
        let tp = TimeProvider::new();
        // Feed only 10 samples with a 5 ms offset
        for _ in 0..10 {
            tp.update(0, 10_000, 10_000); // c2s=10ms, rtt=10ms → diff=5ms
        }
        let off = tp.offset_us();
        assert!(
            (off - 5_000).abs() < 1_000,
            "should converge within 10 samples, got {off}"
        );
    }

    /// Reset clears the Kalman state.
    #[test]
    fn reset_clears_state() {
        let tp = TimeProvider::new();
        for _ in 0..20 {
            tp.update(0, 10_000, 8_000);
        }
        tp.reset();
        assert_eq!(tp.offset_us(), 0);
        assert_eq!(tp.sample_count(), 0);
    }

    /// Adaptive nudge: large diff converges faster.
    #[test]
    fn adaptive_nudge_large_diff_converges_fast() {
        let tp = TimeProvider::new();
        // 30 ms diff → damping 3 → correction 10 ms per call
        tp.nudge_group_offset(30_000);
        let after = tp.group_offset_us();
        // correction = 30000 / 3 = 10000, new = 0 - 10000 = -10000
        assert_eq!(after, -10_000);
    }

    /// Adaptive nudge: small diff uses gentle correction.
    #[test]
    fn adaptive_nudge_small_diff_is_gentle() {
        let tp = TimeProvider::new();
        // 2 ms diff → damping 50 → correction 40 µs
        tp.nudge_group_offset(2_000);
        let after = tp.group_offset_us();
        assert_eq!(after, -40); // -2000/50 = -40
    }

    /// Group offset is clamped to ±50 ms.
    #[test]
    fn group_offset_clamp() {
        let tp = TimeProvider::new();
        tp.set_group_offset(100_000);
        assert_eq!(tp.group_offset_us(), 50_000);
        tp.set_group_offset(-100_000);
        assert_eq!(tp.group_offset_us(), -50_000);
    }
}
