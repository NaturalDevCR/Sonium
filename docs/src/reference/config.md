# Configuration Reference

## `sonium.toml` — complete reference

```toml
# ── Server network ─────────────────────────────────────────────────────────
[server]

# IP address to bind to.  "0.0.0.0" listens on all interfaces.
bind = "0.0.0.0"

# TCP port for the audio stream protocol.
stream_port = 1710

# HTTP/WebSocket port for the control API and embedded web UI.
control_port = 1711

# Advertise the server via mDNS so clients find it automatically.
# Disable if you want manual IP configuration only.
mdns = true

# When true, also advertise _snapcast._tcp via mDNS so legacy Snapcast clients
# can discover this server.  For full Snapcast compatibility you must also set
# stream_port = 1704 and control_port = 1780.
snapcast_compat = false


# ── Audio streams ──────────────────────────────────────────────────────────
# Define one or more audio sources.  Each becomes an independent stream
# that groups can be assigned to.

[[streams]]

# Unique stream identifier (used in group assignments).
id = "default"

# Input source:
#   "-"              — stdin (raw PCM)
#   "/path/to/fifo"  — named FIFO or file (raw PCM)
#   "pipe:///usr/bin/ffmpeg?-i&song.mp3&-f&s16le&-"  — external process
source = "-"

# Audio codec.
#   "opus"  — recommended; good quality at low bitrate (~128 kbps stereo)
#   "pcm"   — uncompressed; useful for testing, uses ~1.5 MB/s stereo
#   "flac"  — lossless compression
codec = "opus"

# Jitter buffer size in milliseconds.
# Higher = more tolerance for network jitter, more end-to-end latency.
# Lower  = tighter sync, may cause dropouts on congested networks.
buffer_ms = 1000

# Sample format for this stream's input.
[streams.sample_format]
rate     = 48000   # Samples per second
bits     = 16      # Bit depth per sample per channel
channels = 2       # 1 = mono, 2 = stereo, 6 = 5.1


# ── Logging ────────────────────────────────────────────────────────────────
[log]
# Log level: "trace" | "debug" | "info" | "warn" | "error"
level = "info"
```

## `sonium-client.toml` — complete reference

```toml
# Hostname or IP address of the Sonium server.
server_host = "192.168.1.100"

# Audio stream port (must match server's stream_port).
server_port = 1710

# Extra latency offset in milliseconds.
#   Positive: play later (compensate for Bluetooth / HDMI delay)
#   Negative: play earlier (unusual)
latency_ms = 0

# Optional display name shown in the web UI. Falls back to system hostname.
# client_name = "Living Room"

# Audio output device (substring match, case-insensitive).
# Leave unset to use the system default.
# device = "USB Audio"

[log]
level = "info"
```

## Environment variable overrides

Every config key maps to an environment variable with the prefix `SONIUM_`:

| Config key | Environment variable |
|---|---|
| `server.stream_port` | `SONIUM_STREAM_PORT` |
| `server.control_port` | `SONIUM_CONTROL_PORT` |
| `server.bind` | `SONIUM_BIND` |
| `log.level` | `SONIUM_LOG` |

Client:

| Config key | Environment variable |
|---|---|
| `server_host` | `SONIUM_SERVER` |
| `server_port` | `SONIUM_PORT` |
| `latency_ms` | `SONIUM_LATENCY` |
| `log.level` | `SONIUM_LOG` |

Environment variables take precedence over `sonium.toml`.
