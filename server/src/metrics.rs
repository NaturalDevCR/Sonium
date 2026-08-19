//! Prometheus metrics for Sonium server.
//!
//! All metrics are registered in the default Prometheus registry.
//! Exposed at `GET /metrics` (plain text, Prometheus scrape format).

use lazy_static::lazy_static;
use prometheus::{
    register_int_counter, register_int_counter_vec, register_int_gauge, register_int_gauge_vec,
    IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts,
};
use sonium_control::state::{StreamRecovery, StreamStatus};
use sonium_protocol::messages::{AudioHealthState, HealthReport};
use std::{collections::HashMap, sync::Mutex};

lazy_static! {
    /// Current session generation for each client ID. Held through metric
    /// cleanup so an old disconnect cannot delete a replacement's labels.
    static ref CLIENT_SESSION_GENERATIONS: Mutex<HashMap<String, u64>> = Mutex::new(HashMap::new());
    /// Number of TCP audio clients currently connected.
    pub static ref CONNECTED_CLIENTS: IntGauge =
        register_int_gauge!(Opts::new(
            "sonium_connected_clients",
            "Number of TCP audio clients currently connected"
        )).unwrap();

    /// Total TCP audio connections accepted since server start.
    pub static ref TOTAL_CONNECTIONS: IntCounter =
        register_int_counter!(Opts::new(
            "sonium_total_connections_total",
            "Total TCP audio client connections since server start"
        )).unwrap();

    /// Number of active WebSocket event-stream connections.
    pub static ref WS_CLIENTS: IntGauge =
        register_int_gauge!(Opts::new(
            "sonium_ws_clients",
            "Number of active WebSocket event-stream connections"
        )).unwrap();

    /// Stream status: 2=recovering, 1=playing, 0=idle, -1=error.
    pub static ref STREAM_STATUS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new("sonium_stream_status", "Stream status (2=recovering, 1=playing, 0=idle, -1=error)"),
            &["stream_id"]
        ).unwrap();

    /// Current consecutive reopen attempt, or zero outside recovery.
    pub static ref STREAM_RECOVERY_ATTEMPT: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_stream_recovery_attempt",
                "Current consecutive source reopen attempt (zero outside recovery)"
            ),
            &["stream_id"]
        ).unwrap();

    /// Delay before the next source reopen attempt, or zero outside recovery.
    pub static ref STREAM_RECOVERY_RETRY_MS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_stream_recovery_retry_ms",
                "Milliseconds before the next source reopen attempt (zero outside recovery)"
            ),
            &["stream_id"]
        ).unwrap();

    /// Encoded WireChunk frames broadcast per stream.
    pub static ref ENCODED_CHUNKS: IntCounterVec =
        register_int_counter_vec!(
            Opts::new("sonium_encoded_chunks_total", "WireChunk frames encoded and broadcast per stream"),
            &["stream_id"]
        ).unwrap();

    /// Server uptime in seconds (updated by the heartbeat task).
    pub static ref UPTIME_SECONDS: IntGauge =
        register_int_gauge!(Opts::new(
            "sonium_uptime_seconds",
            "Server uptime in seconds"
        )).unwrap();

    /// Client health reports received by transport.
    pub static ref CLIENT_HEALTH_REPORTS: IntCounterVec =
        register_int_counter_vec!(
            Opts::new(
                "sonium_client_health_reports_total",
                "Client health reports received by transport"
            ),
            &["transport"]
        ).unwrap();

    /// Latest client playback health state. Exactly one state label should be 1 per client/transport.
    pub static ref CLIENT_HEALTH_STATE: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_health_state",
                "Latest client playback health state (1=current, 0=inactive)"
            ),
            &["client_id", "transport", "state"]
        ).unwrap();

    /// Latest reported jitter-buffer depth in milliseconds.
    pub static ref CLIENT_BUFFER_DEPTH_MS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_buffer_depth_ms",
                "Latest reported client jitter-buffer depth in milliseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported output/player ring-buffer depth in milliseconds.
    pub static ref CLIENT_OUTPUT_BUFFER_MS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_output_buffer_ms",
                "Latest reported client output/player ring-buffer depth in milliseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported decoded chunk count in the jitter buffer.
    pub static ref CLIENT_JITTER_BUFFER_CHUNKS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_jitter_buffer_chunks",
                "Latest reported decoded chunk count in the client jitter buffer"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported target playout latency in milliseconds.
    pub static ref CLIENT_TARGET_PLAYOUT_LATENCY_MS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_target_playout_latency_ms",
                "Latest reported target playout latency in milliseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported count of audio callbacks that arrived much later than expected.
    pub static ref CLIENT_CALLBACK_STARVATIONS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_callback_starvations",
                "Latest reported count of audio callbacks that arrived much later than expected"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported count of output callback errors or xruns from the audio backend.
    pub static ref CLIENT_AUDIO_CALLBACK_XRUNS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_audio_callback_xruns",
                "Latest reported count of output callback errors or xruns from the audio backend"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported packet/chunk jitter estimate in milliseconds.
    pub static ref CLIENT_JITTER_MS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_jitter_ms",
                "Latest reported client packet/chunk jitter estimate in milliseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported underrun count.
    pub static ref CLIENT_UNDERRUNS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_underruns",
                "Latest reported client playback underrun count"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported stale/late chunk drop count.
    pub static ref CLIENT_STALE_DROPS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_stale_drops",
                "Latest reported client stale or late chunk drop count"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported output overrun count.
    pub static ref CLIENT_OVERRUNS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_overruns",
                "Latest reported client output overrun count"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest client clock offset estimate in milliseconds.
    pub static ref CLIENT_CLOCK_OFFSET_MS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_clock_offset_ms",
                "Latest reported client clock offset estimate in milliseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported RTP packets received by the client UDP media path.
    pub static ref CLIENT_RTP_PACKETS_RECEIVED: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_rtp_packets_received",
                "Latest reported RTP packets received by the client UDP media path"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported RTP sequence gaps detected by the client UDP media path.
    pub static ref CLIENT_RTP_SEQUENCE_GAPS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_rtp_sequence_gaps",
                "Latest reported RTP sequence gaps detected by the client UDP media path"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported RTP datagrams rejected by the client decoder.
    pub static ref CLIENT_RTP_DECODE_ERRORS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_rtp_decode_errors",
                "Latest reported RTP datagrams rejected by the client decoder"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported missing RTP packets concealed by the client decoder.
    pub static ref CLIENT_RTP_CONCEALED_PACKETS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_rtp_concealed_packets",
                "Latest reported missing RTP packets concealed by the client decoder"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported ARQ NACK packets sent by the client.
    pub static ref CLIENT_ARQ_NACKS_SENT: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_arq_nacks_sent",
                "Latest reported ARQ NACK packets sent by the client"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported retransmitted audio packets received by the client.
    pub static ref CLIENT_ARQ_RETRANSMIT_RECEIVED: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_arq_retransmit_received",
                "Latest reported retransmitted audio packets received by the client"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported audio packets recovered via FEC by the client.
    pub static ref CLIENT_ARQ_FEC_RECOVERED: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_arq_fec_recovered",
                "Latest reported audio packets recovered via FEC by the client"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported clock offset between client and server, microseconds.
    pub static ref CLIENT_CLOCK_OFFSET_US: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_clock_offset_us",
                "Latest reported client clock offset to server (Kalman estimate), microseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest reported group offset applied, microseconds (signed).
    pub static ref CLIENT_GROUP_OFFSET_US: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_group_offset_us",
                "Latest reported group offset applied at the client, microseconds (signed)"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Sync error vs the group target, microseconds (signed).
    pub static ref CLIENT_SYNC_ERROR_US: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_sync_error_us",
                "Latest reported sync error to the group target, microseconds (signed)"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Maximum absolute sync error across clients of a group, microseconds.
    pub static ref GROUP_SKEW_MAX_US: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_group_skew_max_us",
                "Maximum absolute sync error across all clients in a group, microseconds"
            ),
            &["group"]
        ).unwrap();

    /// Playout-error percentiles per client (p50/p95/p99), microseconds.
    pub static ref CLIENT_PLAYOUT_ERROR_P50_US: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_playout_error_p50_us",
                "P50 of |playout error| over rolling window, microseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    pub static ref CLIENT_PLAYOUT_ERROR_P95_US: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_playout_error_p95_us",
                "P95 of |playout error| over rolling window, microseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    pub static ref CLIENT_PLAYOUT_ERROR_P99_US: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_playout_error_p99_us",
                "P99 of |playout error| over rolling window, microseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// P99 of audio-callback duration, microseconds.
    pub static ref CLIENT_CALLBACK_DURATION_P99_US: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_callback_duration_p99_us",
                "P99 of cpal callback duration over rolling window, microseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Latest output device latency reported by the client audio backend, microseconds.
    pub static ref CLIENT_OUTPUT_LATENCY_US: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_output_latency_us",
                "Latest output device latency reported by the client audio backend, microseconds"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Combined ARQ+FEC packet recovery ratio in basis points (0–10000).
    pub static ref CLIENT_ARQ_FEC_RECOVERY_PCT_BP: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_arq_fec_recovery_pct_bp",
                "Combined ARQ+FEC packet recovery as a fraction of total loss (basis points, 0=0%, 10000=100%)"
            ),
            &["client_id", "transport"]
        ).unwrap();

    /// Active transport mode per client (1 for current label, 0 for others).
    pub static ref CLIENT_TRANSPORT_ACTIVE: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new(
                "sonium_client_transport_active",
                "Active transport mode per client (1=current, 0=inactive)"
            ),
            &["client_id", "transport"]
        ).unwrap();
}

