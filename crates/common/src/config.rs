use anyhow::Context;
use serde::{Deserialize, Serialize};
use sonium_transport::{TransportConfig, TransportMode};

use crate::SampleFormat;

/// Error returned when a protocol value is rejected before it can affect
/// server state or metric labels.
pub type ProtocolError = crate::SoniumError;

/// Largest permitted stable client identifier in bytes.
///
/// Sonium derives IDs as `hostname-instance`; 96 bytes leaves room for the
/// longest practical hostname and instance suffix while keeping externally
/// supplied state keys small.
pub const MAX_CLIENT_ID_LEN: usize = 96;

/// Validate a client-supplied stable identifier before admitting a session.
///
/// This is an input bound, not client authentication. Media authentication is
/// deliberately outside this trusted-LAN phase.
pub fn validate_client_id(id: &str) -> Result<(), ProtocolError> {
    if id.is_empty() || id.len() > MAX_CLIENT_ID_LEN {
        return Err(ProtocolError::Protocol(
            "client ID must be 1 to 96 ASCII bytes".into(),
        ));
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ProtocolError::Protocol(
            "client ID may contain only ASCII letters, digits, '-' and '_'".into(),
        ));
    }
    Ok(())
}

/// Top-level config loaded from `sonium.toml` (or defaults — no file required).
///
/// Example layout:
/// ```toml
/// [server]
/// bind         = "0.0.0.0"
/// control_bind = "127.0.0.1"
/// stream_port  = 1710
/// control_port = 1711
/// max_clients = 64
/// max_known_clients = 256
/// mdns         = true
/// snapcast_compat = false
///
/// [server.audio]
/// buffer_ms         = 200
/// chunk_ms          = 10
/// output_prefill_ms = 0
///
/// [server.auto_buffer]
/// enabled       = false
/// min_ms        = 20
/// max_ms        = 3000
/// step_up_ms    = 120
/// step_down_ms  = 40
/// cooldown_ms   = 8000
///
/// [server.transport]
/// mode     = "tcp"
/// udp_port = 0
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub server: ServerNet,
    /// One entry per audio stream source.  The first entry is the "default" stream.
    pub streams: Vec<StreamSource>,
    pub log: LogConfig,
    /// IANA timezone identifier for log timestamps and web UI display.
    /// e.g. "America/Costa_Rica", "Europe/Berlin", "UTC".
    /// Defaults to system local time if not set.
    pub timezone: Option<String>,
}

/// Network and feature flags for the server.
///
/// Audio, auto-buffer, and transport are in dedicated sub-sections so the
/// `[server]` table stays small and readable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerNet {
    pub bind: String,
    /// Bind address for the control API, web UI, and metrics endpoint.
    ///
    /// This intentionally defaults to loopback because the control plane is
    /// authenticated but should not be exposed accidentally on every adapter.
    pub control_bind: String,
    /// TCP port for the audio stream protocol.
    pub stream_port: u16,
    /// HTTP/WS port for the control API and web UI.
    pub control_port: u16,
    /// Maximum concurrent TCP client sessions, including clients awaiting Hello.
    pub max_clients: usize,
    /// Maximum remembered clients. When full, the least-recently-seen
    /// disconnected client is evicted; connected clients are never evicted.
    pub max_known_clients: usize,
    /// Advertise via mDNS so clients can discover the server automatically.
    pub mdns: bool,
    /// Advertise `_snapcast._tcp` for legacy Snapcast client discovery.
    /// Set ports to 1704/1780 manually for full wire compatibility.
    pub snapcast_compat: bool,

    /// Audio timing settings (`[server.audio]`).
    pub audio: AudioConfig,
    /// Automatic per-client jitter-buffer tuning (`[server.auto_buffer]`).
    pub auto_buffer: AutoBufferConfig,
    /// Media transport selection (`[server.transport]`).
    pub transport: TransportConfig,
}

