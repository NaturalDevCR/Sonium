# Configuration

Sonium can use defaults when no configuration file exists, but a server still
needs an initial administrator (`--init-admin`) before it can start. Once an
explicit `sonium.toml` exists, it is strict: malformed TOML, unknown keys,
invalid addresses, unsafe port combinations, and invalid audio timing values
stop startup instead of falling back to defaults.

`--init-admin` is a flag with no password argument. Provide the password on
standard input, for example `printf '%s' "$SONIUM_INIT_ADMIN_PASSWORD" |
sonium-server --init-admin`. Releases that accepted
`--init-admin PASSWORD` no longer do so, because command-line arguments can be
read from process inspection and shell history. Update automation to feed stdin
or an inherited protected file descriptor; do not restore the old argument form.

If you need to customise behaviour, create a `sonium.toml` file in the working
directory where you run `sonium-server` (typically `/etc/sonium/sonium.toml`
when installed via the Linux installer).

## Server — `sonium.toml`

```toml
# Timezone for log timestamps and web UI display. It is a root key and must
# appear before the first TOML table.
timezone = "America/Costa_Rica"

[server]
bind            = "0.0.0.0"   # Listen on all interfaces
control_bind    = "127.0.0.1" # Web UI/API/metrics; keep local by default
stream_port     = 1710        # Audio stream port (Sonium default)
control_port    = 1711        # HTTP/WS control API and web UI
max_clients     = 64          # 1–256 active sessions, including handshakes
max_known_clients = 256       # Remembered clients; max_clients–1024
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
mode     = "tcp"              # "tcp" | "rtp_udp" | "rist" | "quic_dgram"
udp_port = 0                  # Must be 0 for tcp/quic_dgram; rtp_udp/rist use 0 = stream_port + 2

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

[log]
level = "info"  # "trace" | "debug" | "info" | "warn" | "error"
```

### Trusted-LAN boundary and control bind

This configuration is for a trusted LAN. `server.bind` is the audio listener;
Sonium does not encrypt or authenticate that media path. Never expose it to the
Internet or through an untrusted routed network.

The web UI, REST API, WebSocket endpoint, and metrics endpoint use
`server.control_bind`, not `server.bind`. The default is `127.0.0.1`, so a
fresh install is administered locally. To administer from a trusted LAN host,
set `control_bind` to a specific private address (preferred) or `0.0.0.0`, then
apply a host firewall rule that permits only the intended administrator hosts.
This is an explicit exposure decision; JWT login does not turn the deployment
into a TLS or authenticated-media service.

### Account storage and migration

The account database is `users.json` next to `sonium.toml`. On Unix Sonium
keeps the directory at `0700` and writes the account file atomically at `0600`.
It includes password hashes and the persistent JWT signing secret, so keep it
on durable local storage and never copy it into TOML, Compose files, URLs,
logs, or source control.

Existing account files that predate `session_version` remain readable: missing
values are treated as `0` and are written back on the next successful startup.
Tokens issued before session versions were added do not carry that claim and
must be replaced by signing in again. Afterwards, a password, role, or account
deletion change invalidates that user's earlier tokens, including after a
restart. A present-but-corrupt, unreadable, or semantically invalid `users.json`
is fatal and is not replaced; restore it from a known-good backup before
retrying.

### Initial-admin bootstrap

Run the one-time bootstrap locally before the first ordinary server start. The
server reads its only input from standard input only when `--init-admin` is
present, then exits; normal server startup never reads stdin for a password.
The Docker, source, Windows, and Linux-installer procedures all use this same
stdin contract. Keep any temporary secret in memory only, clear it immediately
after success or failure, and never place it in a command argument, TOML file,
Compose file, shell history, log, or source control.

### Legacy installer configuration

Phase 1 accepts timing keys only in `[server.audio]`. The Linux installer
preflights an existing configuration with Python 3.11+ `tomllib` before it
changes binaries or systemd. If it finds legacy `buffer_ms`, `chunk_ms`, or
`output_prefill_ms` keys directly under `[server]`, or cannot parse the
existing TOML, it aborts without stopping the current service. Move those
values to `[server.audio]`, fix any TOML syntax error, and rerun the installer.
If the host lacks Python 3.11+ with `tomllib`, install or enable it first; this
deliberate stop avoids restarting a healthy service with a strict-config
startup failure.

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
| Internet    | N/A         | Unsupported in Phase 1; do not expose media to a WAN |

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

If the process output closes, Sonium restarts the external source with backoff.
For ordinary file and FIFO paths, Sonium reports `recovering` with retry context
and reopens after producer disconnects, path recreation, or file replacement.
The Recovering state is only for ordinary file/FIFO sources after a recoverable
open/read/EOF condition. `error` is reserved for terminal source failures such
as an empty path, permission denial, a directory, unsupported path type, or a
symlink loop; it does not mean a `pipe://` child simply closed.

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

## Snapcast discovery migration

To advertise a Sonium server to legacy Snapcast discovery tooling:

```toml
[server]
stream_port     = 1704   # Snapcast's default audio port
control_port    = 1780   # Snapcast's default HTTP port
snapcast_compat = true   # Advertise _snapcast._tcp mDNS service
```

> **Note:** Sonium's native defaults are `1710`/`1711`. Changing ports and
> enabling the mDNS advertisement can assist discovery, but does **not** claim
> drop-in protocol compatibility with all Snapcast clients or versions.

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