/// Keep Prometheus synchronized with the canonical stream status stored by
/// `ServerState` and emitted through REST/WebSocket.
pub fn update_stream_status(
    stream_id: &str,
    status: StreamStatus,
    recovery: Option<StreamRecovery>,
) {
    let status_value = match status {
        StreamStatus::Playing => 1,
        StreamStatus::Idle => 0,
        StreamStatus::Recovering => 2,
        StreamStatus::Error => -1,
    };
    STREAM_STATUS
        .with_label_values(&[stream_id])
        .set(status_value);
    let (attempt, retry_in_ms) = recovery
        .map(|recovery| {
            (
                i64::from(recovery.attempt),
                i64::try_from(recovery.retry_in_ms).unwrap_or(i64::MAX),
            )
        })
        .unwrap_or((0, 0));
    STREAM_RECOVERY_ATTEMPT
        .with_label_values(&[stream_id])
        .set(attempt);
    STREAM_RECOVERY_RETRY_MS
        .with_label_values(&[stream_id])
        .set(retry_in_ms);
}

/// Register a newly admitted session before it can create client metric labels.
pub fn register_client_session(client_id: &str, generation: u64) {
    CLIENT_SESSION_GENERATIONS
        .lock()
        .expect("client metric session registry lock poisoned")
        .insert(client_id.into(), generation);
}

