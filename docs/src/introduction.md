# Sonium

**Sonium** is an open-source multiroom audio server and client written in Rust.
It streams synchronized audio to any number of speakers over your local network —
no cloud, no subscription, no configuration file required to get started.

## Why Sonium?

Most self-hosted multiroom audio solutions were designed years ago and show it:
manual configuration, brittle reconnection, no web interface, and architectures
that don't take advantage of modern async runtimes or hardware clock capabilities.
Sonium is built from scratch for correctness, performance, and ease of use:

| | Typical self-hosted | **Sonium** |
|---|---|---|
| Config to start | Required config files | **Zero** — works out of the box |
| Auto-discovery | No | **mDNS built-in** |
| Web interface | Third-party only | **Bundled** with drag-and-drop |
| Reconnection | Manual restart | **Automatic** with backoff |
| Installation | Build from source | **Single static binary** per platform |
| Language | C / C++ | **Rust** — memory-safe, cross-platform |
| Clock precision | Software only (~1 ms) | **PTP-ready** — pluggable `TimeSource` for nanosecond sync |
| Codecs | Limited | **Opus + FLAC + PCM** out of the box |
| Multi-stream | One global stream | **Per-group streams** with live switching |

## Key features

- **Zero required config** — `sonium-server` runs immediately. Point a browser at `http://server:1711` and you're done.
- **Multi-codec** — Opus for bandwidth efficiency, FLAC for lossless quality, PCM for zero-latency.
- **Tokio async** — handles hundreds of clients on a Raspberry Pi without threads-per-client overhead.
- **Pluggable clock sync** — NTP-like software sync today, PTPv2 hardware timestamping tomorrow.
- **Single binary per role** — `sonium-server` and `sonium-client`, each under 10 MB stripped.
- **Snapcast migration path** — optional compatibility mode lets existing Snapcast clients connect
  to a Sonium server during migration (see [configuration](./getting-started/configuration.md)).

## Current status

Sonium is in active early development. The wire protocol, clock-sync, and codec
crates are feature-complete and tested. The full audio playback path with CPAL
integration is functional.

> **Not production-ready yet.** Sonium is approaching its first stable release.
> Follow the [roadmap](./contributing/roadmap.md) for progress.

## Quick look

```bash
# Start the server — streams PCM from stdin
ffmpeg -f lavfi -i "sine=frequency=440" -f s16le -ar 48000 -ac 2 - | sonium-server

# On any client machine — auto-discover the server
sonium-client --discover
```

See the [Quick Start](./getting-started/quick-start.md) for a full walkthrough.