/// Audio timing knobs — buffer, chunk size, and output prefill.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    /// Global jitter buffer suggested to clients unless a stream overrides it.
    pub buffer_ms: u32,
    /// Global encoded audio chunk duration unless a stream overrides it.
    /// Smaller → lower latency; larger → less packet overhead.
    pub chunk_ms: u32,
    /// Output-device prefill in ms (`0` = derive from `buffer_ms`).
    ///
    /// Intentionally separate from `buffer_ms`: `buffer_ms` absorbs network
    /// jitter while this keeps the client audio ring fed ahead of the DAC.
    pub output_prefill_ms: u32,
}

/// Server-side automatic jitter-buffer tuning.
///
/// When `enabled`, the server monitors each client's health reports and
/// nudges `buffer_ms` up on degradation and down during sustained stability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AutoBufferConfig {
    pub enabled: bool,
    /// Minimum buffer the auto-tuner will set (ms).
    pub min_ms: u32,
    /// Maximum buffer the auto-tuner will set (ms).
    pub max_ms: u32,
    /// Buffer increase step on health degradation (ms).
    pub step_up_ms: u32,
    /// Buffer decrease step during stable playback (ms).
    pub step_down_ms: u32,
    /// Minimum interval between adjustments (ms).
    pub cooldown_ms: u64,
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
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct LogConfig {
    pub level: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: ServerNet::default(),
            streams: vec![StreamSource::default()],
            log: LogConfig::default(),
            timezone: None,
        }
    }
}

impl Default for ServerNet {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0".into(),
            control_bind: "127.0.0.1".into(),
            stream_port: 1710,
            control_port: 1711,
            max_clients: 64,
            max_known_clients: 256,
            mdns: true,
            snapcast_compat: false,
            audio: AudioConfig::default(),
            auto_buffer: AutoBufferConfig::default(),
            transport: TransportConfig::default(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            buffer_ms: 200,
            chunk_ms: 10,
            output_prefill_ms: 0,
        }
    }
}

impl Default for AutoBufferConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_ms: 20,
            max_ms: 3000,
            step_up_ms: 120,
            step_down_ms: 40,
            cooldown_ms: 8_000,
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
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read server configuration {}", path.display()))?;
        Self::from_toml(&content, path)
    }

    /// Load a configuration file, using defaults only when the file is absent.
    pub fn from_file_or_default(path: &std::path::Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(content) => Self::from_toml(&content, path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error)
                .with_context(|| format!("cannot read server configuration {}", path.display())),
        }
    }

    fn from_toml(content: &str, path: &std::path::Path) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(content)
            .with_context(|| format!("invalid TOML in server configuration {}", path.display()))?;
        config
            .validate()
            .with_context(|| format!("invalid server configuration {}", path.display()))?;
        Ok(config)
    }

    /// Check values that TOML's type system cannot express safely.
    pub fn validate(&self) -> anyhow::Result<()> {
        self.server
            .bind
            .parse::<std::net::IpAddr>()
            .with_context(|| "server.bind must be an IP address")?;
        self.server
            .control_bind
            .parse::<std::net::IpAddr>()
            .with_context(|| "server.control_bind must be an IP address")?;
        if self.server.stream_port == 0 {
            anyhow::bail!("server.stream_port must be between 1 and 65535");
        }
        if self.server.control_port == 0 {
            anyhow::bail!("server.control_port must be between 1 and 65535");
        }
        if self.server.stream_port == self.server.control_port {
            anyhow::bail!("server.stream_port and server.control_port must differ");
        }
        if !(1..=256).contains(&self.server.max_clients) {
            anyhow::bail!("server.max_clients must be between 1 and 256");
        }
        if !(self.server.max_clients..=1024).contains(&self.server.max_known_clients) {
            anyhow::bail!("server.max_known_clients must be between server.max_clients and 1024");
        }

        validate_buffer_and_chunk(
            "server.audio",
            self.server.audio.buffer_ms,
            self.server.audio.chunk_ms,
            None,
        )?;
        if matches!(
            self.server.transport.mode,
            TransportMode::RtpUdp | TransportMode::Rist
        ) && self.server.transport.udp_port == 0
            && self.server.stream_port > u16::MAX - 2
        {
            anyhow::bail!(
                "server.stream_port must be at most {} when UDP transport chooses stream_port + 2",
                u16::MAX - 2
            );
        }

        let mut largest_chunk_ms = self.server.audio.chunk_ms;
        for stream in &self.streams {
            let effective_chunk_ms = self.effective_chunk_ms(stream);
            validate_buffer_and_chunk(
                &format!("stream `{}`", stream.id),
                self.effective_buffer_ms(stream),
                effective_chunk_ms,
                Some(stream.codec.as_str()),
            )?;
            validate_stream_format(stream, effective_chunk_ms)?;
            largest_chunk_ms = largest_chunk_ms.max(effective_chunk_ms);
        }
        validate_auto_buffer(&self.server.auto_buffer, largest_chunk_ms)?;
        validate_log_config(&self.log)?;

        Ok(())
    }

    /// Returns the first stream, or a default `StreamSource` if none are configured.
    pub fn default_stream(&self) -> StreamSource {
        self.streams.first().cloned().unwrap_or_default()
    }

    pub fn effective_buffer_ms(&self, stream: &StreamSource) -> u32 {
        stream.buffer_ms.unwrap_or(self.server.audio.buffer_ms)
    }

    pub fn effective_chunk_ms(&self, stream: &StreamSource) -> u32 {
        stream.chunk_ms.unwrap_or(self.server.audio.chunk_ms)
    }
}