pub fn observe_client_health(
    client_id: &str,
    transport: &str,
    report: &HealthReport,
    state: AudioHealthState,
) {
    CLIENT_HEALTH_REPORTS.with_label_values(&[transport]).inc();
    CLIENT_BUFFER_DEPTH_MS
        .with_label_values(&[client_id, transport])
        .set(report.buffer_depth_ms as i64);
    CLIENT_OUTPUT_BUFFER_MS
        .with_label_values(&[client_id, transport])
        .set(report.output_buffer_ms as i64);
    CLIENT_JITTER_BUFFER_CHUNKS
        .with_label_values(&[client_id, transport])
        .set(report.jitter_buffer_chunks as i64);
    CLIENT_TARGET_PLAYOUT_LATENCY_MS
        .with_label_values(&[client_id, transport])
        .set(report.target_playout_latency_ms as i64);
    CLIENT_CALLBACK_STARVATIONS
        .with_label_values(&[client_id, transport])
        .set(report.callback_starvation_count as i64);
    CLIENT_AUDIO_CALLBACK_XRUNS
        .with_label_values(&[client_id, transport])
        .set(report.audio_callback_xrun_count as i64);
    CLIENT_JITTER_MS
        .with_label_values(&[client_id, transport])
        .set(report.jitter_ms as i64);
    CLIENT_UNDERRUNS
        .with_label_values(&[client_id, transport])
        .set(report.underrun_count as i64);
    CLIENT_STALE_DROPS
        .with_label_values(&[client_id, transport])
        .set(report.stale_drop_count as i64);
    CLIENT_OVERRUNS
        .with_label_values(&[client_id, transport])
        .set(report.overrun_count as i64);
    CLIENT_CLOCK_OFFSET_MS
        .with_label_values(&[client_id, transport])
        .set(report.latency_ms as i64);
    CLIENT_RTP_PACKETS_RECEIVED
        .with_label_values(&[client_id, transport])
        .set(report.rtp_packets_received as i64);
    CLIENT_RTP_SEQUENCE_GAPS
        .with_label_values(&[client_id, transport])
        .set(report.rtp_sequence_gaps as i64);
    CLIENT_RTP_DECODE_ERRORS
        .with_label_values(&[client_id, transport])
        .set(report.rtp_decode_error_count as i64);
    CLIENT_RTP_CONCEALED_PACKETS
        .with_label_values(&[client_id, transport])
        .set(report.rtp_concealed_packets as i64);
    CLIENT_ARQ_NACKS_SENT
        .with_label_values(&[client_id, transport])
        .set(report.arq_nacks_sent as i64);
    CLIENT_ARQ_RETRANSMIT_RECEIVED
        .with_label_values(&[client_id, transport])
        .set(report.arq_retransmit_received as i64);
    CLIENT_ARQ_FEC_RECOVERED
        .with_label_values(&[client_id, transport])
        .set(report.arq_fec_recovered as i64);
    CLIENT_CLOCK_OFFSET_US
        .with_label_values(&[client_id, transport])
        .set(report.clock_offset_us);
    CLIENT_GROUP_OFFSET_US
        .with_label_values(&[client_id, transport])
        .set(report.group_offset_us);
    CLIENT_SYNC_ERROR_US
        .with_label_values(&[client_id, transport])
        .set(report.sync_error_to_group_us);
    CLIENT_PLAYOUT_ERROR_P50_US
        .with_label_values(&[client_id, transport])
        .set(report.playout_error_us_p50 as i64);
    CLIENT_PLAYOUT_ERROR_P95_US
        .with_label_values(&[client_id, transport])
        .set(report.playout_error_us_p95 as i64);
    CLIENT_PLAYOUT_ERROR_P99_US
        .with_label_values(&[client_id, transport])
        .set(report.playout_error_us_p99 as i64);
    CLIENT_CALLBACK_DURATION_P99_US
        .with_label_values(&[client_id, transport])
        .set(report.callback_xrun_us_p99 as i64);
    CLIENT_OUTPUT_LATENCY_US
        .with_label_values(&[client_id, transport])
        .set(report.output_latency_us as i64);
    CLIENT_ARQ_FEC_RECOVERY_PCT_BP
        .with_label_values(&[client_id, transport])
        .set(report.arq_fec_combined_recovery_pct as i64);

    for candidate in AudioHealthState::ALL {
        CLIENT_HEALTH_STATE
            .with_label_values(&[client_id, transport, candidate.as_str()])
            .set(if candidate == state { 1 } else { 0 });
    }
}

