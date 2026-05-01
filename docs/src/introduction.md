# Sonium

**Sonium** is an open-source multiroom audio server and client written in Rust.
It streams synchronized audio to any number of speakers over your local network —
no cloud, no subscription, no configuration file required to get started.

> **Early project warning:** Sonium is not production-ready. It still has known
> audio stability, low-latency buffering, upgrade, and diagnostics gaps. Use it
> for experimentation and local testing, not for unattended or commercial audio.

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
| Installation | Build from source | **Release packages + Linux installer + Desktop Agent** |
| Language | C / C++ | **Rust** — memory-safe, cross-platform |
| Clock precision | Software only (~1 ms) | **PTP-ready** — pluggable `TimeSource` for nanosecond sync |
| Codecs | Limited | **Opus + FLAC + PCM** out of the box |
| Multi-stream | One global stream | **Per-group streams** with live switching |

## Key features

- **Zero required config** — `sonium-server` runs immediately. Point a browser at `http://server:1711` and you're done.
- **Multi-codec** — Opus for bandwidth efficiency, FLAC for lossless quality, PCM for zero-latency.
- **Multiple source types** — FIFO/file, TCP, external `pipe://` processes, ffmpeg radio templates, and meta streams.
- **Recovering radio/process streams** — external sources restart with backoff if their stdout closes.
- **Tokio async** — handles hundreds of clients on a Raspberry Pi without threads-per-client overhead.
- **Pluggable clock sync** — NTP-like software sync today, PTPv2 hardware timestamping tomorrow.
- **Admin UI** — users, roles, groups, streams, config editing, dependency checks, logs, and supervised restart requests.
- **Desktop Agent** — tray app for macOS and Windows client instances.
- **Snapcast migration path** — optional compatibility mode lets existing Snapcast clients connect
  to a Sonium server during migration (see [configuration](./getting-started/configuration.md)).

## Current status

Sonium is in active early development. The protocol, codec, control API, web UI,
installer, and CPAL playback path are functional, but the project is still being
hardened around real-world jitter, low-latency operation, device behavior, and
operator workflows.

> **Not production-ready yet.** Expect dropouts, bugs, and occasional breaking
> changes. Follow the [roadmap](./contributing/roadmap.md) for progress.

## Quick look

```bash
# Start the server — streams PCM from stdin
ffmpeg -f lavfi -i "sine=frequency=440" -f s16le -ar 48000 -ac 2 - | sonium-server

# On any client machine — auto-discover the server
sonium-client --discover
```

See the [Quick Start](./getting-started/quick-start.md) for a full walkthrough.
