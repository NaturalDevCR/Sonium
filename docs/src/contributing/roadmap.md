# Roadmap

Sonium is developed in phases, each delivering independent value.

## Completed

### Fase 0 — Foundation ✅
- Cargo workspace with all crates
- Protocol crate: full Snapcast v2 wire format, little-endian, all message types
- Codec crate: Opus encoder/decoder + PCM passthrough
- Sync crate: NTP-like clock offset estimation with 200-sample median filter + jitter buffer
- Server skeleton: TCP listener, broadcaster, session handler, stream reader
- Client skeleton: auto-reconnect, Hello, codec detection, clock sync, player stub
- Unit tests: protocol round-trips, sync convergence, codec round-trips
- Documentation (this site)

---

## In progress

### Fase 1 — Protocol validation 🔄
- Parse a real Snapcast traffic dump and verify byte-identical reconstruction
- Test interop: Sonium server ↔ Snapclient original
- Fuzz testing with `cargo-fuzz` on the message parser

---

## Planned

### Fase 2 — Clock sync hardening
- Two-process loopback test measuring actual desync with Audacity
- Raspberry Pi LAN test: target **< 1 ms** desync
- Thread-priority tuning for the audio loop (outside Tokio executor)
- `TimeSource` trait groundwork for future PTPv2 support
- **Goal:** < 50 ms desync. Stretch: < 1 ms.

### Fase 3 — Server MVP
- FLAC encoder integration (`flac-bound`)
- Named pipe (`--pipe`) support
- Multiple streams (one pipe per stream)
- Graceful shutdown + cleanup

### Fase 4 — Client MVP with real audio output
- CPAL integration (Linux ALSA/PipeWire, macOS CoreAudio)
- USB DAC hotplug handling (known Snapcast weakness)
- Audio underrun recovery without clicks
- **Milestone:** Sonium client + Snapcast server = synchronized audio ✅

### Fase 5 — Hardening
- 24-hour stability test on Raspberry Pi
- Reconnection after server restart
- Structured logging with `tracing`
- CLI flags for all config values
- Environment variable overrides

### Fase 6 — Multiple streams and groups
- Client groups — each group plays a different stream
- Per-client volume/mute via server-side mixing
- Stream sources: pipe, file, external process

### Fase 7 — Web UI (major differentiator)
- Embedded SvelteKit SPA served from `sonium-server`
- Real-time WebSocket events
- Drag-and-drop group assignment
- Per-client volume sliders
- **Goal:** full setup in < 5 minutes without touching config files

### Fase 8 — Zero-config discovery
- mDNS/Bonjour advertisement (`_snapcast._tcp`, `_snapcast-http._tcp`)
- Client auto-discovers server on first launch
- Multiple-server selection

### Fase 9 — Cross-platform
- Windows: WASAPI audio, MSI installer
- Android: evaluation of `oboe-rs` vs thin Kotlin wrapper
- macOS: CoreAudio, notarized `.app` bundle

### Fase 10 — Advanced features

| Feature | Notes |
|---------|-------|
| **PTPv2 hardware clock** | IEEE 1588v2 via `/dev/ptp0` — nanosecond sync; [requested for Snapcast](https://github.com/badaix/snapcast/issues/1478) |
| Cross-subnet relay | Stream to remote networks via a relay node |
| TLS + token auth | Secure control API for internet exposure |
| Plugin sources | Spotify Connect, AirPlay, Tidal |
| Volume normalization | Per-track EBU R128 / ReplayGain |
| Per-room EQ | FIR filters via `fundsp` |
| Mobile control app | Flutter or React Native |

---

## PTPv2 detail

[Snapcast issue #1478](https://github.com/badaix/snapcast/issues/1478) makes a
compelling case: commodity hardware (USB adapters for ~$10) now supports PTPv2
hardware timestamping with **nanosecond-level** accuracy — orders of magnitude
better than software NTP sync.

Sonium's `TimeSource` trait (planned for Fase 2) is designed with this in mind:
the rest of the audio pipeline is agnostic to *how* the clock offset is
obtained.  Plugging in a PTP-backed `TimeSource` will require no changes to the
encoder, decoder, or jitter buffer.
