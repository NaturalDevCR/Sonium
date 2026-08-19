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
  monitor**, refreshed responsive styling, and role-aware routes.
- **Users, roles, JWT auth**, first-run/admin setup, and role-aware UI.
- **Groups, per-client volume/mute/latency, EQ**, and live stream switching.
- **Multiple configured streams**, including FIFO/files, TCP, `pipe://` external
  processes, ffmpeg-style radio sources, virtual AirPlay/Spotify templates in
  the UI, and meta streams.
- **External stream recovery**: `pipe://` sources restart with backoff if their
  stdout closes; file and FIFO inputs reopen after producer disconnects or
  replacement; ffmpeg stderr is captured for diagnostics.
- **System/admin tooling**: dependency checks, raw TOML editing, log viewer with
  time filters, and restart requests when systemd permissions are installed.
- **Multi-room sync foundation**: GroupSync, server-computed group offsets,
  client drift nudging, source-quality reporting, timezone config, and chrony
  integration guidance.
- **Pluggable media transports**: stable TCP today, implemented-but-validating
  `rtp_udp`, experimental Sonium-native ARQ/FEC over UDP via `rist`, and a
  config-visible `quic_dgram` placeholder for a later encrypted datagram path.
- **Same-machine optimization**: `--on-server` flag skips network sync when
  client and server share a machine.
- **Sonium Desktop Agent** for macOS/Windows to configure client instances.
- **Client audio output** through CPAL with dedicated audio thread, underrun
  crossfade, device hotplug recovery, output prefill, and `chunk_ms` control.
- **Observability** through HealthReport sync metrics, Prometheus `/metrics`,
  Sync Monitor UI, and Home Assistant entities for groups, clients, streams, and
  client health.

## Operating boundary (Phase 1)

Sonium Phase 1 is intended for a **trusted LAN only**. The audio stream is not
TLS-encrypted or mutually authenticated, and the experimental UDP transports
are not hardened for untrusted networks. Do not port-forward Sonium, place it
directly on the Internet, or treat it as a VPN replacement. TCP is the
supported transport default; `rtp_udp` and `rist` still need broader field
validation.

`server.bind` controls the audio listener and commonly remains `0.0.0.0` on a
LAN. The control API, web UI, and metrics listener use the separate
`server.control_bind`, which defaults to `127.0.0.1`. Keep it there and use a
local browser or SSH tunnel whenever possible. If remote LAN administration is
required, set it explicitly to a private LAN address (or `0.0.0.0`) and limit
`control_port` with the host firewall to the administrators who need it.

Authentication protects the control plane; it does not authenticate media.
The installer creates an initial admin only when `users.json` is absent. Store
`/etc/sonium` on persistent local storage: it contains password hashes and the
durable JWT signing secret. Sonium writes the account file atomically with
owner-only permissions and keeps its directory private on Unix. Do not put
passwords, JWTs, or `users.json` in TOML, Compose files, source control, URLs,
or logs.

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
read -r -s -p "Initial Sonium admin password: " SONIUM_INIT_ADMIN_PASSWORD
printf '\n'
export SONIUM_INIT_ADMIN_PASSWORD
if docker compose --profile bootstrap run --rm init-admin; then
  unset SONIUM_INIT_ADMIN_PASSWORD
  docker compose up -d
else
  status=$?
  unset SONIUM_INIT_ADMIN_PASSWORD
  exit "$status"
