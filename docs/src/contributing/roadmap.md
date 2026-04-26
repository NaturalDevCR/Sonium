# Roadmap

Sonium is developed in phases, each delivering independent value.

---

## Completed

### Fase 0 — Foundation ✅

- Cargo workspace with all crates (`sonium-protocol`, `sonium-codec`, `sonium-sync`, `sonium-common`, `sonium-control`, `sonium-server`, `sonium-client`)
- Protocol crate: compact binary wire format (little-endian), all message types, full round-trip unit tests
- Codec crate: Opus encoder/decoder + PCM passthrough
- Sync crate: NTP-like clock offset estimation with 200-sample median filter + jitter buffer
- Server skeleton: TCP listener, broadcaster, session handler, stream reader
- Client skeleton: auto-reconnect, Hello, codec detection, clock sync, player stub
- Documentation site

### Fase 1 — Protocol hardening ✅

- Fuzz testing with `cargo-fuzz` — 3 targets: `fuzz_message_parser`, `fuzz_wire_chunk`, `fuzz_codec_header`
- Seed corpus of 13 hand-crafted binary payloads

### Fase 2 — Clock sync hardening ✅

- Thread-priority tuning (dedicated OS thread + `thread-priority` elevation)
- `TimeSource` trait for PTPv2 groundwork (`NtpTimeSource` default implementation)
- Two-process loopback test rig (`tests/loopback/`)

### Fase 3 — Server MVP ✅

- Stream reader: `stdin`, named FIFO, external process (`pipe://`)
- PCM + Opus + FLAC encoders; FLAC decoder — lossless round-trip verified
- Multiple streams per server (one reader + broadcaster per `[[streams]]` entry)
- Graceful shutdown (`CancellationToken` + SIGINT/SIGTERM)
- Integration test suite (14 tests, `tower::ServiceExt::oneshot` + real TCP for WS)

### Fase 4 — Client MVP with real audio output ✅

- CPAL integration (Linux ALSA/PipeWire, macOS CoreAudio)
- Ring buffer + dedicated audio thread (avoids `!Send` on CoreAudio)
- Advanced underrun recovery (4-phase crossfade state machine)
- USB DAC hotplug with exponential backoff retry

### Fase 5 — Hardening ✅

- Auto-reconnect after server restart (exponential backoff 500 ms → 30 s)
- `tracing` structured logging: `EnvFilter`, `#[instrument]` on sessions and stream readers
- CLI flags and `SONIUM_*` env vars for all config values (server + client)

### Fase 6 — Multiple streams and groups ✅

- Client groups with full CRUD (create, rename, delete, move clients)
- **Live stream switching** — sessions react to `EventBus` events (`ClientGroupChanged`, `GroupStreamChanged`) without dropping the TCP connection; new `CodecHeader` sent on switch
- Per-client volume / mute via REST + WebSocket events
- Per-session volume scaling + mute in the wire path (zero-copy at volume 100)

### Fase 7 — Web UI ✅

- Embedded Vue 3 + Pinia SPA via `rust-embed` — no separate process
- Real-time WebSocket events (`EventBus` → `/api/events`)
- Per-client volume sliders + mute toggle
- Group management with drag-and-drop client assignment
- Dark theme, responsive layout

### Fase 8 — Zero-config discovery ✅

- mDNS advertisement (`_sonium._tcp`, `_sonium-http._tcp`)
- Optional `_snapcast._tcp` when `snapcast_compat = true`
- Client auto-discover (`--discover` + interactive menu)
- Subnet scanner for cross-VLAN networks

---

## In progress / hardware-blocked

### Wire compatibility validation 🔄

- Interop test: Sonium server ↔ real Snapcast client
  — _blocked on hardware test setup; not a blocker for other work_

### Clock sync precision 🔄

- Raspberry Pi LAN test — **goal:** < 1 ms desync
  — _requires Pi hardware; not a blocker_

---

## Planned

### Fase 11 — Authentication & authorization

