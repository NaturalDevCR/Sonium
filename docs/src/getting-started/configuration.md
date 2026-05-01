# Configuration

Sonium is designed to work **without any configuration file**.  All defaults
are chosen to be immediately useful on a typical home network.

If you do need to customise behaviour, create a `sonium.toml` file in the
working directory where you run `sonium-server`.

## Server — `sonium.toml`

```toml
[server]
bind            = "0.0.0.0"   # Listen on all interfaces
stream_port     = 1710        # Audio stream port (Sonium default)
control_port    = 1711        # HTTP/WS control API and web UI
mdns            = true        # Advertise via mDNS for zero-config discovery
snapcast_compat = false       # Set true to also advertise _snapcast._tcp mDNS
buffer_ms       = 1000        # Global jitter buffer default
chunk_ms        = 20          # Global encoded chunk default
auto_buffer             = false # Enable dynamic buffer tuning from health telemetry
auto_buffer_min_ms      = 400   # Lower clamp for auto mode
auto_buffer_max_ms      = 3000  # Upper clamp for auto mode
auto_buffer_step_up_ms  = 120   # Increase step when underruns/jitter spikes appear
auto_buffer_step_down_ms = 40   # Decrease step when playback remains stable
auto_buffer_cooldown_ms = 8000  # Minimum delay between auto adjustments

[[streams]]
id        = "default"
source    = "-"          # "-" = stdin; or a file/FIFO path
codec     = "opus"       # "opus" | "pcm" | "flac"
# Optional per-stream overrides:
# buffer_ms = 1000
# chunk_ms  = 20
idle_timeout_ms = 3000   # Optional: mark stream idle after no input data
silence_on_idle = true   # Optional: emit silence while idle

# Add more streams for multi-room setups:
# [[streams]]
# id     = "kitchen"
# source = "/tmp/kitchen.fifo"
# codec  = "flac"

[log]
level = "info"  # "trace" | "debug" | "info" | "warn" | "error"
```

### Stream buffering and chunk size

`buffer_ms` is the client-side playout buffer target. Larger values tolerate
more network jitter and scheduling delays, but increase end-to-end latency.
Configure it globally under `[server]`; add `buffer_ms` inside a `[[streams]]`
entry only when that stream needs a different value. `1000` ms is currently the
safest default. Lower values may work on clean LANs, but Sonium is still being
tuned and may stutter below that on some systems.

`chunk_ms` controls the duration of each encoded audio chunk. Smaller chunks can
reduce scheduling latency and smooth delivery, but increase packet and CPU
overhead. Configure it globally under `[server]`; add `chunk_ms` inside a
`[[streams]]` entry for stream-specific overrides. For Opus, Sonium clamps values
to safe Opus frame durations:

| `chunk_ms` | Use when |
| --- | --- |
| `10` | Testing lower latency on a reliable LAN |
| `20` | Default balance |
| `40` | Lower overhead, more forgiving scheduling |
| `60` | Maximum Opus frame duration, lowest packet rate |

Future releases may add an automatic mode that recommends or adjusts these
values from real jitter/underrun telemetry. Manual control will remain available.

Automatic mode can now be enabled with `auto_buffer = true`. When enabled, the
server adjusts each client session's effective `buffer_ms` over time based on
reported underruns, stale drops, and jitter. It starts from the stream/global
`buffer_ms`, then adjusts within `auto_buffer_min_ms` and
`auto_buffer_max_ms` using the configured step sizes and cooldown.

### External process and radio streams

`pipe://` starts an external process and reads raw PCM from its stdout. This is
the recommended way to use ffmpeg for files, playlists, internet radio, and
AzuraCast/Icecast-style MP3 streams:

```toml
[[streams]]
id = "radio"
display_name = "Radio"
source = "pipe:///usr/bin/ffmpeg?-reconnect&1&-reconnect_streamed&1&-i&https://example.com/radio.mp3&-f&s16le&-ar&48000&-ac&2&-"
codec = "opus"
buffer_ms = 1200 # Optional override; otherwise [server].buffer_ms is used
chunk_ms = 40    # Optional override; otherwise [server].chunk_ms is used
idle_timeout_ms = 3000
silence_on_idle = true
```

If the process output closes, Sonium marks the stream idle and restarts the
external source with backoff.

## CLI flags

Command-line flags override `sonium.toml` values.  Environment variables
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

[log]
level = "info"
```

### Bluetooth latency compensation

Bluetooth speakers typically add 100–250 ms of latency.  Use `latency_ms` to
compensate so all speakers stay in sync:

```toml
latency_ms = 150  # Adjust to match your Bluetooth device
```

## Snapcast migration (drop-in replacement)

To use Sonium as a drop-in replacement for an existing Snapcast setup — keeping
legacy Snapcast clients working while you migrate:

```toml
[server]
stream_port     = 1704   # Snapcast's default audio port
control_port    = 1780   # Snapcast's default HTTP port
snapcast_compat = true   # Advertise _snapcast._tcp mDNS service
```

> **Note:** Sonium's native defaults are `1710`/`1711`.  Changing them to
> Snapcast's ports is only needed for legacy client compatibility.

## Environment variables

All config values can be set via environment variables (useful for Docker /
`systemd` `Environment=` directives):

```bash
SONIUM_STREAM_PORT=1710 \
SONIUM_CONTROL_PORT=1711 \
SONIUM_LOG=debug \
  sonium-server
```

## Logs and restart behavior

When installed under systemd, the admin UI can read recent service logs and
filter by time window. Logs are formatted in the server's local timezone and
avoid ANSI color escapes so they are easier to read in `journalctl` and the UI.

The admin UI can also request a server restart after config changes. On Linux,
this requires the installer-created sudoers rule that allows the `sonium` service
user to run only:

```bash
systemctl restart sonium-server.service
```

If you created the service manually, restart requests may fail with
`Access denied`. Re-run the installer or add an equivalent, narrowly scoped
permission yourself.
