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

The generated control listener is `127.0.0.1`, while the audio listener is
available on the LAN. This is deliberately a trusted-LAN profile: media is not
TLS-encrypted or authenticated. Do not expose Sonium through a router or public
address. To administer from another LAN machine, change `control_bind`
explicitly and restrict `control_port` with the host firewall.

`/etc/sonium` is private (`0700`) because `users.json` within it holds password
hashes and the persistent JWT signing secret. Keep that directory persistent,
back it up securely, and never add its contents to a configuration repository.

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

If upgrading an existing deployment, leave `users.json` in place. Old records
without `session_version` migrate to version `0` on a successful startup; users
must sign in again to obtain versioned tokens. A corrupt or unreadable existing
account file stops the server and is deliberately not replaced—restore it from
a known-good backup before retrying.

The installer also checks an existing `sonium.toml` **before** it downloads,
replaces binaries, or changes systemd. Phase 1 rejects legacy
`buffer_ms`, `chunk_ms`, and `output_prefill_ms` keys under `[server]`; if it
finds any, it aborts without stopping the existing service and tells you to move
those same values under `[server.audio]`, then rerun. Check an upgrade file
without installing anything with:

```bash
sudo bash install.sh --preflight-config /etc/sonium/sonium.toml
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
$adminPassword = Read-Host -AsSecureString "Initial Sonium admin password"
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($adminPassword)
try {
  $plainPassword = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
  .\sonium-server.exe --config .\sonium.toml --init-admin $plainPassword
  if ($LASTEXITCODE -ne 0) { throw "Initial admin setup failed" }
}
finally {
  if ($null -ne $bstr) { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
  Remove-Variable plainPassword -ErrorAction SilentlyContinue
}
.\sonium-server.exe --config .\sonium.toml
.\sonium-client.exe 192.168.1.50
```

The password is prompted rather than written into PowerShell history, TOML, or
an environment file. Run the bootstrap on the local trusted machine before the
first normal server start; a server without `users.json` intentionally refuses
to start.

## Docker Server

Docker is useful for the server. The client should usually run directly on the
playback device because it needs access to local audio hardware. On first boot,
prompt for the administrator password without putting it in Compose, TOML, or
shell history, then run the bootstrap profile before the server service:

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

The Compose template supplies a strict, valid TOML file, persists `users.json`
in the named volume, and publishes the control port only as
`127.0.0.1:1711`. Open it on the Docker host or through an SSH tunnel. To make
it reachable from a trusted LAN, deliberately change the published port and
add host-firewall restrictions; that does not add TLS or media authentication.
The included stream reads stdin as a placeholder. Replace it with a mounted
file/FIFO source for a persistent deployment. Only recoverable file/FIFO
open/read/EOF conditions enter `recovering` and reopen automatically; terminal
path errors such as permissions, directories, unsupported paths, or symlink
loops enter `error`. A `pipe://` child closing uses its separate restart loop
and is not reported as this file/FIFO recovery state.

The server exposes:

| Port | Purpose |
| --- | --- |
| `1710/tcp` | Sonium audio stream protocol |
| `1711/tcp` | Web UI, REST API, WebSocket events |
| `1712/udp` or auto-bound UDP | Optional `rtp_udp` / `rist` media transport when enabled |

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
