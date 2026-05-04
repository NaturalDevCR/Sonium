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

- **Built-in web UI** with control view, admin dashboard, and **real-time sync
  monitor**.
- **Users, roles, JWT auth**, first-run/admin setup, and role-aware UI.
- **Groups, per-client volume/mute/latency, EQ**, and live stream switching.
- **Multiple configured streams**, including FIFO/files, TCP, `pipe://` external
  processes, ffmpeg-style radio sources, and meta streams.
- **External stream recovery**: `pipe://` sources restart with backoff if their
  stdout closes.
- **System/admin tooling**: dependency checks, raw TOML editing, log viewer with
  time filters, and restart requests when systemd permissions are installed.
- **Multi-room sync foundation**: GroupSync protocol, timezone config, and
  chrony integration guidance.
- **Same-machine optimization**: `--on-server` flag skips network sync when
  client and server share a machine.
- **Sonium Desktop Agent** for macOS/Windows to configure client instances.
- **Client audio output** through CPAL with dedicated audio thread, underrun
  crossfade, device hotplug recovery, output prefill, and `chunk_ms` control.

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

[server.audio]
buffer_ms = 200
chunk_ms = 10
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

- **Low-latency reliability:** TCP streaming is now stable at 200 ms buffer on
  most LANs, but Wi-Fi and mixed networks may need 400–800 ms. Auto-buffer
  tuning is available but still experimental.
- **Clock sync precision:** built-in protocol achieves ~10–50 ms. For < 1 ms
  accuracy, chrony/NTP is required. PTP hardware support is planned.
- **Source supervision:** `pipe://` sources recover automatically, but ffmpeg
  stderr diagnostics are not yet surfaced in the UI.
- **Upgrade/installer edges:** Linux systemd installs work best through the
  installer; hand-written services may miss the sudoers restart permission.
- **Observability:** health reports, sync status, and logs are visible in the UI,
  but automated troubleshooting workflows are not yet implemented.
- **Compatibility:** Snapcast discovery/migration pieces exist, but full
  drop-in compatibility is not guaranteed.

### Roadmap

**Recently Completed (v0.1.78):**
- ✅ TCP streaming stability: dedicated writer task, audio-first drain loop
- ✅ Remove faulty RTT filter, eliminate State::Buffering gate
- ✅ GroupSync protocol for multi-room shared timeline
- ✅ Timezone config support
- ✅ `--on-server` flag for same-machine optimization
- ✅ Web UI redesign: Dashboard, Sync Monitor, Expert mode toggle

**In Progress:**
- Smart GroupSync: compute median group offset server-side
- Source quality reporting in GroupSync (chrony integration)
- Web UI setup wizard for first-time users
- Auto-buffer tuning validation on real hardware

**Planned:**
- Improve diagnostics: surface ffmpeg stderr, automated troubleshooting
- Harden restart/config flows: partial reloads, permission checks
- PTP/hardware timestamp support for sub-microsecond sync
- Relay/cross-subnet modes, TLS, richer source integrations
- Continue packaging polish for Linux, macOS, and Windows Desktop Agent

## License

GNU General Public License v3.0.
