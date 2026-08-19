# Configuration Reference

## `sonium.toml` — complete reference

```toml
# Optional IANA timezone used for structured logs and UI timestamps.
# If unset, Sonium uses the host's local timezone.
timezone = "America/Costa_Rica"


# ── Server network ─────────────────────────────────────────────────────────
[server]

# IP address to bind to.  "0.0.0.0" listens on all interfaces.
bind = "0.0.0.0"

# Web UI, REST API, WebSocket, and metrics bind address. Keep loopback unless
# remote administration is explicitly required on a trusted LAN.
control_bind = "127.0.0.1"

# TCP port for the audio stream protocol.
stream_port = 1710

# HTTP/WebSocket port for the control API and embedded web UI.
control_port = 1711

# Global limits for admitted and remembered client identities.
max_clients = 64
max_known_clients = 256

# Advertise the server via mDNS so clients find it automatically.
# Disable if you want manual IP configuration only.
mdns = true

# When true, advertise _snapcast._tcp via mDNS for migration-oriented discovery.
# Matching stream_port = 1704 and control_port = 1780 can help legacy setups,
# but Sonium does not claim full Snapcast compatibility.
snapcast_compat = false

# ── Audio timing ───────────────────────────────────────────────────────────
[server.audio]

# Global jitter buffer suggested to clients unless a stream overrides it.
buffer_ms = 200

# Encoded audio chunk duration. Smaller chunks reduce latency but increase
# packet/task overhead.
chunk_ms = 10

# Output-device prefill in milliseconds. 0 lets the client derive it from
# buffer_ms.
output_prefill_ms = 0


# ── Automatic buffer tuning ────────────────────────────────────────────────
[server.auto_buffer]

# Experimental. When enabled, the server watches client health reports and
# nudges buffers up/down within these limits.
enabled = false
min_ms = 20
max_ms = 3000
step_up_ms = 120
step_down_ms = 40
cooldown_ms = 8000


# ── Media transport ────────────────────────────────────────────────────────
[server.transport]

# "tcp"        — stable default; media and control share the TCP session
# "rtp_udp"    — implemented UDP media plane; pending wider live validation
# "rist"       — experimental Sonium-native ARQ/FEC over UDP, not libRIST wire-compatible
# "quic_dgram" — config-visible placeholder; not implemented yet
mode = "tcp"

# UDP media port for rtp_udp/rist. 0 lets the server auto-bind when a UDP mode
# requires it, or disables UDP when TCP is selected.
udp_port = 0


# ── Audio streams ──────────────────────────────────────────────────────────
# Define one or more audio sources.  Each becomes an independent stream
# that groups can be assigned to.

[[streams]]

# Unique stream identifier (used in group assignments).
id = "default"

# Optional friendly name shown in the web UI.
display_name = "Main room"

# Input source:
#   "-"              — stdin (raw PCM)
#   "/path/to/fifo"  — named FIFO or file (raw PCM)
#   "pipe:///usr/bin/ffmpeg?-i&song.mp3&-f&s16le&-"  — external process
#   "tcp://127.0.0.1:4953"                           — connect to TCP PCM source
#   "tcp-listen://0.0.0.0:4953"                      — listen for TCP PCM senders
#   "tcp://0.0.0.0:4953?mode=server"                 — Snapcast-style listener
source = "-"

# Audio codec.
#   "opus"  — recommended; good quality at low bitrate (~128 kbps stereo)
#   "pcm"   — uncompressed; useful for testing, uses ~1.5 MB/s stereo
#   "flac"  — lossless compression
codec = "opus"

# Optional per-stream jitter buffer override. If omitted, [server.audio].buffer_ms is used.
buffer_ms = 400

# Optional per-stream encoded chunk duration override.
chunk_ms = 10

# Mark the stream idle after no input arrives for this many milliseconds.
idle_timeout_ms = 5000

# Emit silence while idle so connected clients do not immediately underrun.
silence_on_idle = true

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

Common CLI-backed settings can be overridden with environment variables:

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

Environment variables take precedence over `sonium.toml` for the keys listed
above.

## Security and migration notes

An absent configuration file uses defaults; an explicit file is parsed and
validated strictly. Unknown keys, malformed TOML, invalid IP addresses and
ports, or invalid timing/format combinations prevent startup. Fix the file
rather than expecting Sonium to ignore unsupported settings.

Phase 1 supports trusted-LAN deployments only. `server.bind` exposes the audio
listener and media is neither TLS-encrypted nor authenticated. Keep
`control_bind = "127.0.0.1"` unless a host firewall restricts an intentionally
exposed trusted-LAN control port. JWT authentication protects control requests,
not the audio transport.

`users.json` beside the config contains password hashes and a JWT signing
secret. On Unix it is atomically stored with `0600` permissions in a `0700`
directory. Legacy account records without `session_version` load as version
`0` and are persisted on the next successful startup; users must sign in again
to obtain current versioned tokens. Do not overwrite a corrupt or unreadable
existing account file—restore it from backup.
