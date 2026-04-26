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

[[streams]]
id        = "default"
source    = "-"          # "-" = stdin; or a file/FIFO path
codec     = "opus"       # "opus" | "pcm" | "flac"
buffer_ms = 1000         # Jitter buffer in milliseconds

# Add more streams for multi-room setups:
# [[streams]]
# id     = "kitchen"
# source = "/tmp/kitchen.fifo"
# codec  = "flac"

[log]
level = "info"  # "trace" | "debug" | "info" | "warn" | "error"
```

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
