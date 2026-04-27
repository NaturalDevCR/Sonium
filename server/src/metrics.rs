//! Prometheus metrics for Sonium server.
//!
//! All metrics are registered in the default Prometheus registry.
//! Exposed at `GET /metrics` (plain text, Prometheus scrape format).

use lazy_static::lazy_static;
use prometheus::{
    IntCounter, IntCounterVec, IntGauge, IntGaugeVec,
    Opts, register_int_counter, register_int_counter_vec,
    register_int_gauge, register_int_gauge_vec,
};

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
}

/// Render all registered metrics as Prometheus text format.
pub fn gather() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let mut buf = Vec::new();
    encoder.encode(&prometheus::gather(), &mut buf).unwrap_or(());
    String::from_utf8(buf).unwrap_or_default()
}
