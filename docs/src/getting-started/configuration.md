# Configuration

Sonium works **without any configuration file**. All defaults are chosen to be
immediately useful on a typical home network.

If you need to customise behaviour, create a `sonium.toml` file in the working
directory where you run `sonium-server` (typically `/etc/sonium/sonium.toml`
when installed via the Linux installer).

## Server — `sonium.toml`

```toml
[server]
bind            = "0.0.0.0"   # Listen on all interfaces
stream_port     = 1710        # Audio stream port (Sonium default)
control_port    = 1711        # HTTP/WS control API and web UI
mdns            = true        # Advertise via mDNS for zero-config discovery
snapcast_compat = false       # Set true to also advertise _snapcast._tcp mDNS

[server.audio]
buffer_ms         = 200       # Global jitter buffer default (was 1000 pre-v0.1.78)
chunk_ms          = 10        # Global encoded chunk default
output_prefill_ms = 0         # Local audio-device prefill; 0 = automatic

[server.auto_buffer]
enabled       = false         # Enable dynamic buffer tuning from health telemetry
min_ms        = 20            # Lower clamp for auto mode
max_ms        = 3000          # Upper clamp for auto mode
step_up_ms    = 120           # Increase step when underruns/jitter spikes appear
step_down_ms  = 40            # Decrease step when playback remains stable
cooldown_ms   = 8000          # Minimum delay between auto adjustments

[server.transport]
mode     = "tcp"              # "tcp" | "rtp_udp" | "quic_dgram"
udp_port = 0                  # Server UDP port for RTP (0 = same as stream_port)

[[streams]]
id        = "default"
source    = "-"          # "-" = stdin; or a file/FIFO path
codec     = "opus"       # "opus" | "pcm" | "flac"
# Optional per-stream overrides:
# buffer_ms = 200
# chunk_ms  = 10
idle_timeout_ms = 3000   # Optional: mark stream idle after no input data
silence_on_idle = true   # Optional: emit silence while idle

# Add more streams for multi-room setups:
# [[streams]]
# id     = "kitchen"
# source = "/tmp/kitchen.fifo"
# codec  = "flac"

# Timezone for log timestamps and web UI display
timezone = "America/Costa_Rica"

[log]
level = "info"  # "trace" | "debug" | "info" | "warn" | "error"
```

### Audio Timing

`buffer_ms` is the client-side playout buffer target. Larger values tolerate more
network jitter and scheduling delays, but increase end-to-end latency.

Since v0.1.78, the default `buffer_ms` was reduced from `1000` to `200` because
TCP streaming stability improvements eliminated the need for large buffers on
most networks.

| Environment | `buffer_ms` | Notes |
|-------------|-------------|-------|
| Wired LAN   | 0–50        | Zero-config if all devices use wired Ethernet |
| Wi-Fi LAN   | 100–200     | Default; handles most Wi-Fi jitter |
| Mesh/PLC    | 200–400     | Powerline or mesh Wi-Fi with higher latency |
| Internet    | 500–1000    | Only for WAN streaming (not recommended) |

`output_prefill_ms` is separate from `buffer_ms`. `buffer_ms` absorbs network
jitter; `output_prefill_ms` keeps the client's local audio-device ring fed. Use
`0` for the automatic value derived from `buffer_ms`.

`chunk_ms` controls the duration of each encoded audio chunk. For Opus, Sonium
uses safe frame durations:

| `chunk_ms` | Use when |
|------------|----------|
| `10`       | Low latency on reliable LAN |
| `20`       | Balanced (was default pre-v0.1.78) |
| `40`       | Lower overhead, forgiving scheduling |
| `60`       | Maximum Opus frame, lowest packet rate |

### Auto-Buffer Tuning

When `enabled`, the server monitors each client's health reports and adjusts
`buffer_ms` automatically:

```toml
[server.auto_buffer]
enabled       = true
min_ms        = 20
max_ms        = 1000
step_up_ms    = 120
step_down_ms  = 40
cooldown_ms   = 8000
```

- **Steps up** on underruns or high jitter
- **Steps down** during sustained stability
- Respects `min_ms`/`max_ms` bounds

### External Process and Radio Streams

`pipe://` starts an external process and reads raw PCM from its stdout. This is
the recommended way to use ffmpeg for files, playlists, internet radio, and
AzuraCast/Icecast-style MP3 streams:

```toml
[[streams]]
id = "radio"
display_name = "Radio"
source = "pipe:///usr/bin/ffmpeg?-reconnect&1&-reconnect_streamed&1&-i&https://example.com/radio.mp3&-f&s16le&-ar&48000&-ac&2&-"
codec = "opus"
buffer_ms = 200
chunk_ms = 40
idle_timeout_ms = 3000
silence_on_idle = true
```

If the process output closes, Sonium marks the stream idle and restarts the
external source with backoff.

### Timezone

Set the timezone for log timestamps and web UI display:

```toml
timezone = "Europe/Berlin"
```

If not set, the system default timezone is used. This affects:
- Log file timestamps
- Web UI "connected at" times
- Journalctl log display

## CLI Flags

Command-line flags override `sonium.toml` values. Environment variables
(prefixed `SONIUM_`) override both:

```bash
sonium-server \
  --bind 0.0.0.0 \
  --stream-port 1710 \
  --control-port 1711 \
  --log debug
```

## Client — `sonium-client.toml`

```toml
server_host = "192.168.1.100"  # Server IP or hostname
server_port = 1710
latency_ms  = 0                # Extra latency offset (positive for Bluetooth)

# Timezone for client-side log timestamps
timezone = "America/Costa_Rica"

[log]
level = "info"
```

### Same-Machine Server

If the client runs on the same machine as the server, Sonium detects this
automatically for `localhost`/`127.0.0.1` connections. You can also force it:

```bash
sonium-client --on-server 192.168.1.100
```

When `--on-server` is active, network time sync is skipped (offset = 0) because
both processes share the same system clock.

### Bluetooth Latency Compensation

Bluetooth speakers typically add 100–250 ms of latency. Use `latency_ms` to
compensate so all speakers stay in sync:

```toml
latency_ms = 150  # Adjust to match your Bluetooth device
```

## Snapcast Migration (Drop-in Replacement)

To use Sonium as a drop-in replacement for an existing Snapcast setup:

```toml
[server]
stream_port     = 1704   # Snapcast's default audio port
control_port    = 1780   # Snapcast's default HTTP port
snapcast_compat = true   # Advertise _snapcast._tcp mDNS service
```

> **Note:** Sonium's native defaults are `1710`/`1711`. Changing them to
> Snapcast's ports is only needed for legacy client compatibility.

## Environment Variables

All config values can be set via environment variables (useful for Docker /
`systemd` `Environment=` directives):

```bash
SONIUM_STREAM_PORT=1710 \
SONIUM_CONTROL_PORT=1711 \
SONIUM_LOG=debug \
  sonium-server
```

## Logs and Restart Behavior

When installed under systemd, the admin UI can read recent service logs and
filter by time window. Logs are formatted in the configured timezone and avoid
ANSI color escapes.

The admin UI can also request a server restart after config changes. On Linux,
this requires the installer-created sudoers rule:

```bash
systemctl restart sonium-server.service
```

If you created the service manually, restart requests may fail with
`Access denied`. Re-run the installer or add an equivalent permission.
