# Installation

Sonium ships as two binaries:

| Binary | Install where | Purpose |
| --- | --- | --- |
| `sonium-server` | One machine on the network | Reads audio, hosts the web UI/API, broadcasts streams. |
| `sonium-client` | Every playback machine | Connects to the server and plays synchronized audio. |

## Prerequisites for Multi-Room Sync

For **sample-accurate multi-room synchronisation** (multiple clients playing in
perfect unison), all devices must share a common time reference within ±5 ms.

### Linux (Server + Client)

Install **chrony** on every Linux device:

```bash
sudo apt-get install chrony   # Debian/Ubuntu
sudo dnf install chrony       # Fedora
sudo pacman -S chrony         # Arch
```

Verify sync quality:

```bash
chronyc tracking
# Look for "System time" — should be within ±0.005 seconds
```

### macOS (Client)

macOS uses `sntp` by default. Verify:

```bash
sntp -s time.apple.com
```

### Windows (Client)

Windows Time service usually suffices. For better accuracy, install
[Meinberg NTP](https://www.meinberg.de/english/sw/ntp.htm).

### Time Zone Configuration

Sonium uses UTC internally, but logs and the web UI display local time.
Set the timezone on each device:

```bash
# Linux
sudo timedatectl set-timezone America/Costa_Rica

# macOS
sudo systemsetup -settimezone America/Costa_Rica
```

Or configure via the Sonium Agent UI (client-side only).

---

## Linux Installer

The Linux installer downloads the right release package, writes
`/etc/sonium/sonium.toml`, creates `/tmp/sonium.fifo`, and optionally installs a
systemd service. It also installs a narrowly scoped sudoers rule so the web UI
can restart `sonium-server.service` after admin-approved config changes.

```bash
curl -fsSL https://github.com/NaturalDevCR/Sonium/releases/latest/download/install.sh | sudo bash
```

Useful options:

```bash
sudo bash install.sh --version v0.1.0
sudo bash install.sh --prefix /opt/sonium
sudo bash install.sh --no-service
sudo bash install.sh --server-only
sudo bash install.sh --client-only
```

After installation:

```bash
systemctl status sonium-server
journalctl -u sonium-server -f
```

If the admin UI says restart is not permitted, the service was likely installed
before restart permissions existed or was written by hand. Re-run the installer
or add an equivalent sudoers rule for the Sonium service user.

Feed audio:

```bash
ffmpeg -re -i song.flac -f s16le -ar 48000 -ac 2 - > /tmp/sonium.fifo
```

Run a client:

```bash
sonium-client --discover
```

## Desktop Agent

For macOS and Windows playback machines, the recommended client experience is
the Sonium Desktop Agent from the
[GitHub Releases](https://github.com/NaturalDevCR/Sonium/releases) page. It runs
in the tray/menu bar and lets you configure client instances, output devices,
latency, and background startup without editing config files.

## GitHub Release Packages

Download CLI packages from the
[GitHub Releases](https://github.com/NaturalDevCR/Sonium/releases) page.

| Platform | Package |
| --- | --- |
| Linux x86_64 | `sonium-vX.Y.Z-linux-x86_64.tar.gz` |
| Linux aarch64 | `sonium-vX.Y.Z-linux-aarch64.tar.gz` |
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