fn validate_buffer_and_chunk(
    scope: &str,
    buffer_ms: u32,
    chunk_ms: u32,
    codec: Option<&str>,
) -> anyhow::Result<()> {
    if !(10..=60).contains(&chunk_ms) {
        anyhow::bail!("{scope}.chunk_ms must be between 10 and 60 ms");
    }
    if matches!(codec, Some("opus")) && !matches!(chunk_ms, 10 | 20 | 40 | 60) {
        anyhow::bail!("{scope}.chunk_ms must be 10, 20, 40, or 60 ms for opus");
    }
    if buffer_ms > 0 && buffer_ms < chunk_ms {
        anyhow::bail!("{scope}.buffer_ms must be zero or at least chunk_ms");
    }
    if buffer_ms > i32::MAX as u32 {
        anyhow::bail!("{scope}.buffer_ms exceeds the protocol's signed millisecond range");
    }
    Ok(())
}

fn validate_auto_buffer(config: &AutoBufferConfig, largest_chunk_ms: u32) -> anyhow::Result<()> {
    if config.min_ms > config.max_ms {
        anyhow::bail!("server.auto_buffer.min_ms must not exceed max_ms");
    }
    if config.step_up_ms == 0 || config.step_down_ms == 0 {
        anyhow::bail!("server.auto_buffer step sizes must be greater than zero");
    }
    if config.cooldown_ms == 0 {
        anyhow::bail!("server.auto_buffer.cooldown_ms must be greater than zero");
    }
    if config.max_ms > i32::MAX as u32 {
        anyhow::bail!("server.auto_buffer.max_ms exceeds the protocol's signed millisecond range");
    }
    if config.enabled && config.min_ms < largest_chunk_ms {
        anyhow::bail!(
            "server.auto_buffer.min_ms must be at least the largest effective stream chunk ({largest_chunk_ms} ms)"
        );
    }
    Ok(())
}

fn validate_stream_format(stream: &StreamSource, chunk_ms: u32) -> anyhow::Result<()> {
    let format = stream.sample_format;
    if format.rate == 0 {
        anyhow::bail!(
            "stream `{}` sample_format.rate must be greater than zero",
            stream.id
        );
    }
    if format.channels == 0 {
        anyhow::bail!(
            "stream `{}` sample_format.channels must be greater than zero",
            stream.id
        );
    }
    if format.bits != 16 {
        anyhow::bail!(
            "stream `{}` sample_format.bits must be 16 because the input reader consumes i16 PCM",
            stream.id
        );
    }
    match stream.codec.as_str() {
        "opus" => {
            if !matches!(format.rate, 8_000 | 12_000 | 16_000 | 24_000 | 48_000) {
                anyhow::bail!(
                    "stream `{}` opus sample_format.rate must be one of 8000, 12000, 16000, 24000, or 48000",
                    stream.id
                );
            }
            if !matches!(format.channels, 1 | 2) {
                anyhow::bail!(
                    "stream `{}` opus sample_format.channels must be 1 or 2",
                    stream.id
                );
            }
        }
        "pcm" => {}
        "flac" => validate_flac_format(stream, chunk_ms)?,
        codec => anyhow::bail!("stream `{}` has unsupported codec `{codec}`", stream.id),
    }
    Ok(())
}

