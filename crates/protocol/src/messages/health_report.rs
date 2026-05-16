use crate::wire::{WireRead, WireWrite};
use serde::{Deserialize, Serialize};
use sonium_common::error::Result;

/// Coarse playback health state derived from client telemetry.
///
/// This is intentionally transport-agnostic so TCP, RTP/UDP, and QUIC
/// DATAGRAM can all report into the same operator-facing model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioHealthState {
    Buffering,
    Stable,
    Degraded,
    Recovering,
    Underrun,
    Fallback,
    Offline,
    /// Multi-client sync error is within target band (group skew < 2 ms).
    SyncOk,
    /// Group skew elevated but recoverable (2-10 ms) — adaptive engine may nudge.
    SyncDegraded,
    /// Group skew large or oscillating (> 10 ms) — likely clock-source or transport problem.
    SyncUnstable,
}

impl AudioHealthState {
    pub const ALL: [Self; 10] = [
        Self::Buffering,
        Self::Stable,
        Self::Degraded,
        Self::Recovering,
        Self::Underrun,
        Self::Fallback,
        Self::Offline,
        Self::SyncOk,
        Self::SyncDegraded,
        Self::SyncUnstable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buffering => "buffering",
            Self::Stable => "stable",
            Self::Degraded => "degraded",
            Self::Recovering => "recovering",
            Self::Underrun => "underrun",
            Self::Fallback => "fallback",
            Self::Offline => "offline",
            Self::SyncOk => "sync_ok",
            Self::SyncDegraded => "sync_degraded",
            Self::SyncUnstable => "sync_unstable",
        }
    }

    pub fn from_report_snapshot(report: &HealthReport, target_buffer_ms: u32) -> Self {
        if report.underrun_count > 0 {
            return Self::Underrun;
        }

        let playout_queue_ms = report.total_playout_queue_ms();
        if playout_queue_ms == 0 {
            return Self::Buffering;
        }

        let high_jitter = report.jitter_ms > jitter_warning_ms(target_buffer_ms);
        let low_buffer = playout_queue_ms < low_buffer_warning_ms(target_buffer_ms);

        if report.overrun_count > 0 || report.stale_drop_count > 0 || high_jitter || low_buffer {
            Self::Degraded
        } else {
            Self::Stable
        }
    }

    /// Derive a sync-quality state from `sync_error_to_group_us` and playout error percentiles.
    ///
    /// Thresholds align with the SONOS-level roadmap: `p95 ≤ 5 ms` is the target band.
    pub fn sync_from_report(report: &HealthReport) -> Self {
        let abs_skew_us = report.sync_error_to_group_us.unsigned_abs();
        let playout_p95_us = report.playout_error_us_p95 as u64;
        if abs_skew_us <= 2_000 && playout_p95_us <= 2_000 {
            Self::SyncOk
        } else if abs_skew_us <= 10_000 && playout_p95_us <= 5_000 {
            Self::SyncDegraded
        } else {
            Self::SyncUnstable
        }
    }
}

impl std::fmt::Display for AudioHealthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn jitter_warning_ms(target_buffer_ms: u32) -> u32 {
    (target_buffer_ms.saturating_mul(7) / 10).max(80)
}

pub fn low_buffer_warning_ms(target_buffer_ms: u32) -> u32 {
    (target_buffer_ms / 4).max(20)
}

