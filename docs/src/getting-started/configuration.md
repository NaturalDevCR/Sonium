# Configuration

Sonium is designed to work **without any configuration file**.  All defaults
are chosen to be immediately useful on a typical home network.

If you do need to customise behaviour, create a `sonium.toml` file in the
working directory where you run `sonium-server`.

## Server — `sonium.toml`

```toml
[server]
bind         = "0.0.0.0"   # Listen on all interfaces
stream_port  = 1704        # Audio stream port (Snapcast-compatible)
control_port = 1780        # HTTP/WS control API and web UI
mdns         = true        # Advertise via mDNS for zero-config discovery

[stream]
codec         = "opus"     # "opus" | "pcm" | "flac"  (flac: Fase 5+)
buffer_ms     = 1000       # Jitter buffer size in milliseconds
# pipe = "/tmp/sonium.pcm" # Read from a named FIFO instead of stdin

[stream.sample_format]
rate     = 48000
bits     = 16
channels = 2

[log]
level = "info"  # "trace" | "debug" | "info" | "warn" | "error"
```

## CLI flags (planned)

Command-line flags will override `sonium.toml` values:

```bash
sonium-server \
  --bind 0.0.0.0 \
  --stream-port 1704 \
  --codec opus \
  --buffer-ms 750 \
  --pipe /tmp/sonium.pcm \
  --log-level debug
```

## Client — `sonium-client.toml`

```toml
server_host = "192.168.1.100"  # Server IP or hostname
server_port = 1704
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

## Environment variables

All config values can be set via environment variables (useful for Docker /
`systemd` `Environment=` directives):

```bash
SONIUM_STREAM_PORT=1705 \
SONIUM_CODEC=pcm \
SONIUM_LOG_LEVEL=debug \
  sonium-server
```

> Environment variable support is planned for **Fase 5**.
