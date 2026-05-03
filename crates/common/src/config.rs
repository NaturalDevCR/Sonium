use serde::{Deserialize, Serialize};
use sonium_transport::TransportConfig;

use crate::SampleFormat;

/// Top-level config loaded from `sonium.toml` (or defaults — no file required).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub server: ServerNet,
    /// One entry per audio stream source.  The first entry is the "default" stream.
    pub streams: Vec<StreamSource>,
    pub log: LogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerNet {
    pub bind: String,
    /// TCP port for audio stream protocol.
    pub stream_port: u16,
    /// HTTP/WS port for control API + web UI.
    pub control_port: u16,
    pub mdns: bool,
    /// When true: advertise `_snapcast._tcp` via mDNS so legacy Snapcast
    /// clients can discover this server.  Also useful if you want to use
    /// Sonium as a drop-in replacement on an existing Snapcast setup.
    /// Ports must also be set to 1704/1780 manually for full compatibility.
    pub snapcast_compat: bool,
    /// Global jitter buffer suggested to connected clients unless a stream overrides it.
    pub buffer_ms: u32,
    /// Global encoded audio chunk duration unless a stream overrides it.
    pub chunk_ms: u32,
    /// Local output-device prefill in milliseconds (`0` = derive from buffer_ms).
    ///
    /// This is intentionally separate from `buffer_ms`: `buffer_ms` absorbs
    /// network jitter, while this keeps the client's audio backend ring fed.
    pub output_prefill_ms: u32,
    /// Enable server-side automatic jitter buffer tuning per client session.
    pub auto_buffer: bool,
    /// Lower clamp for auto-tuned buffer target.
    pub auto_buffer_min_ms: u32,
    /// Upper clamp for auto-tuned buffer target.
    pub auto_buffer_max_ms: u32,
    /// Buffer increase step used when health degrades.
    pub auto_buffer_step_up_ms: u32,
    /// Buffer decrease step used during stable playback.
    pub auto_buffer_step_down_ms: u32,
    /// Minimum delay between auto-buffer adjustments.
    pub auto_buffer_cooldown_ms: u64,
    /// Media transport selection.  See [`TransportConfig`] for options.
    /// Defaults to `tcp`; future modes (`rtp_udp`, `quic_dgram`) require
    /// Phase 2+ implementations before they can be enabled.
    pub transport: TransportConfig,
}

/// One audio source that the server encodes and broadcasts.
///
/// In `sonium.toml` use an array of tables:
/// ```toml
/// [[streams]]
/// id     = "default"
/// source = "-"          # stdin
///
/// [[streams]]
/// id     = "kitchen"
/// source = "/tmp/sonium-kitchen.fifo"
/// codec  = "pcm"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamSource {
    /// Unique stream identifier.  Must match a group's `stream_id`.
    pub id: String,
    /// Optional friendly name shown in the web UI.
    pub display_name: Option<String>,
    /// Input source.  Supported formats:
    /// - `"-"` — stdin (raw PCM)
    /// - `/path/to/file.pcm` or `/tmp/fifo` — file or named FIFO (raw PCM)
    /// - `pipe:///usr/bin/ffmpeg?-i&song.mp3&-f&s16le&-` — external process
    ///   (command path after `pipe://`, arguments separated by `&`)
    /// - `tcp://host:port` — connect to a TCP sender that outputs raw PCM
    /// - `tcp-listen://0.0.0.0:4953` — listen for TCP senders
    /// - `tcp://0.0.0.0:4953?mode=server` — Snapcast-style TCP listener
    pub source: String,
    pub codec: String,
    pub sample_format: SampleFormat,
    /// Optional per-stream jitter buffer override.
    pub buffer_ms: Option<u32>,
    /// Encoded audio frame duration in milliseconds. Smaller chunks reduce
    /// scheduling latency; larger chunks reduce packet overhead.
    pub chunk_ms: Option<u32>,
    /// After this many milliseconds of no input data, mark stream as Idle.
    /// `None` disables idle detection (stream stays in whatever state main.rs set).
    pub idle_timeout_ms: Option<u32>,
    /// When `idle_timeout_ms` fires, emit silence frames so connected clients
    /// don't buffer-underrun while waiting for audio to return.
    pub silence_on_idle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: ServerNet::default(),
            streams: vec![StreamSource::default()],
            log: LogConfig::default(),
        }
    }
}

impl Default for ServerNet {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0".into(),
            stream_port: 1710,
            control_port: 1711,
            mdns: true,
            snapcast_compat: false,
            buffer_ms: 200,
            chunk_ms: 10,
            output_prefill_ms: 0,
            auto_buffer: false,
            auto_buffer_min_ms: 20,
            auto_buffer_max_ms: 3000,
            auto_buffer_step_up_ms: 120,
            auto_buffer_step_down_ms: 40,
            auto_buffer_cooldown_ms: 8_000,
            transport: TransportConfig::default(),
        }
    }
}

impl Default for StreamSource {
    fn default() -> Self {
        Self {
            id: "default".into(),
            display_name: None,
            source: "-".into(),
            codec: "opus".into(),
            sample_format: SampleFormat::default(),
            buffer_ms: None,
            chunk_ms: None,
            idle_timeout_ms: None,
            silence_on_idle: false,
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
        }
    }
}

impl ServerConfig {
    pub fn from_file(path: &std::path::Path) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::SoniumError::Config(format!("cannot read config: {e}")))?;
        toml::from_str(&content)
            .map_err(|e| crate::SoniumError::Config(format!("invalid TOML: {e}")))
    }

    pub fn from_file_or_default(path: &std::path::Path) -> Self {
        Self::from_file(path).unwrap_or_default()
    }

    /// Returns the first stream, or a default `StreamSource` if none are configured.
    pub fn default_stream(&self) -> StreamSource {
        self.streams.first().cloned().unwrap_or_default()
    }

    pub fn effective_buffer_ms(&self, stream: &StreamSource) -> u32 {
        stream.buffer_ms.unwrap_or(self.server.buffer_ms)
    }

    pub fn effective_chunk_ms(&self, stream: &StreamSource) -> u32 {
        stream.chunk_ms.unwrap_or(self.server.chunk_ms)
    }
}

/// Client-side configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientConfig {
    pub server_host: String,
    pub server_port: u16,
    /// Extra latency offset in ms (useful for Bluetooth sinks).
    pub latency_ms: i32,
    /// Optional display name shown in the web UI. Falls back to hostname if None.
    pub client_name: Option<String>,
    /// Optional audio output device name (substring match, case-insensitive).
    /// When set, the player will select the first output device whose name
    /// contains this string.  Useful for loopback testing with virtual cables.
    pub device: Option<String>,
    /// The instance ID, useful for running multiple isolated clients on the same host.
    pub instance: u32,
    pub log: LogConfig,
    /// Enable the new callback-driven playout path with precise drift correction.
    pub experimental_callback: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_host: "127.0.0.1".into(),
            server_port: 1710,
            latency_ms: 0,
            client_name: None,
            device: None,
            instance: 1,
            log: LogConfig::default(),
            experimental_callback: true,
        }
    }
}

impl ClientConfig {
    pub fn from_file(path: &std::path::Path) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::SoniumError::Config(format!("cannot read client config: {e}")))?;
        toml::from_str(&content)
            .map_err(|e| crate::SoniumError::Config(format!("invalid TOML: {e}")))
    }

    pub fn from_file_or_default(path: &std::path::Path) -> Self {
        Self::from_file(path).unwrap_or_default()
    }
}
