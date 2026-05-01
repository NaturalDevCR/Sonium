//! Prometheus metrics for Sonium server.
//!
//! All metrics are registered in the default Prometheus registry.
//! Exposed at `GET /metrics` (plain text, Prometheus scrape format).

use lazy_static::lazy_static;
use prometheus::{
    register_int_counter, register_int_counter_vec, register_int_gauge, register_int_gauge_vec,
    IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts,
};
use sonium_protocol::messages::{AudioHealthState, HealthReport};

lazy_static! {
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

    /// Stream status: 1=playing, 0=idle, -1=error, per stream_id label.
    pub static ref STREAM_STATUS: IntGaugeVec =
        register_int_gauge_vec!(
            Opts::new("sonium_stream_status", "Stream status (1=playing, 0=idle, -1=error)"),
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

    for candidate in AudioHealthState::ALL {
        CLIENT_HEALTH_STATE
            .with_label_values(&[client_id, transport, candidate.as_str()])
            .set(if candidate == state { 1 } else { 0 });
    }
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
