use serde::{Deserialize, Serialize};

use crate::SampleFormat;

/// Top-level config loaded from `sonium.toml` (or defaults — no file required).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub server:  ServerNet,
    /// One entry per audio stream source.  The first entry is the "default" stream.
    pub streams: Vec<StreamSource>,
    pub log:     LogConfig,
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
    /// Input source.  Supported formats:
    /// - `"-"` — stdin (raw PCM)
    /// - `/path/to/file.pcm` or `/tmp/fifo` — file or named FIFO (raw PCM)
    /// - `pipe:///usr/bin/ffmpeg?-i&song.mp3&-f&s16le&-` — external process
    ///   (command path after `pipe://`, arguments separated by `&`)
    pub source: String,
    pub codec:  String,
    pub sample_format: SampleFormat,
    /// Milliseconds of jitter buffer suggested to connected clients.
    pub buffer_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server:  ServerNet::default(),
            streams: vec![StreamSource::default()],
            log:     LogConfig::default(),
        }
    }
}

impl Default for ServerNet {
    fn default() -> Self {
        Self {
            bind:           "0.0.0.0".into(),
            stream_port:    1710,
            control_port:   1711,
            mdns:           true,
            snapcast_compat: false,
        }
    }
}

impl Default for StreamSource {
    fn default() -> Self {
        Self {
            id:            "default".into(),
            source:        "-".into(),
            codec:         "opus".into(),
            sample_format: SampleFormat::default(),
            buffer_ms:     1000,
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self { level: "info".into() }
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
    pub log: LogConfig,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_host: "127.0.0.1".into(),
            server_port: 1710,
            latency_ms:  0,
            client_name: None,
            device:      None,
            log: LogConfig::default(),
        }
    }
}