fn validate_flac_format(stream: &StreamSource, chunk_ms: u32) -> anyhow::Result<()> {
    // flacenc 0.5.1 verifies StreamInfo against these codec limits. Keeping
    // them here prevents startup from reaching the encoder with an unsupported
    // format or allocating a frame outside the codec's maximum input shape.
    const FLAC_MAX_SAMPLE_RATE: u32 = 96_000;
    const FLAC_MAX_CHANNELS: u16 = 8;
    const FLAC_MIN_BLOCK_SIZE: u64 = 32;
    const FLAC_MAX_BLOCK_SIZE: u64 = 32_767;
    const FLAC_ENCODER_FRAME_MS: u64 = 20;

    let format = stream.sample_format;
    if format.rate > FLAC_MAX_SAMPLE_RATE {
        anyhow::bail!(
            "stream `{}` flac sample_format.rate must not exceed {FLAC_MAX_SAMPLE_RATE}",
            stream.id
        );
    }
    if format.channels > FLAC_MAX_CHANNELS {
        anyhow::bail!(
            "stream `{}` flac sample_format.channels must not exceed {FLAC_MAX_CHANNELS}",
            stream.id
        );
    }

    let encoder_block_size = u64::from(format.rate)
        .checked_mul(FLAC_ENCODER_FRAME_MS)
        .ok_or_else(|| anyhow::anyhow!("stream `{}` flac block size overflows", stream.id))?
        / 1_000;
    if !(FLAC_MIN_BLOCK_SIZE..=FLAC_MAX_BLOCK_SIZE).contains(&encoder_block_size) {
        anyhow::bail!(
            "stream `{}` flac encoder block size must be between {FLAC_MIN_BLOCK_SIZE} and {FLAC_MAX_BLOCK_SIZE} samples",
            stream.id
        );
    }

    let frame_bytes = u64::from(format.rate)
        .checked_mul(u64::from(chunk_ms))
        .and_then(|samples_per_second| samples_per_second.checked_div(1_000))
        .and_then(|frames| frames.checked_mul(u64::from(format.channels)))
        .and_then(|samples| samples.checked_mul(2))
        .ok_or_else(|| anyhow::anyhow!("stream `{}` frame size overflows", stream.id))?;
    usize::try_from(frame_bytes).map_err(|_| {
        anyhow::anyhow!(
            "stream `{}` frame size does not fit this platform",
            stream.id
        )
    })?;
    Ok(())
}

fn validate_log_config(log: &LogConfig) -> anyhow::Result<()> {
    if !matches!(
        log.level.as_str(),
        "trace" | "debug" | "info" | "warn" | "error"
    ) {
        anyhow::bail!("log.level must be trace, debug, info, warn, or error");
    }
    Ok(())
}

/// Client-side configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
    /// IANA timezone identifier for log timestamps and UI display.
    /// e.g. "America/Costa_Rica", "Europe/Berlin", "UTC".
    /// Defaults to system local time if not set.
    pub timezone: Option<String>,
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
            timezone: None,
        }
    }
}

