# Installation

Sonium ships as two binaries:

| Binary | Install where | Purpose |
| --- | --- | --- |
| `sonium-server` | One machine on the network | Reads audio, hosts the web UI/API, broadcasts streams. |
| `sonium-client` | Every playback machine | Connects to the server and plays synchronized audio. |

## Linux Installer

The Linux installer downloads the right release package, writes
`/etc/sonium/sonium.toml`, creates `/tmp/sonium.fifo`, and optionally installs a
systemd service.

```bash
curl -fsSL https://github.com/NaturalDevCR/Sonium/releases/latest/download/install.sh | sudo bash
```

Useful options:

```bash
sudo bash install.sh --version v0.1.0
sudo bash install.sh --prefix /opt/sonium
sudo bash install.sh --no-service
sudo bash install.sh --server-only
```

After installation:

```bash
systemctl status sonium-server
journalctl -u sonium-server -f
```

Feed audio:

```bash
ffmpeg -re -i song.flac -f s16le -ar 48000 -ac 2 - > /tmp/sonium.fifo
```

Run a client:

```bash
sonium-client --discover
```

## GitHub Release Packages

Download a package from the
[GitHub Releases](https://github.com/NaturalDevCR/Sonium/releases) page.

| Platform | Package |
| --- | --- |
| Linux x86_64 | `sonium-vX.Y.Z-linux-x86_64.tar.gz` |
| Linux aarch64 | `sonium-vX.Y.Z-linux-aarch64.tar.gz` |
| macOS Intel | `sonium-vX.Y.Z-macos-x86_64.tar.gz` |
| macOS Apple Silicon | `sonium-vX.Y.Z-macos-aarch64.tar.gz` |
| Windows x86_64 | `sonium-vX.Y.Z-windows-x86_64.zip` |

Extract the package and place the binaries on your `PATH`.

macOS may quarantine downloaded binaries. If Gatekeeper blocks them:

```bash
xattr -d com.apple.quarantine sonium-server sonium-client
```

On Windows, run from PowerShell:

```powershell
.\sonium-server.exe --config .\sonium.toml
.\sonium-client.exe 192.168.1.50
```

## Docker Server

Docker is useful for the server. The client should usually run directly on the
playback device because it needs access to local audio hardware.

```bash
docker compose up -d
```

The server exposes:

| Port | Purpose |
| --- | --- |
| `1710/tcp` | Sonium audio stream protocol |
| `1711/tcp` | Web UI, REST API, WebSocket events |

## Build from Source

Install Rust and Node.js, then build the embedded web UI before building the
server:

```bash
git clone https://github.com/NaturalDevCR/Sonium
cd sonium

pnpm --dir web install
pnpm --dir web build
cargo build --release --bin sonium-server --bin sonium-client
```

Linux dependencies:

```bash
sudo apt-get install pkg-config libopus-dev libasound2-dev
```

macOS dependencies:

```bash
brew install opus
```

Windows requires the Visual Studio Build Tools with the C++ workload.

## Server vs Client Setup

A typical home setup looks like this:

```text
music source -> sonium-server -> LAN -> sonium-client -> speaker
                                      -> sonium-client -> speaker
                                      -> sonium-client -> speaker
```

Only the server needs the web UI and config file. Each client only needs to know
how to reach the server, either through mDNS discovery or a server IP address.
