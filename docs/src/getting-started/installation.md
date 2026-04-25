# Installation

## From source (recommended for now)

```bash
# Requires Rust 1.75+
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

git clone https://github.com/jdavidoa91/sonium
cd sonium
cargo build --release --bin sonium-server --bin sonium-client
```

### System dependencies

#### Linux

Opus development headers are required for the `audiopus` crate:

```bash
# Debian / Ubuntu / Raspberry Pi OS
sudo apt install libopus-dev

# Arch
sudo pacman -S opus

# Fedora / RHEL
sudo dnf install opus-devel
```

#### macOS

```bash
brew install opus
```

#### Windows

On Windows, `audiopus-sys` will attempt to compile `libopus` from source
automatically — no extra dependencies needed if you have Visual Studio Build
Tools installed.

## Pre-built binaries

Pre-built binaries for Linux (x86\_64, aarch64) and macOS (arm64) will be
available on the [GitHub Releases](https://github.com/jdavidoa91/sonium/releases)
page once the project reaches its **Fase 4** milestone.

## Raspberry Pi

Sonium is designed to run comfortably on a Raspberry Pi 4 or later.  For
maximum audio performance, consider using the `--release` build:

```bash
cargo build --release --target aarch64-unknown-linux-gnu
```

Cross-compilation from a faster host machine is recommended to reduce build
time.

## Systemd service (Linux)

```ini
# /etc/systemd/system/sonium-server.service
[Unit]
Description=Sonium multiroom audio server
After=network.target

[Service]
ExecStart=/usr/local/bin/sonium-server
Restart=on-failure
RestartSec=5
User=audio
Group=audio

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now sonium-server
```