impl ClientConfig {
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read client configuration {}", path.display()))?;
        Self::from_toml(&content, path)
    }

    /// Load a configuration file, using defaults only when the file is absent.
    pub fn from_file_or_default(path: &std::path::Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(content) => Self::from_toml(&content, path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error)
                .with_context(|| format!("cannot read client configuration {}", path.display())),
        }
    }

    fn from_toml(content: &str, path: &std::path::Path) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(content)
            .with_context(|| format!("invalid TOML in client configuration {}", path.display()))?;
        config
            .validate()
            .with_context(|| format!("invalid client configuration {}", path.display()))?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.server_host.trim().is_empty() {
            anyhow::bail!("server_host must not be empty");
        }
        if self.server_port == 0 {
            anyhow::bail!("server_port must be between 1 and 65535");
        }
        validate_log_config(&self.log)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT_TEST_FILE: AtomicUsize = AtomicUsize::new(0);

    fn write_config(name: &str, content: &str) -> PathBuf {
        let sequence = NEXT_TEST_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sonium-config-{name}-{}-{sequence}.toml",
            std::process::id()
        ));
        fs::write(&path, content).expect("write test configuration");
        path
    }

    fn installer_config() -> String {
        const INSTALLER: &str = include_str!("../../../install.sh");
        let start = INSTALLER
            .find("cat > \"${CONF_DIR}/sonium.toml\" <<EOF\n")
            .expect("installer config heredoc start")
            + "cat > \"${CONF_DIR}/sonium.toml\" <<EOF\n".len();
        let end = INSTALLER[start..]
            .find("\nEOF\n")
            .expect("installer config heredoc end")
            + start;

        INSTALLER[start..end]
            .replace("${STREAM_PORT}", "1710")
            .replace("${CONTROL_PORT}", "1711")
            .replace("${FIFO_PATH}", "/tmp/sonium.fifo")
    }

    #[test]
    fn test_deserialize_new_structure() {
        let toml_str = r#"
[server]
bind         = "0.0.0.0"
stream_port  = 1710
control_port = 1711
mdns         = true

[server.audio]
buffer_ms         = 200
chunk_ms          = 10
output_prefill_ms = 0

[server.auto_buffer]
enabled       = false
min_ms        = 20
max_ms        = 3000
step_up_ms    = 120
step_down_ms  = 40
cooldown_ms   = 8000

[server.transport]
mode     = "tcp"
udp_port = 0

[[streams]]
id     = "default"
source = "-"
"#;
        let cfg: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.server.audio.buffer_ms, 200);
        assert!(!cfg.server.auto_buffer.enabled);
        assert_eq!(
            cfg.server.transport.mode,
            sonium_transport::TransportMode::Tcp
        );
    }

    #[test]
    fn defaults_keep_control_listener_on_loopback_with_a_bounded_client_cap() {
        let cfg = ServerConfig::default();
        let control_bind: std::net::IpAddr = cfg
            .server
            .control_bind
            .parse()
            .expect("the default control bind must be an IP address");

        assert!(control_bind.is_loopback());
        assert!((1..=256).contains(&cfg.server.max_clients));
        assert!((cfg.server.max_clients..=1024).contains(&cfg.server.max_known_clients));
    }

    #[test]
    fn explicit_config_rejects_unknown_nested_fields() {
        for (name, content) in [
            (
                "unknown-audio-field",
                "[server.audio]\nbuffer_ms = 200\nunexpected = true\n",
            ),
            (
                "unknown-transport-field",
                "[server.transport]\nunexpected = true\n",
            ),
            (
                "unknown-sample-format-field",
                "[[streams]]\nsample_format = { rate = 48000, bits = 16, channels = 2, unexpected = true }\n",
            ),
        ] {
            let path = write_config(name, content);
            let error = ServerConfig::from_file(&path).expect_err("unknown field must be rejected");
            assert!(
                format!("{error:#}").contains("unknown field `unexpected`"),
                "unexpected error: {error:#}"
            );
            fs::remove_file(path).expect("remove test configuration");
        }
    }

    #[test]
    fn explicit_malformed_config_reports_its_path() {
        let path = write_config("malformed", "[server\nstream_port = 1710");

        let error = ServerConfig::from_file(&path).expect_err("malformed TOML must be rejected");
        assert!(
            error.to_string().contains(&path.display().to_string()),
            "error must identify the rejected config path: {error:#}"
        );
        fs::remove_file(path).expect("remove test configuration");
    }

    #[test]
    fn explicit_config_rejects_invalid_port_buffer_chunk_and_format_combinations() {
        for (name, content) in [
            (
                "zero-stream-port",
                r#"
[server]
stream_port = 0
"#,
            ),
            (
                "buffer-smaller-than-chunk",
                r#"
[server.audio]
buffer_ms = 5
chunk_ms = 10
"#,
            ),
            (
                "unsupported-opus-chunk",
                r#"
[server.audio]
chunk_ms = 15
"#,
            ),
            (
                "non-i16-input-format",
                r#"
[[streams]]
sample_format = { rate = 48000, bits = 24, channels = 2 }
"#,
            ),
        ] {
            let path = write_config(name, content);
            assert!(
                ServerConfig::from_file(&path).is_err(),
                "{name} must be rejected"
            );
            fs::remove_file(path).expect("remove test configuration");
        }
    }

    #[test]
    fn explicit_config_rejects_flac_formats_outside_encoder_and_frame_bounds() {
        for (name, content) in [
            (
                "flac-rate-over-limit",
                "[[streams]]\ncodec = \"flac\"\nsample_format = { rate = 96001, bits = 16, channels = 2 }\n",
            ),
            (
                "flac-channel-over-limit",
                "[[streams]]\ncodec = \"flac\"\nsample_format = { rate = 96000, bits = 16, channels = 9 }\n",
            ),
        ] {
            let path = write_config(name, content);
            assert!(
                ServerConfig::from_file(&path).is_err(),
                "{name} must be rejected before allocating a stream frame"
            );
            fs::remove_file(path).expect("remove test configuration");
        }

        let path = write_config(
            "flac-maximum-supported-frame",
            "[server.audio]\nchunk_ms = 60\n\n[[streams]]\ncodec = \"flac\"\nsample_format = { rate = 96000, bits = 16, channels = 8 }\n",
        );
        ServerConfig::from_file(&path).expect("flacenc's maximum supported frame is valid");
        fs::remove_file(path).expect("remove test configuration");
    }

    #[test]
    fn enabled_auto_buffer_requires_a_minimum_for_every_effective_chunk() {
        let path = write_config(
            "auto-buffer-below-stream-chunk",
            "[server.auto_buffer]\nenabled = true\nmin_ms = 20\n\n[[streams]]\nchunk_ms = 60\n",
        );

        assert!(
            ServerConfig::from_file(&path).is_err(),
            "auto-buffer must not tune below an active stream's frame duration"
        );
        fs::remove_file(path).expect("remove test configuration");
    }

    #[test]
    fn explicit_server_and_client_configs_reject_invalid_log_levels() {
        let server_path = write_config("server-invalid-log", "[log]\nlevel = \"verbose\"\n");
        assert!(ServerConfig::from_file(&server_path).is_err());
        fs::remove_file(server_path).expect("remove test configuration");

        let client_path = write_config("client-invalid-log", "[log]\nlevel = \"verbose\"\n");
        assert!(ClientConfig::from_file(&client_path).is_err());
        fs::remove_file(client_path).expect("remove test configuration");
    }

    #[test]
    fn installer_config_uses_the_audio_table_and_passes_server_validation() {
        let path = write_config("installer", &installer_config());
        let config = ServerConfig::from_file(&path).expect("installer config must load");

        assert_eq!(config.server.audio.buffer_ms, 1000);
        assert_eq!(config.server.audio.chunk_ms, 20);
        assert_eq!(config.server.audio.output_prefill_ms, 0);
        fs::remove_file(path).expect("remove test configuration");
    }

    #[test]
    fn missing_config_uses_defaults_but_an_explicit_invalid_file_does_not() {
        let missing = write_config("missing", "");
        fs::remove_file(&missing).expect("remove placeholder test configuration");
        let defaults =
            ServerConfig::from_file_or_default(&missing).expect("missing config uses defaults");
        assert_eq!(defaults.server.stream_port, 1710);

        let invalid = write_config("explicit-invalid", "[server\nstream_port = 1710");
        assert!(
            ServerConfig::from_file_or_default(&invalid).is_err(),
            "an explicit malformed config must not fall back to defaults"
        );
        fs::remove_file(invalid).expect("remove test configuration");
    }

    #[test]
    fn client_config_rejects_unknown_fields() {
        let path = write_config("client-unknown-field", "unexpected = true");

        let error =
            ClientConfig::from_file(&path).expect_err("unknown client field must be rejected");
        assert!(
            format!("{error:#}").contains("unknown field `unexpected`"),
            "unexpected error: {error:#}"
        );
        fs::remove_file(path).expect("remove test configuration");
    }
}