- Server-side user accounts (username + password, argon2 hashing)
- JWT bearer tokens with configurable TTL
- Role-based access control:
  - **admin** — full access: users, server config, all management
  - **operator** — manage groups/streams/clients/volumes; no user or config access
  - **viewer** — read-only
- `POST /api/auth/setup` — first-run admin account creation
- `POST /api/auth/login` → JWT token
- `GET/POST/PUT/DELETE /api/users` — user management (admin only)
- Auth middleware in axum (Bearer token verification on protected routes)

### Fase 12 — Admin UI

Full-featured admin panel (Vue 3 + Tailwind CSS, served at `/admin`):

| Tab | Description |
|---|---|
| **Dashboard** | Server status, uptime, active clients + streams, live metrics |
| **Streams** | Create/edit/delete audio sources with guided forms (stdin, FIFO, ffmpeg, HTTP, AirPlay, Spotify) |
| **Groups & Clients** | Full management — create groups, move clients, assign streams, volume |
| **Config** | Standard form editor + Expert raw-TOML editor with syntax highlighting; hot-reload |
| **Users** | Create/delete users, assign roles, change passwords |

### Fase 13 — Control PWA

Mobile-first progressive web app (served at `/`):

- Vue 3 + Tailwind CSS; installs as an app on iOS / Android / desktop
- Per-group view: stream selector, group mute, per-client volume sliders
- Per-source volume memory (restore saved volume when switching streams)
- Linked volume mode: move all clients in a group together by delta
- Role-aware UI: operator sees controls, viewer sees read-only display
- Offline page when server unreachable (service worker)
- Dark + light theme, respects system preference

### Fase 14 — Config hot-reload & rich audio sources

- `GET /api/config/raw` → current `sonium.toml` as text
- `PUT /api/config/raw` → write new TOML; validate before saving; live-reload non-structural changes
- Source type wizards in the Admin UI:
  - `stdin` / named FIFO — raw PCM
  - `pipe://` — arbitrary ffmpeg or external process inline
  - HTTP stream — pull from URL (Icecast, HLS)
  - AirPlay — spawn `shairport-sync` as child process
  - Spotify — spawn `librespot` as child process
  - MPD — read from MPD's FIFO output
- Source health indicator in Admin UI (restart stream, view last 50 log lines)

### Fase 9 — Cross-platform

- Windows: WASAPI audio, MSI installer
- Android: evaluation of `oboe-rs` vs thin Kotlin wrapper
- macOS: notarized `.app` bundle (CoreAudio already works via CPAL)

### Fase 10 — Advanced features

| Feature                  | Notes                                                                                          |
| ------------------------ | ---------------------------------------------------------------------------------------------- |
| **PTPv2 hardware clock** | IEEE 1588v2 via `/dev/ptp0` — nanosecond sync. `TimeSource` trait is ready.                   |
| Cross-subnet relay       | Stream to remote networks. Subnet scanner groundwork already exists.                           |
| TLS + HTTPS              | Self-signed cert generation + Let's Encrypt via ACME                                           |
| Plugin sources           | Spotify Connect, AirPlay, Tidal                                                                |
| Volume normalization     | Per-track EBU R128 / ReplayGain                                                                |
| Per-room EQ              | FIR filters via `fundsp`                                                                       |
| Mobile control app       | Flutter or React Native                                                                        |
| Snapcast compat UI toggle| Web UI toggle for `snapcast_compat` mode without editing config files                         |

---

## PTPv2 detail

Commodity hardware (USB adapters ~$10) supports PTPv2 with **nanosecond-level**
accuracy — orders of magnitude better than software NTP. No existing open-source
multiroom audio system takes advantage of this
([relevant discussion](https://github.com/badaix/snapcast/issues/1478)).

Sonium's `TimeSource` trait (Fase 2) makes the audio pipeline agnostic to clock
source. Plugging in a PTP-backed `TimeSource` requires no changes to encoder,
decoder, or jitter buffer.
