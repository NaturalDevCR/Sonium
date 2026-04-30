# Sonium

Open-source multiroom audio for local networks. Sonium runs one server that
receives audio and a lightweight client on every playback device.

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

[[streams]]
id = "default"
display_name = "Main"
source = "/tmp/sonium.fifo"
codec = "opus"
buffer_ms = 1000
chunk_ms = 20
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

Sonium is in active early development. The core audio path, web UI, REST API,
metrics, Docker server flow, release packaging, multi-codec support, and client
sync loop are under active iteration.

## License

GNU General Public License v3.0.