/// Mark `transport` as the active mode for `client_id`. Sets the active gauge to 1
/// for this transport and clears it for any others previously seen.
pub fn observe_active_transport(client_id: &str, active_transport: &str) {
    for kind in ["tcp", "rtp_udp", "rist", "arq_udp", "quic_dgram"] {
        let val = if kind == active_transport { 1 } else { 0 };
        CLIENT_TRANSPORT_ACTIVE
            .with_label_values(&[client_id, kind])
            .set(val);
    }
}

/// Remove all per-client Prometheus series when a client disconnects or is evicted.
pub fn forget_client(client_id: &str, generation: u64) {
    const TRANSPORTS: [&str; 5] = ["tcp", "rtp_udp", "rist", "arq_udp", "quic_dgram"];

    // Keep this mutex through removal. A replacement session must register its
    // generation before creating labels, so it either wins first (and stale
    // cleanup is skipped) or starts only after old labels are removed.
    let mut sessions = CLIENT_SESSION_GENERATIONS
        .lock()
        .expect("client metric session registry lock poisoned");
    if sessions.get(client_id) != Some(&generation) {
        return;
    }
    sessions.remove(client_id);

    macro_rules! remove_transport_series {
        ($transport:expr, $($metric:ident),+ $(,)?) => {
            $(let _ = $metric.remove_label_values(&[client_id, $transport]);)+
        };
    }

    for transport in TRANSPORTS {
        remove_transport_series!(
            transport,
            CLIENT_BUFFER_DEPTH_MS,
            CLIENT_OUTPUT_BUFFER_MS,
            CLIENT_JITTER_BUFFER_CHUNKS,
            CLIENT_TARGET_PLAYOUT_LATENCY_MS,
            CLIENT_CALLBACK_STARVATIONS,
            CLIENT_AUDIO_CALLBACK_XRUNS,
            CLIENT_JITTER_MS,
            CLIENT_UNDERRUNS,
            CLIENT_STALE_DROPS,
            CLIENT_OVERRUNS,
            CLIENT_CLOCK_OFFSET_MS,
            CLIENT_RTP_PACKETS_RECEIVED,
            CLIENT_RTP_SEQUENCE_GAPS,
            CLIENT_RTP_DECODE_ERRORS,
            CLIENT_RTP_CONCEALED_PACKETS,
            CLIENT_ARQ_NACKS_SENT,
            CLIENT_ARQ_RETRANSMIT_RECEIVED,
            CLIENT_ARQ_FEC_RECOVERED,
            CLIENT_CLOCK_OFFSET_US,
            CLIENT_GROUP_OFFSET_US,
            CLIENT_SYNC_ERROR_US,
            CLIENT_PLAYOUT_ERROR_P50_US,
            CLIENT_PLAYOUT_ERROR_P95_US,
            CLIENT_PLAYOUT_ERROR_P99_US,
            CLIENT_CALLBACK_DURATION_P99_US,
            CLIENT_OUTPUT_LATENCY_US,
            CLIENT_ARQ_FEC_RECOVERY_PCT_BP,
            CLIENT_TRANSPORT_ACTIVE,
        );
        for state in AudioHealthState::ALL {
            let _ =
                CLIENT_HEALTH_STATE.remove_label_values(&[client_id, transport, state.as_str()]);
        }
    }
}