fi
```

This bootstrap creates the administrator before the first normal server start
without saving the password in Compose or shell history. See the installation
guide for the trusted-LAN control-port profile and source mounting.

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
control_bind = "127.0.0.1"
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

read -r -s -p "Initial Sonium admin password: " SONIUM_INIT_ADMIN_PASSWORD
printf '\n'
if ./target/release/sonium-server --config sonium.toml --init-admin "$SONIUM_INIT_ADMIN_PASSWORD"; then
  unset SONIUM_INIT_ADMIN_PASSWORD
  ./target/release/sonium-server --config sonium.toml
else
  status=$?
  unset SONIUM_INIT_ADMIN_PASSWORD
  exit "$status"
fi
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
- [Real-Time Transport Plan](https://naturaldevcr.github.io/Sonium/architecture/transport-migration-plan)
- [Roadmap](https://naturaldevcr.github.io/Sonium/contributing/roadmap)

## Current Status

Sonium is usable for experimentation, but still rough. The web UI, auth,
configuration flow, multi-stream model, release packaging, and client playback
loop are all active development surfaces. Expect bugs and occasional breaking
changes between releases.

### Known Challenges

- **Transport validation:** TCP remains the safest default. `rtp_udp` and `rist`
  are implemented for the real-time-first migration, but still need wider live
  hardware validation before becoming default recommendations. `quic_dgram` is
  intentionally not implemented yet.
- **Low-latency reliability:** TCP is much stronger than earlier releases, but
  Wi-Fi and mixed networks may still need conservative buffers. Auto-buffer
  tuning exists and is still being validated.
- **Clock sync precision:** GroupSync and sync telemetry are in place, but the
  project still needs measured multi-device validation before claiming
  Sonos-class reliability. Chrony/NTP is recommended for tighter clocks; PTP and
  hardware timestamping remain future work.
- **Source supervision:** file/FIFO inputs reopen after producer restarts and
  expose `recovering` retry state; terminal configuration or permission errors
  expose `error`. `pipe://` recovery and ffmpeg stderr capture also exist, but
  operator-facing source health and guided remediation are still maturing.
- **Upgrade/installer edges:** Linux systemd installs work best through the
  installer; hand-written services may miss restart permissions or migration
  steps.
- **Compatibility:** Snapcast discovery/migration pieces exist, but full
  drop-in compatibility with every Snapcast client/version is not guaranteed.

### Roadmap Toward Sonos-Class Reliability

**Recently Completed (through v0.1.90):**
- Stability Phase 1: fail-closed strict configuration and account storage,
  session-version invalidation, bounded client admission, loopback-by-default
  control listener, and supervised file/FIFO recovery.
- TCP streaming stability work: dedicated writers, audio-first draining, relaxed
  false-positive sync warnings, and UDP auto-bind fixes.
- GroupSync protocol with server-computed group offsets, smoother client nudges,
  source-quality field, and health-report sync metrics in the UI.
- Real-time transport foundation: `sonium-transport`, RTP/UDP media sender,
  experimental ARQ/FEC/NACK path exposed as `rist`, and transport API/UI hooks.
- Observability phase A: extended HealthReport, playout percentiles, callback
  timing, drift/drop/dup counters, Prometheus metrics, and Sync Monitor updates.
- Web/admin redesign, first-run auth flow hardening, stream templates for
  AirPlay and Spotify Connect helpers, and Home Assistant integration.
- Release packaging polish for Linux plus macOS/Windows Desktop Agent builds.

**Next focus:**
- Validate `rtp_udp` and `rist` on real LAN/Wi-Fi hardware, then document safe
  profiles for default, low-latency, and recovery-heavy deployments.
- Build operator-facing source diagnostics around ffmpeg stderr, stream state,
  restart history, and likely next actions.
- Turn health telemetry into automatic buffer recommendations and safer
  auto-buffer behavior.
- Harden config reload/restart flows and upgrade checks.
- Validate the callback-driven audio path under real-time scheduling; it is not
  yet a certified real-time callback design.
- Define and test authenticated media/TLS and complete UDP hardening before any
  routed or untrusted-network deployment guidance.

**Longer-term:**
- QUIC DATAGRAM transport for encrypted/routed deployments.
- PTP/hardware timestamp support through the `TimeSource` abstraction.
- Relay/cross-subnet operation, TLS deployment profiles, richer source
  integrations, calibration/DSP tools, and safer auto-update flows.
- Signed packaging, supported-platform certification, and long-running
  fault-injection validation.

## License

GNU General Public License v3.0.
