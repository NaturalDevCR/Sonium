use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::SampleFormat;

/// Top-level config loaded from `sonium.toml` (or defaults — no file required).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub server: ServerNet,
    pub stream: StreamDefaults,
    pub log: LogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerNet {
    pub bind: String,
    /// TCP port for audio stream protocol (Snapcast-compatible).
    pub stream_port: u16,
    /// HTTP/WS port for control API + web UI.
    pub control_port: u16,
    pub mdns: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamDefaults {
    pub codec: String,
    pub sample_format: SampleFormat,
    /// Milliseconds of jitter buffer on client side.
    pub buffer_ms: u32,
    pub pipe: Option<PathBuf>,
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
            stream: StreamDefaults::default(),
            log: LogConfig::default(),
        }
    }
}

impl Default for ServerNet {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0".into(),
            stream_port: 1704,
            control_port: 1780,
            mdns: true,
        }
    }
}

impl Default for StreamDefaults {
    fn default() -> Self {
        Self {
            codec: "opus".into(),
            sample_format: SampleFormat::default(),
            buffer_ms: 1000,
            pipe: None,
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
}

/// Client-side configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientConfig {
    pub server_host: String,
    pub server_port: u16,
    /// Extra latency offset in ms (useful for Bluetooth sinks).
    pub latency_ms: i32,
    pub log: LogConfig,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_host: "127.0.0.1".into(),
            server_port: 1704,
            latency_ms: 0,
            log: LogConfig::default(),
        }
    }
}
