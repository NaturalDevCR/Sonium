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
}

impl AudioHealthState {
    pub const ALL: [Self; 7] = [
        Self::Buffering,
        Self::Stable,
        Self::Degraded,
        Self::Recovering,
        Self::Underrun,
        Self::Fallback,
        Self::Offline,
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
    ) -> Self {
        self.rtp_packets_received = packets_received;
        self.rtp_sequence_gaps = sequence_gaps;
        self.rtp_decode_error_count = decode_error_count;
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
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = WireWrite::with_capacity(56);
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
            .with_rtp_metrics(100, 3, 1);
        let decoded = HealthReport::decode(&original.encode()).unwrap();

        assert_eq!(decoded.output_buffer_ms, 180);
        assert_eq!(decoded.jitter_buffer_chunks, 6);
        assert_eq!(decoded.target_playout_latency_ms, 500);
        assert_eq!(decoded.callback_starvation_count, 2);
        assert_eq!(decoded.audio_callback_xrun_count, 1);
        assert_eq!(decoded.rtp_packets_received, 100);
        assert_eq!(decoded.rtp_sequence_gaps, 3);
        assert_eq!(decoded.rtp_decode_error_count, 1);
        assert_eq!(decoded.total_playout_queue_ms(), 300);
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
    }
}