/// Update the per-group max absolute sync error in microseconds.
///
/// Wired into the adaptive engine in Phase E; for Phase A the gauge is
/// registered but populated only when the engine is enabled.
#[allow(dead_code)]
pub fn observe_group_skew_max(group: &str, max_abs_us: i64) {
    GROUP_SKEW_MAX_US
        .with_label_values(&[group])
        .set(max_abs_us);
}

/// Render all registered metrics as Prometheus text format.
pub fn gather() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let mut buf = Vec::new();
    encoder
        .encode(&prometheus::gather(), &mut buf)
        .unwrap_or(());
    String::from_utf8(buf).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonium_control::state::{StreamRecovery, StreamStatus};

    #[test]
    fn recovering_metric_has_distinct_state_and_retry_context() {
        update_stream_status(
            "metric-recovery-test",
            StreamStatus::Recovering,
            Some(StreamRecovery {
                attempt: 4,
                retry_in_ms: 400,
            }),
        );

        let metrics = gather();
        assert!(metrics.contains("sonium_stream_status{stream_id=\"metric-recovery-test\"} 2"));
        assert!(metrics
            .contains("sonium_stream_recovery_attempt{stream_id=\"metric-recovery-test\"} 4"));
        assert!(metrics
            .contains("sonium_stream_recovery_retry_ms{stream_id=\"metric-recovery-test\"} 400"));
    }

    #[test]
    fn forget_client_removes_client_labelled_series() {
        let id = "metrics-cleanup-client";
        let report = HealthReport::new(0, 0, 0, 100, 0, 0);

        register_client_session(id, 1);
        observe_client_health(id, "tcp", &report, AudioHealthState::Stable);
        observe_active_transport(id, "tcp");
        assert!(gather().contains(&format!("client_id=\"{id}\"")));

        forget_client(id, 1);

        assert!(!gather().contains(&format!("client_id=\"{id}\"")));
    }

    #[test]
    fn stale_cleanup_does_not_remove_reconnected_client_labels() {
        let id = "metrics-reconnect-race-client";
        let report = HealthReport::new(0, 0, 0, 100, 0, 0);

        register_client_session(id, 1);
        observe_client_health(id, "tcp", &report, AudioHealthState::Stable);
        observe_active_transport(id, "tcp");
        // A reconnect has already created its replacement labels when the old
        // session cleanup finally runs.
        register_client_session(id, 2);
        observe_client_health(id, "tcp", &report, AudioHealthState::Stable);
        observe_active_transport(id, "tcp");

        forget_client(id, 1);

        assert!(gather().contains(&format!("client_id=\"{id}\"")));
        forget_client(id, 2);
    }
}