/// Real-time health metrics from a client playback session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthReport {
    /// Number of times the playback buffer ran dry (underrun).
    pub underrun_count: u32,
    /// Number of samples dropped due to buffer overflow (overrun).
    pub overrun_count: u32,
    /// Number of chunks dropped because they arrived after their playout time.
    pub stale_drop_count: u32,
    /// Current depth of the jitter buffer in milliseconds.
    pub buffer_depth_ms: u32,
    /// Estimated network jitter in milliseconds.
    pub jitter_ms: u32,
    /// Measured end-to-end latency in milliseconds (offset from server clock).
    pub latency_ms: i32,
    /// Current depth of the output/player ring buffer in milliseconds.
    #[serde(default)]
    pub output_buffer_ms: u32,
    /// Number of decoded chunks currently queued in the jitter buffer.
    #[serde(default)]
    pub jitter_buffer_chunks: u32,
    /// Current target playout latency in milliseconds.
    #[serde(default)]
    pub target_playout_latency_ms: u32,
    /// Number of audio callbacks that arrived much later than expected.
    #[serde(default)]
    pub callback_starvation_count: u32,
    /// Number of output callback errors/xruns reported by the audio backend.
    #[serde(default)]
    pub audio_callback_xrun_count: u32,
    /// Number of RTP packets received by the client UDP media path.
    #[serde(default)]
    pub rtp_packets_received: u32,
    /// Number of RTP sequence numbers skipped by the client UDP media path.
    #[serde(default)]
    pub rtp_sequence_gaps: u32,
    /// Number of RTP datagrams rejected by the client decoder.
    #[serde(default)]
    pub rtp_decode_error_count: u32,
    /// Number of missing RTP packets concealed by the client decoder.
    #[serde(default)]
    pub rtp_concealed_packets: u32,
    /// Number of frames dropped to correct clock drift.
    #[serde(default)]
    pub drift_drop_count: u64,
    /// Number of frames duplicated to correct clock drift.
    #[serde(default)]
    pub drift_dup_count: u64,
    /// Number of ARQ NACK packets sent by the client.
    #[serde(default)]
    pub arq_nacks_sent: u32,
    /// Number of retransmitted audio packets received by the client.
    #[serde(default)]
    pub arq_retransmit_received: u32,
    /// Number of audio packets recovered via FEC (no retransmission needed).
    #[serde(default)]
    pub arq_fec_recovered: u32,
    /// Client estimate of clock offset to server in microseconds (after Kalman filter).
    #[serde(default)]
    pub clock_offset_us: i64,
    /// Group offset applied to playout in microseconds (server-broadcast target).
    #[serde(default)]
    pub group_offset_us: i64,
    /// Total offset applied (clock + group) in microseconds.
    #[serde(default)]
    pub total_offset_us: i64,
    /// Output device latency reported by the audio backend in microseconds.
    #[serde(default)]
    pub output_latency_us: u32,
    /// P50 of |scheduled_playout - actual_playout| over rolling 60s window, microseconds.
    #[serde(default)]
    pub playout_error_us_p50: u32,
    /// P95 of playout error, microseconds.
    #[serde(default)]
    pub playout_error_us_p95: u32,
    /// P99 of playout error, microseconds.
    #[serde(default)]
    pub playout_error_us_p99: u32,
    /// P99 of audio callback duration (Instant pre/post cpal callback), microseconds.
    #[serde(default)]
    pub callback_xrun_us_p99: u32,
    /// Signed error between this client's playout and the group target, microseconds.
    #[serde(default)]
    pub sync_error_to_group_us: i64,
    /// Resample ratio the server commanded (rate_ppm from group sync), parts-per-million.
    #[serde(default)]
    pub resample_ratio_ppm_commanded: i32,
    /// Resample ratio the client actually applied (may differ if engine is stepwise), ppm.
    #[serde(default)]
    pub resample_ratio_ppm_applied: i32,
    /// Combined ARQ+FEC packet recovery rate as percent of total loss, 0-10000 (basis points).
    #[serde(default)]
    pub arq_fec_combined_recovery_pct: u16,
}

impl HealthReport {
    pub fn new(
        underrun_count: u32,
        overrun_count: u32,
        stale_drop_count: u32,
        buffer_depth_ms: u32,
        jitter_ms: u32,
        latency_ms: i32,
    ) -> Self {
        Self {
            underrun_count,
            overrun_count,
            stale_drop_count,
            buffer_depth_ms,
            jitter_ms,
            latency_ms,
            output_buffer_ms: 0,
            jitter_buffer_chunks: 0,
            target_playout_latency_ms: 0,
            callback_starvation_count: 0,
            audio_callback_xrun_count: 0,
            rtp_packets_received: 0,
            rtp_sequence_gaps: 0,
            rtp_decode_error_count: 0,
            rtp_concealed_packets: 0,
            drift_drop_count: 0,
            drift_dup_count: 0,
            arq_nacks_sent: 0,
            arq_retransmit_received: 0,
            arq_fec_recovered: 0,
            clock_offset_us: 0,
            group_offset_us: 0,
            total_offset_us: 0,
            output_latency_us: 0,
            playout_error_us_p50: 0,
            playout_error_us_p95: 0,
            playout_error_us_p99: 0,
            callback_xrun_us_p99: 0,
            sync_error_to_group_us: 0,
            resample_ratio_ppm_commanded: 0,
            resample_ratio_ppm_applied: 0,
            arq_fec_combined_recovery_pct: 0,
        }
    }

    pub fn snapshot_state(&self, target_buffer_ms: u32) -> AudioHealthState {
        AudioHealthState::from_report_snapshot(self, target_buffer_ms)
    }

