# Sonium

Open-source multiroom audio server and client written in Rust.
Streams perfectly synchronized audio to any number of speakers over your local
network — **no cloud, no configuration file required, no subscription**.

[![CI](https://github.com/jdavidoa91/sonium/actions/workflows/ci.yml/badge.svg)](https://github.com/jdavidoa91/sonium/actions/workflows/ci.yml)
[![Docs](https://github.com/jdavidoa91/sonium/actions/workflows/docs.yml/badge.svg)](https://jdavidoa91.github.io/sonium/)
[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)

## Quick start

```bash
# Build
cargo build --release

# Start server — streams a 440 Hz test tone
ffmpeg -f lavfi -i "sine=frequency=440" -f s16le -ar 48000 -ac 2 - \
  | ./target/release/sonium-server

# Connect a client (same machine or any host on the LAN)
./target/release/sonium-client
```

## Documentation

Full documentation: **[jdavidoa91.github.io/sonium](https://jdavidoa91.github.io/sonium/)**

- [Quick Start](https://jdavidoa91.github.io/sonium/getting-started/quick-start.html)
- [Architecture](https://jdavidoa91.github.io/sonium/architecture/overview.html)
- [Binary Protocol](https://jdavidoa91.github.io/sonium/reference/protocol.html)
- [Roadmap](https://jdavidoa91.github.io/sonium/contributing/roadmap.html)

## Why Sonium?

[Snapcast](https://github.com/badaix/snapcast) is the standard for self-hosted
multiroom audio.  Sonium is a Rust rewrite that keeps full wire-protocol
compatibility while adding:

- **Zero required config** — works out of the box
- **Built-in web UI** (Fase 7)
- **mDNS auto-discovery** (Fase 8)
- **Automatic reconnection**
- **PTPv2 hardware clock support** (Fase 10) — nanosecond-accurate sync

## Status

Early development — **Fase 0 complete**.  Not production-ready yet.
See the [roadmap](https://jdavidoa91.github.io/sonium/contributing/roadmap.html).

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE).
