# Sonium

Open-source multiroom audio server and client written in Rust.
Streams perfectly synchronized audio to any number of speakers over your local
network — **no cloud, no configuration file required, no subscription**.

[![CI](https://github.com/jdavidoa91/sonium/actions/workflows/ci.yml/badge.svg)](https://github.com/jdavidoa91/sonium/actions/workflows/ci.yml)
[![Docs](https://github.com/jdavidoa91/sonium/actions/workflows/docs.yml/badge.svg)](https://jdavidoa91.github.io/sonium/)
[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)

## Quick start

For local development, use the one-command runner:

```bash
./dev.sh
```

This builds the embedded web UI and starts `sonium-server` with
`run/sonium.toml`. Open the web UI at <http://127.0.0.1:1711>.

To also start a local client:

```bash
./dev.sh --with-client
```

If your default audio device does not accept Sonium's 48 kHz stereo output,
select one explicitly:

```bash
./dev.sh --with-client --client-device "BlackHole"
```

```bash
# Build
cargo build --release

# Start server — streams a 440 Hz test tone
ffmpeg -f lavfi -i "sine=frequency=440" -f s16le -ar 48000 -ac 2 - \
  | ./target/release/sonium-server

# Connect a client (same machine or any host on the LAN)
./target/release/sonium-client --discover
```

## Documentation

Full documentation: **[jdavidoa91.github.io/sonium](https://jdavidoa91.github.io/sonium/)**

- [Quick Start](https://jdavidoa91.github.io/sonium/getting-started/quick-start.html)
- [Architecture](https://jdavidoa91.github.io/sonium/architecture/overview.html)
- [Binary Protocol](https://jdavidoa91.github.io/sonium/reference/protocol.html)
- [Roadmap](https://jdavidoa91.github.io/sonium/contributing/roadmap.html)

## Why Sonium?

Sonium is a **next-generation** multiroom audio system built from scratch in Rust.
It's designed for the modern home network: zero config, instant setup, and
professional-grade synchronization — all without vendor lock-in.

- **Zero required config** — works out of the box
- **Built-in web UI** with drag-and-drop group management
- **mDNS auto-discovery** — clients find the server automatically
- **Automatic reconnection** with exponential backoff
- **PTPv2 hardware clock support** (planned) — nanosecond-accurate sync on commodity hardware
- **Multi-codec** — Opus, FLAC, and PCM out of the box
- **Interoperable** — optional backward compatibility with Snapcast v2 clients for easy migration

## Status

Early development — core audio pipeline complete. Not production-ready yet.
See the [roadmap](https://jdavidoa91.github.io/sonium/contributing/roadmap.html).

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE).
