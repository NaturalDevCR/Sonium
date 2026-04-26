# Installation

## One-line installer (Linux)

The quickest way to get Sonium running on a Linux system:

```bash
curl -fsSL https://github.com/sonium-audio/sonium/releases/latest/download/install.sh | sudo bash
```

The installer will:
1. Download the pre-built binary for your architecture (`x86_64`, `aarch64`, `armv7`)
2. Create a `sonium` system user
3. Write a default config to `/etc/sonium/server.toml`
4. Create the audio FIFO at `/tmp/sonium.fifo`
5. Install and start the `sonium-server.service` systemd unit

The web UI will be available at `http://<your-ip>:1711` immediately after installation.

### Installer options

```
--prefix /opt      Install binaries to /opt/bin instead of /usr/local/bin
--no-service       Skip systemd service installation
--version 0.2.0    Install a specific release version
```

---

## From source

```bash
# Requires Rust 1.75+
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

git clone https://github.com/sonium-audio/sonium
cd sonium

# Build both binaries in release mode
cargo build --release --bin sonium-server --bin sonium-client

# Copy binaries to PATH
sudo cp target/release/sonium-server target/release/sonium-client /usr/local/bin/
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

`audiopus-sys` compiles `libopus` from source automatically if Visual Studio
Build Tools are installed — no extra steps needed.

---

## Systemd service

The installer writes the following unit file; you can also install it manually:

```bash
sudo cp sonium-server.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now sonium-server
```

Full unit file (also at [`sonium-server.service`](https://github.com/sonium-audio/sonium/blob/main/sonium-server.service) in the repo):

```ini
[Unit]
Description=Sonium multiroom audio server
After=network.target sound.target
Wants=network.target

[Service]
Type=simple
User=sonium
ExecStart=/usr/local/bin/sonium-server --config /etc/sonium/server.toml
Restart=on-failure
RestartSec=5s
AmbientCapabilities=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/log/sonium /tmp

[Install]
WantedBy=multi-user.target
```

---

## Raspberry Pi

Sonium is designed to run comfortably on a Raspberry Pi 4 or later.  The
`aarch64` pre-built binary targets Alpine-style musl libc and runs on any
64-bit Pi OS without needing extra runtime libraries.

For cross-compilation from an x86 host:

```bash
rustup target add aarch64-unknown-linux-musl
cargo build --release --target aarch64-unknown-linux-musl --bin sonium-client
```

---

## Pre-built binaries

Pre-built binaries are available on the
[GitHub Releases](https://github.com/sonium-audio/sonium/releases) page.

| Platform | Binary |
|---|---|
| Linux x86_64 | `sonium-server-x86_64-unknown-linux-musl` |
| Linux aarch64 | `sonium-server-aarch64-unknown-linux-musl` |
| Linux armv7 | `sonium-server-armv7-unknown-linux-musleabihf` |
| macOS arm64 | `sonium-server-aarch64-apple-darwin` |