    pub fn with_queue_metrics(
        mut self,
        output_buffer_ms: u32,
        jitter_buffer_chunks: u32,
        target_playout_latency_ms: u32,
    ) -> Self {
        self.output_buffer_ms = output_buffer_ms;
        self.jitter_buffer_chunks = jitter_buffer_chunks;
        self.target_playout_latency_ms = target_playout_latency_ms;
        self
    }

    pub fn with_callback_metrics(
        mut self,
        callback_starvation_count: u32,
        audio_callback_xrun_count: u32,
    ) -> Self {
        self.callback_starvation_count = callback_starvation_count;
        self.audio_callback_xrun_count = audio_callback_xrun_count;
        self
    }

    pub fn with_rtp_metrics(
        mut self,
        packets_received: u32,
        sequence_gaps: u32,
        decode_error_count: u32,
        concealed_packets: u32,
    ) -> Self {
        self.rtp_packets_received = packets_received;
        self.rtp_sequence_gaps = sequence_gaps;
        self.rtp_decode_error_count = decode_error_count;
        self.rtp_concealed_packets = concealed_packets;
        self
    }

    pub fn with_drift_metrics(mut self, drop_count: u64, dup_count: u64) -> Self {
        self.drift_drop_count = drop_count;
        self.drift_dup_count = dup_count;
        self
    }

    pub fn with_arq_metrics(
        mut self,
        nacks_sent: u32,
        retransmit_received: u32,
        fec_recovered: u32,
    ) -> Self {
        self.arq_nacks_sent = nacks_sent;
        self.arq_retransmit_received = retransmit_received;
        self.arq_fec_recovered = fec_recovered;
        self
    }

    pub fn with_sync_metrics(
        mut self,
        clock_offset_us: i64,
        group_offset_us: i64,
        total_offset_us: i64,
        sync_error_to_group_us: i64,
    ) -> Self {
        self.clock_offset_us = clock_offset_us;
        self.group_offset_us = group_offset_us;
        self.total_offset_us = total_offset_us;
        self.sync_error_to_group_us = sync_error_to_group_us;
        self
    }

    pub fn with_playout_error(
        mut self,
        p50_us: u32,
        p95_us: u32,
        p99_us: u32,
        callback_xrun_p99_us: u32,
        output_latency_us: u32,
    ) -> Self {
        self.playout_error_us_p50 = p50_us;
        self.playout_error_us_p95 = p95_us;
        self.playout_error_us_p99 = p99_us;
        self.callback_xrun_us_p99 = callback_xrun_p99_us;
        self.output_latency_us = output_latency_us;
        self
    }

    pub fn with_resample_metrics(mut self, commanded_ppm: i32, applied_ppm: i32) -> Self {
        self.resample_ratio_ppm_commanded = commanded_ppm;
        self.resample_ratio_ppm_applied = applied_ppm;
        self
    }

    pub fn with_arq_fec_recovery_pct(mut self, basis_points: u16) -> Self {
        self.arq_fec_combined_recovery_pct = basis_points;
        self
    }

