# Quick Start

Get Sonium running in under 5 minutes.

## Prerequisites

- Rust toolchain ≥ 1.75 (`rustup update stable`)
- A source of audio (any application that outputs to stdout as raw PCM, or
  `ffmpeg`)

## Build

```bash
git clone https://github.com/jdavidoa91/sonium
cd sonium
cargo build --release
```

Binaries land in `target/release/`:

```
target/release/sonium-server
target/release/sonium-client
```

## Start the server

Sonium server reads raw interleaved **signed 16-bit little-endian PCM** from
stdin and streams it to all connected clients.

```bash
# Stream a 440 Hz test tone
ffmpeg -f lavfi -i "sine=frequency=440:duration=3600" \
       -f s16le -ar 48000 -ac 2 - \
  | ./target/release/sonium-server
```

Output:

```
2024-01-15T12:00:00Z  INFO sonium_server: Sonium server starting stream_port=1710 control_port=1711 codec=opus format=48000Hz/16bit/2ch
2024-01-15T12:00:00Z  INFO sonium_server: Listening on 0.0.0.0:1710
2024-01-15T12:00:00Z  INFO sonium_server::streamreader: Stream reader started — reading PCM from stdin
```

## Connect a client

On the same machine or any machine on your network:

```bash
./target/release/sonium-client
```

If the server is on a different host:

```bash
./target/release/sonium-client --server 192.168.1.100
```

## Use with Spotify / MPD / any source

Sonium works with any source that can write PCM to a FIFO or pipe.  Create a
named pipe and point your audio daemon at it:

```bash
# Create a FIFO
mkfifo /tmp/sonium.pcm

# Start Sonium reading from the FIFO
sonium-server --pipe /tmp/sonium.pcm

# Configure MPD to write to the FIFO (mpd.conf):
# audio_output {
#     type  "fifo"
#     name  "Sonium"
#     path  "/tmp/sonium.pcm"
#     format "48000:16:2"
# }
```

## Open the web interface

Once the server is running, navigate to:

```
http://<server-ip>:1711
```

> The web interface is planned for **Fase 7**.  In the current build this
> endpoint is not yet available.

## Next steps

- [Configuration](./configuration.md) — tune the server without config files
- [Protocol reference](../reference/protocol.md) — understand the wire format
