# Sonium

Open-source multiroom audio for local networks. Sonium runs one server that
receives audio and a lightweight client on every playback device.

> [!WARNING]
> **Sonium is not production-ready.** It is an early, fast-moving project with
> known audio stability gaps, sync edge cases, rough upgrade paths, and
> incomplete hardening. It is suitable for experiments, local testing, and
> helping shape the project. Do not rely on it for venues, unattended installs,
> alarms, commercial environments, or any setup where audio dropouts matter.

[![CI](https://github.com/NaturalDevCR/Sonium/actions/workflows/ci.yml/badge.svg)](https://github.com/NaturalDevCR/Sonium/actions/workflows/ci.yml)
[![Docs](https://github.com/NaturalDevCR/Sonium/actions/workflows/docs.yml/badge.svg)](https://naturaldevcr.github.io/Sonium/)
[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](https://www.gnu.org/licenses/gpl-3.0.html)

## How It Works

```text
music source -> sonium-server -> LAN -> sonium-client -> speaker
                                      -> sonium-client -> speaker
                                      -> sonium-client -> speaker
```

- `sonium-server` reads PCM audio, encodes stream chunks, hosts the web UI/API,
  and coordinates groups, volume, latency, EQ, and stream selection.
- `sonium-client` runs on each playback device, discovers or connects to the
  server, syncs time, decodes audio, and writes to local speakers.

## What Works Today

- Built-in web UI with control view and admin dashboard.
- Users, roles, JWT auth, first-run/admin setup, and role-aware UI.
- Groups, per-client volume/mute/latency, EQ, and live stream switching.
- Multiple configured streams, including FIFO/files, TCP, `pipe://` external
  processes, ffmpeg-style radio sources, and meta streams.
- External stream recovery: `pipe://` sources restart with backoff if their
  stdout closes.
- System/admin tooling: dependency checks, raw TOML editing, log viewer with
  time filters, and restart requests when systemd permissions are installed.
- Local-time structured logs with ANSI disabled for easier journal/UI reading.
- Sonium Desktop Agent for macOS/Windows to configure client instances.
- Client audio output through CPAL with a dedicated audio thread, underrun
  crossfade, device hotplug recovery, output prefill, and manual stream
  `chunk_ms` control.

## Install

### Linux

> [!IMPORTANT]
> **System Requirements**: The pre-compiled binaries require **GLIBC 2.39** or higher. This means you need at least **Debian 13 (Trixie/Testing)**, **Ubuntu 24.04 (Noble)**, or any other modern rolling release. If you are on an older system (like Debian 12), you must compile from source.

Use the automated installation script:

```bash
curl -fsSL https://github.com/NaturalDevCR/Sonium/releases/latest/download/install.sh | sudo bash
```

### macOS & Windows

We provide a lightweight native Desktop Agent (`.dmg` for macOS, `.exe` for Windows) that runs in the system tray. This app allows you to configure Sonium instances, select devices, and manage automatic background startup without relying on command line tools.

1. Head to the [Releases](https://github.com/NaturalDevCR/Sonium/releases) page.
2. Download the `.dmg` file for macOS or `.exe` for Windows.
3. Install and run it, and you'll find the Sonium icon in your system tray!

Docker can run the server:

```bash
docker compose up -d
```

The client should usually run directly on the playback machine because it needs
access to local audio hardware.

## Quick Start from Source

```bash
git clone https://github.com/NaturalDevCR/Sonium
cd sonium

pnpm --dir web install
pnpm --dir web build
cargo build --release --bin sonium-server --bin sonium-client
```

Create a FIFO-backed stream:

```bash
mkfifo /tmp/sonium.fifo
cat > sonium.toml <<'EOF'
[server]
bind = "0.0.0.0"
stream_port = 1710
control_port = 1711
mdns = true
buffer_ms = 1000
chunk_ms = 20
output_prefill_ms = 0

[[streams]]
id = "default"
display_name = "Main"
source = "/tmp/sonium.fifo"
codec = "opus"
silence_on_idle = true

[log]
level = "info"
EOF

./target/release/sonium-server --config sonium.toml
```

Feed audio in another terminal:

```bash
ffmpeg -re -f lavfi -i "sine=frequency=440" \
  -f s16le -ar 48000 -ac 2 - > /tmp/sonium.fifo
```

Connect a client:

```bash
./target/release/sonium-client --discover
# or
./target/release/sonium-client 192.168.1.50
```

Open the web UI at <http://127.0.0.1:1711>.

## Documentation

Full docs: [naturaldevcr.github.io/Sonium](https://naturaldevcr.github.io/Sonium/)

- [Quick Start](https://naturaldevcr.github.io/Sonium/getting-started/quick-start)
- [Installation](https://naturaldevcr.github.io/Sonium/getting-started/installation)
- [Configuration](https://naturaldevcr.github.io/Sonium/getting-started/configuration)
- [Architecture](https://naturaldevcr.github.io/Sonium/architecture/overview)

## Current Status

Sonium is usable for experimentation, but still rough. The web UI, auth,
configuration flow, multi-stream model, release packaging, and client playback
loop are all active development surfaces. Expect bugs and occasional breaking
changes between releases.

### Known Challenges

- **Low-latency reliability:** buffers below ~1000 ms can still produce
  dropouts on some machines/networks. Recent client-side output prefill and
  `chunk_ms` support help, but this needs more real-world tuning.
- **Clock sync precision:** software sync works, but sub-millisecond sync across
  varied hardware is not proven yet.
- **Source supervision:** `pipe://` sources now recover, but we still need better
  diagnostics for ffmpeg/network-radio failure modes.
- **Upgrade/installer edges:** Linux systemd installs work best through the
  installer; hand-written services may miss the sudoers restart permission.
- **Observability:** logs are clearer and filterable, but there is no complete
  troubleshooting workflow for buffer underruns, network jitter, or device
  callback timing.
- **Compatibility:** Snapcast discovery/migration pieces exist, but full
  drop-in compatibility is not guaranteed.

### Roadmap

- Stabilize client playback under lower buffer sizes: adaptive output prefill,
  better jitter metrics, and automatic buffer recommendations.
- Make stream tuning friendlier: optional auto mode for `buffer_ms`/`chunk_ms`,
  while keeping manual controls for advanced users.
- Improve diagnostics: surface underruns, stale drops, jitter, process restarts,
  and ffmpeg stderr in the admin UI.
- Harden restart/config flows: clearer prompts, permission checks, and safer
  partial reloads when a full server restart is not needed.
- Validate synchronization on real multi-device hardware, including Raspberry Pi
  and mixed macOS/Linux clients.
- Continue packaging polish for Linux, macOS, and Windows Desktop Agent.
- Longer term: PTP/hardware timestamp support, relay/cross-subnet modes, TLS,
  and richer source integrations.

## License

GNU General Public License v3.0.
