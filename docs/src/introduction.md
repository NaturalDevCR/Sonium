# Sonium

**Sonium** is an open-source multiroom audio server and client written in Rust.
It streams synchronized audio to any number of speakers over your local network —
no cloud, no subscription, no configuration file required to get started.

## Why Sonium?

[Snapcast](https://github.com/badaix/snapcast) is the standard for self-hosted
multiroom audio.  Sonium is a spiritual successor that preserves full
**wire-protocol compatibility** with Snapcast v2 while fixing the rough edges:

| | Snapcast | **Sonium** |
|---|---|---|
| Config to start | Required (`snapserver.conf`) | **Zero** — works out of the box |
| Auto-discovery | No | **mDNS built-in** |
| Web interface | Third-party only | **Bundled** (Fase 7) |
| Reconnection | Manual restart | **Automatic** with backoff |
| Installation | Build from source / apt | **Single static binary** (target) |
| Language | C++ | **Rust** — memory-safe, cross-platform |
| PTP clock support | Planned (open issue) | **Designed for it** (pluggable `TimeSource`) |

## Key features

- **Snapcast-compatible** — connect existing Snapcast clients to Sonium server and vice versa during migration.
- **Zero required config** — `sonium-server` runs immediately. Point a browser at `http://server:1780` and you're done.
- **Tokio async** — handles hundreds of clients on a Raspberry Pi without threads-per-client overhead.
- **Pluggable clock sync** — NTP-like software sync today, PTPv2 hardware timestamping tomorrow.
- **Single binary per role** — `sonium-server` and `sonium-client`, each under 10 MB stripped.

## Current status

Sonium is in active early development.  The wire protocol and clock-sync crates
are feature-complete and tested.  The audio playback path (CPAL integration) is
targeted for **Fase 4** of the roadmap.

> **Not production-ready yet.** Use Snapcast for production deployments until
> Sonium reaches its Fase 4 milestone.

## Quick look

```bash
# Start the server — streams PCM from stdin
ffmpeg -f lavfi -i "sine=frequency=440" -f s16le -ar 48000 -ac 2 - | sonium-server

# On any client machine
sonium-client --server 192.168.1.100
```

See the [Quick Start](./getting-started/quick-start.md) for a full walkthrough.
