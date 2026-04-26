#!/usr/bin/env bash
# Sonium installer — installs sonium-server and sonium-client on Linux.
# Usage: curl -fsSL https://example.com/install.sh | bash
#        or: bash install.sh [--prefix /usr/local] [--no-service]
set -euo pipefail

# ── Defaults ──────────────────────────────────────────────────────────────
PREFIX="/usr/local"
INSTALL_SERVICE=true
SONIUM_USER="sonium"
AUDIO_INPUT="/tmp/sonium.fifo"
CONTROL_PORT=1711
STREAM_PORT=1710
VERSION="${SONIUM_VERSION:-latest}"

# ── Argument parsing ──────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)        PREFIX="$2";          shift 2 ;;
    --no-service)    INSTALL_SERVICE=false; shift   ;;
    --version)       VERSION="$2";         shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

BIN_DIR="$PREFIX/bin"
CONF_DIR="/etc/sonium"
LOG_DIR="/var/log/sonium"

# ── Helpers ───────────────────────────────────────────────────────────────
info()  { echo "  \033[34m→\033[0m $*"; }
ok()    { echo "  \033[32m✓\033[0m $*"; }
warn()  { echo "  \033[33m⚠\033[0m $*"; }
die()   { echo "  \033[31m✗\033[0m $*" >&2; exit 1; }

need_root() {
  if [[ $EUID -ne 0 ]]; then
    die "This step requires root. Re-run with sudo or as root."
  fi
}

detect_arch() {
  local arch
  arch=$(uname -m)
  case "$arch" in
    x86_64)  echo "x86_64-unknown-linux-musl" ;;
    aarch64) echo "aarch64-unknown-linux-musl" ;;
    armv7l)  echo "armv7-unknown-linux-musleabihf" ;;
    *) die "Unsupported architecture: $arch" ;;
  esac
}

# ── Main ──────────────────────────────────────────────────────────────────
echo ""
echo "  ╔══════════════════════════════════╗"
echo "  ║   Sonium Installer               ║"
echo "  ╚══════════════════════════════════╝"
echo ""

ARCH=$(detect_arch)
info "Detected architecture: $ARCH"

# Determine download URL
if [[ "$VERSION" == "latest" ]]; then
  RELEASE_URL="https://github.com/sonium-audio/sonium/releases/latest/download"
else
  RELEASE_URL="https://github.com/sonium-audio/sonium/releases/download/v${VERSION}"
fi

# ── Download binaries ─────────────────────────────────────────────────────
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

info "Downloading binaries for $ARCH…"
for bin in sonium-server sonium-client; do
  curl -fsSL "${RELEASE_URL}/${bin}-${ARCH}" -o "${TMP_DIR}/${bin}" \
    || die "Failed to download ${bin}. Check your connection or try --version <tag>."
  chmod +x "${TMP_DIR}/${bin}"
done
ok "Download complete"

# ── Install binaries ──────────────────────────────────────────────────────
need_root
install -d "$BIN_DIR"
install -m 0755 "${TMP_DIR}/sonium-server" "${BIN_DIR}/sonium-server"
install -m 0755 "${TMP_DIR}/sonium-client" "${BIN_DIR}/sonium-client"
ok "Installed to ${BIN_DIR}"

# ── Create system user ────────────────────────────────────────────────────
if ! id "$SONIUM_USER" &>/dev/null; then
  useradd --system --no-create-home --shell /usr/sbin/nologin "$SONIUM_USER"
  ok "Created system user: $SONIUM_USER"
else
  info "User $SONIUM_USER already exists — skipping"
fi

# Add sonium to the audio group so it can access ALSA/PipeWire
if getent group audio &>/dev/null; then
  usermod -aG audio "$SONIUM_USER" 2>/dev/null || true
fi

# ── Create directories ────────────────────────────────────────────────────
install -d -m 0755 -o "$SONIUM_USER" "$CONF_DIR"
install -d -m 0755 -o "$SONIUM_USER" "$LOG_DIR"
ok "Created $CONF_DIR and $LOG_DIR"

# ── Write default config ──────────────────────────────────────────────────
if [[ ! -f "${CONF_DIR}/server.toml" ]]; then
  cat > "${CONF_DIR}/server.toml" <<EOF
[server]
stream_port   = ${STREAM_PORT}
control_port  = ${CONTROL_PORT}
mdns          = true

[server.audio]
# Path to the named pipe (FIFO) that feeds audio into the server.
# Pipe audio here: ffmpeg -i input.flac -f s16le -ar 48000 -ac 2 - > ${AUDIO_INPUT}
source = "${AUDIO_INPUT}"
sample_rate   = 48000
bit_depth     = 16
channels      = 2
chunk_ms      = 20
EOF
  ok "Wrote ${CONF_DIR}/server.toml"
else
  info "${CONF_DIR}/server.toml already exists — not overwritten"
fi

# ── Create audio FIFO ─────────────────────────────────────────────────────
if [[ ! -p "$AUDIO_INPUT" ]]; then
  mkfifo "$AUDIO_INPUT"
  chown "$SONIUM_USER":audio "$AUDIO_INPUT" 2>/dev/null || chown "$SONIUM_USER" "$AUDIO_INPUT"
  ok "Created FIFO: $AUDIO_INPUT"
fi

# ── Install systemd service ───────────────────────────────────────────────
if $INSTALL_SERVICE && command -v systemctl &>/dev/null; then
  cat > /etc/systemd/system/sonium-server.service <<EOF
[Unit]
Description=Sonium multiroom audio server
After=network.target sound.target
Wants=network.target

[Service]
Type=simple
User=${SONIUM_USER}
ExecStart=${BIN_DIR}/sonium-server --config ${CONF_DIR}/server.toml
Restart=on-failure
RestartSec=5s
StandardOutput=journal
StandardError=journal
SyslogIdentifier=sonium-server
AmbientCapabilities=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${LOG_DIR} /tmp

[Install]
WantedBy=multi-user.target
EOF

  systemctl daemon-reload
  systemctl enable sonium-server
  systemctl restart sonium-server
  ok "Systemd service enabled and started"
else
  [[ "$INSTALL_SERVICE" == "true" ]] && warn "systemd not found — skipping service install"
fi

# ── Done ──────────────────────────────────────────────────────────────────
echo ""
echo "  ╔══════════════════════════════════════════════════╗"
echo "  ║   Sonium installed successfully!                 ║"
echo "  ║                                                  ║"
echo "  ║   Web UI: http://$(hostname -I | awk '{print $1}' 2>/dev/null || echo 'your-ip'):${CONTROL_PORT}          ║"
echo "  ║   Config: ${CONF_DIR}/server.toml         ║"
echo "  ║                                                  ║"
echo "  ║   Feed audio:                                    ║"
echo "  ║   ffmpeg -i input.flac -f s16le -ar 48000 \\     ║"
echo "  ║     -ac 2 - > ${AUDIO_INPUT}              ║"
echo "  ╚══════════════════════════════════════════════════╝"
echo ""