    pub fn total_playout_queue_ms(&self) -> u32 {
        self.buffer_depth_ms.saturating_add(self.output_buffer_ms)
    }

    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = WireRead::new(payload);
        Ok(Self {
            underrun_count: r.read_u32()?,
            overrun_count: r.read_u32()?,
            stale_drop_count: r.read_u32()?,
            buffer_depth_ms: r.read_u32()?,
            jitter_ms: r.read_u32()?,
            latency_ms: r.read_i32()?,
            output_buffer_ms: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            jitter_buffer_chunks: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            target_playout_latency_ms: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            callback_starvation_count: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            audio_callback_xrun_count: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            rtp_packets_received: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            rtp_sequence_gaps: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            rtp_decode_error_count: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            rtp_concealed_packets: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            drift_drop_count: if r.remaining() >= 4 {
                r.read_u32()? as u64
            } else {
                0
            },
            drift_dup_count: if r.remaining() >= 4 {
                r.read_u32()? as u64
            } else {
                0
            },
            arq_nacks_sent: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            arq_retransmit_received: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            arq_fec_recovered: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            clock_offset_us: if r.remaining() >= 8 { r.read_i64()? } else { 0 },
            group_offset_us: if r.remaining() >= 8 { r.read_i64()? } else { 0 },
            total_offset_us: if r.remaining() >= 8 { r.read_i64()? } else { 0 },
            output_latency_us: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            playout_error_us_p50: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            playout_error_us_p95: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            playout_error_us_p99: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            callback_xrun_us_p99: if r.remaining() >= 4 { r.read_u32()? } else { 0 },
            sync_error_to_group_us: if r.remaining() >= 8 { r.read_i64()? } else { 0 },
            resample_ratio_ppm_commanded: if r.remaining() >= 4 { r.read_i32()? } else { 0 },
            resample_ratio_ppm_applied: if r.remaining() >= 4 { r.read_i32()? } else { 0 },
            arq_fec_combined_recovery_pct: if r.remaining() >= 2 { r.read_u16()? } else { 0 },
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = WireWrite::with_capacity(154);
        w.write_u32(self.underrun_count);
        w.write_u32(self.overrun_count);
        w.write_u32(self.stale_drop_count);
        w.write_u32(self.buffer_depth_ms);
        w.write_u32(self.jitter_ms);
        w.write_i32(self.latency_ms);
        w.write_u32(self.output_buffer_ms);
        w.write_u32(self.jitter_buffer_chunks);
        w.write_u32(self.target_playout_latency_ms);
        w.write_u32(self.callback_starvation_count);
        w.write_u32(self.audio_callback_xrun_count);
        w.write_u32(self.rtp_packets_received);
        w.write_u32(self.rtp_sequence_gaps);
        w.write_u32(self.rtp_decode_error_count);
        w.write_u32(self.rtp_concealed_packets);
        w.write_u32(self.drift_drop_count as u32);
        w.write_u32(self.drift_dup_count as u32);
        w.write_u32(self.arq_nacks_sent);
        w.write_u32(self.arq_retransmit_received);
        w.write_u32(self.arq_fec_recovered);
        w.write_i64(self.clock_offset_us);
        w.write_i64(self.group_offset_us);
        w.write_i64(self.total_offset_us);
        w.write_u32(self.output_latency_us);
        w.write_u32(self.playout_error_us_p50);
        w.write_u32(self.playout_error_us_p95);
        w.write_u32(self.playout_error_us_p99);
        w.write_u32(self.callback_xrun_us_p99);
        w.write_i64(self.sync_error_to_group_us);
        w.write_i32(self.resample_ratio_ppm_commanded);
        w.write_i32(self.resample_ratio_ppm_applied);
        w.write_u16(self.arq_fec_combined_recovery_pct);
        w.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(
        underruns: u32,
        overruns: u32,
        stale_drops: u32,
        buffer_ms: u32,
        jitter_ms: u32,
    ) -> HealthReport {
        HealthReport::new(underruns, overruns, stale_drops, buffer_ms, jitter_ms, 0)
    }

    #[test]
    fn snapshot_state_is_stable_when_buffer_and_jitter_are_healthy() {
        assert_eq!(
            report(0, 0, 0, 250, 10).snapshot_state(500),
            AudioHealthState::Stable
        );
    }

    #[test]
    fn snapshot_state_marks_empty_buffer_as_buffering() {
        assert_eq!(
            report(0, 0, 0, 0, 0).snapshot_state(500),
            AudioHealthState::Buffering
        );
    }

    #[test]
    fn snapshot_state_prioritizes_underruns() {
        assert_eq!(
            report(1, 0, 0, 250, 10).snapshot_state(500),
            AudioHealthState::Underrun
        );
    }

    #[test]
    fn snapshot_state_marks_high_jitter_as_degraded() {
        assert_eq!(
            report(0, 0, 0, 250, 400).snapshot_state(500),
            AudioHealthState::Degraded
        );
    }

    #[test]
    fn queue_metrics_round_trip_on_wire() {
        let original = report(0, 0, 0, 120, 8)
            .with_queue_metrics(180, 6, 500)
            .with_callback_metrics(2, 1)
            .with_rtp_metrics(100, 3, 1, 2);
        let decoded = HealthReport::decode(&original.encode()).unwrap();

        assert_eq!(decoded.output_buffer_ms, 180);
        assert_eq!(decoded.jitter_buffer_chunks, 6);
        assert_eq!(decoded.target_playout_latency_ms, 500);
        assert_eq!(decoded.callback_starvation_count, 2);
        assert_eq!(decoded.audio_callback_xrun_count, 1);
        assert_eq!(decoded.rtp_packets_received, 100);
        assert_eq!(decoded.rtp_sequence_gaps, 3);
        assert_eq!(decoded.rtp_decode_error_count, 1);
        assert_eq!(decoded.rtp_concealed_packets, 2);
        assert_eq!(decoded.total_playout_queue_ms(), 300);
    }

    #[test]
    fn sync_metrics_round_trip_on_wire() {
        let original = report(0, 0, 0, 120, 8)
            .with_sync_metrics(-1_234, 567, -667, 320)
            .with_playout_error(900, 1_800, 2_700, 1_500, 4_200)
            .with_resample_metrics(45, 42)
            .with_arq_fec_recovery_pct(9_876);
        let decoded = HealthReport::decode(&original.encode()).unwrap();

        assert_eq!(decoded.clock_offset_us, -1_234);
        assert_eq!(decoded.group_offset_us, 567);
        assert_eq!(decoded.total_offset_us, -667);
        assert_eq!(decoded.sync_error_to_group_us, 320);
        assert_eq!(decoded.playout_error_us_p50, 900);
        assert_eq!(decoded.playout_error_us_p95, 1_800);
        assert_eq!(decoded.playout_error_us_p99, 2_700);
        assert_eq!(decoded.callback_xrun_us_p99, 1_500);
        assert_eq!(decoded.output_latency_us, 4_200);
        assert_eq!(decoded.resample_ratio_ppm_commanded, 45);
        assert_eq!(decoded.resample_ratio_ppm_applied, 42);
        assert_eq!(decoded.arq_fec_combined_recovery_pct, 9_876);
    }

    #[test]
    fn sync_from_report_classifies_skew() {
        let healthy = report(0, 0, 0, 120, 8)
            .with_sync_metrics(0, 0, 0, 1_500)
            .with_playout_error(500, 1_500, 2_000, 0, 0);
        assert_eq!(
            AudioHealthState::sync_from_report(&healthy),
            AudioHealthState::SyncOk
        );

        let degraded = report(0, 0, 0, 120, 8)
            .with_sync_metrics(0, 0, 0, 5_000)
            .with_playout_error(1_000, 4_000, 5_000, 0, 0);
        assert_eq!(
            AudioHealthState::sync_from_report(&degraded),
            AudioHealthState::SyncDegraded
        );

        let unstable = report(0, 0, 0, 120, 8)
            .with_sync_metrics(0, 0, 0, 25_000)
            .with_playout_error(1_000, 4_000, 5_000, 0, 0);
        assert_eq!(
            AudioHealthState::sync_from_report(&unstable),
            AudioHealthState::SyncUnstable
        );
    }

    #[test]
    fn v1_payload_decodes_when_new_sync_fields_are_missing() {
        // Build a payload that contains everything up to and including ARQ FEC recovered
        // (the previous wire end) and verify the new sync fields default to zero.
        let v1 = report(2, 1, 3, 200, 12)
            .with_queue_metrics(100, 4, 250)
            .with_callback_metrics(5, 0)
            .with_rtp_metrics(900, 2, 0, 1)
            .with_drift_metrics(7, 3)
            .with_arq_metrics(11, 9, 4);
        let mut payload = v1.encode();
        // Truncate everything past arq_fec_recovered (the previous trailing field).
        // 20 u32 fields = 80 bytes total in v1 wire.
        payload.truncate(80);
        let decoded = HealthReport::decode(&payload).unwrap();

        assert_eq!(decoded.arq_fec_recovered, 4);
        assert_eq!(decoded.clock_offset_us, 0);
        assert_eq!(decoded.group_offset_us, 0);
        assert_eq!(decoded.total_offset_us, 0);
        assert_eq!(decoded.sync_error_to_group_us, 0);
        assert_eq!(decoded.playout_error_us_p95, 0);
        assert_eq!(decoded.arq_fec_combined_recovery_pct, 0);
    }

    #[test]
    fn legacy_health_report_payload_decodes_with_zero_queue_metrics() {
        let mut legacy_payload = Vec::new();
        legacy_payload.extend_from_slice(&0u32.to_le_bytes());
        legacy_payload.extend_from_slice(&0u32.to_le_bytes());
        legacy_payload.extend_from_slice(&0u32.to_le_bytes());
        legacy_payload.extend_from_slice(&250u32.to_le_bytes());
        legacy_payload.extend_from_slice(&10u32.to_le_bytes());
        legacy_payload.extend_from_slice(&(-3i32).to_le_bytes());

        let decoded = HealthReport::decode(&legacy_payload).unwrap();

        assert_eq!(decoded.buffer_depth_ms, 250);
        assert_eq!(decoded.output_buffer_ms, 0);
        assert_eq!(decoded.jitter_buffer_chunks, 0);
        assert_eq!(decoded.target_playout_latency_ms, 0);
        assert_eq!(decoded.callback_starvation_count, 0);
        assert_eq!(decoded.audio_callback_xrun_count, 0);
        assert_eq!(decoded.rtp_packets_received, 0);
        assert_eq!(decoded.rtp_sequence_gaps, 0);
        assert_eq!(decoded.rtp_decode_error_count, 0);
        assert_eq!(decoded.rtp_concealed_packets, 0);
    }
}
