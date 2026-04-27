#!/usr/bin/env bash
# Sonium installer for Linux hosts.
set -euo pipefail

REPO="${SONIUM_REPO:-jdavidoa91/sonium}"
VERSION="${SONIUM_VERSION:-latest}"
PREFIX="${PREFIX:-/usr/local}"
INSTALL_SERVICE=true
INSTALL_CLIENT=true
SONIUM_USER="${SONIUM_USER:-sonium}"
CONF_DIR="${SONIUM_CONFIG_DIR:-/etc/sonium}"
FIFO_PATH="${SONIUM_FIFO:-/tmp/sonium.fifo}"
STREAM_PORT="${SONIUM_STREAM_PORT:-1710}"
CONTROL_PORT="${SONIUM_CONTROL_PORT:-1711}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix) PREFIX="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --repo) REPO="$2"; shift 2 ;;
    --no-service) INSTALL_SERVICE=false; shift ;;
    --server-only) INSTALL_CLIENT=false; shift ;;
    -h|--help)
      cat <<EOF
Sonium Linux installer

Usage:
  curl -fsSL https://github.com/${REPO}/releases/latest/download/install.sh | sudo bash
  sudo bash install.sh [--version v0.1.0] [--prefix /usr/local] [--no-service]

Options:
  --version TAG    Release tag to install, for example v0.1.0
  --repo OWNER/REPO
  --prefix DIR    Install into DIR/bin
  --no-service    Install binaries and config only
  --server-only   Skip sonium-client
EOF
      exit 0
      ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

info() { printf '  \033[34m->\033[0m %s\n' "$*"; }
ok() { printf '  \033[32mOK\033[0m %s\n' "$*"; }
die() { printf '  \033[31mERR\033[0m %s\n' "$*" >&2; exit 1; }

if [[ "$(uname -s)" != "Linux" ]]; then
  die "This installer is for Linux. Download macOS/Windows packages from GitHub Releases."
fi

if [[ "${EUID}" -ne 0 ]]; then
  die "Run as root, for example: curl ... | sudo bash"
fi

case "$(uname -m)" in
  x86_64|amd64) PACKAGE_ARCH="linux-x86_64" ;;
  aarch64|arm64) PACKAGE_ARCH="linux-aarch64" ;;
  *) die "Unsupported Linux architecture: $(uname -m)" ;;
esac

if [[ "${VERSION}" == "latest" ]]; then
  info "Resolving latest release for ${REPO}"
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' \
    | head -n 1)"
  [[ -n "${VERSION}" ]] || die "Could not resolve latest release tag"
fi

TAG="${VERSION#v}"
BASE_URL="https://github.com/${REPO}/releases/download/v${TAG}"
PACKAGE_NAME="sonium-v${TAG}-${PACKAGE_ARCH}.tar.gz"

BIN_DIR="${PREFIX}/bin"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

echo
echo "Sonium installer"
echo
info "Downloading ${PACKAGE_NAME}"
curl -fsSL "${BASE_URL}/${PACKAGE_NAME}" -o "${TMP_DIR}/sonium.tar.gz" \
  || die "Could not download ${BASE_URL}/${PACKAGE_NAME}"

tar -xzf "${TMP_DIR}/sonium.tar.gz" -C "${TMP_DIR}"
PACKAGE_DIR="$(find "${TMP_DIR}" -maxdepth 1 -type d -name 'sonium-*' | head -n 1)"
[[ -n "${PACKAGE_DIR}" ]] || die "Release archive did not contain a sonium package directory"

install -d "${BIN_DIR}"
install -m 0755 "${PACKAGE_DIR}/sonium-server" "${BIN_DIR}/sonium-server"
if [[ "${INSTALL_CLIENT}" == "true" ]]; then
  install -m 0755 "${PACKAGE_DIR}/sonium-client" "${BIN_DIR}/sonium-client"
fi
ok "Installed binaries to ${BIN_DIR}"

if ! id "${SONIUM_USER}" >/dev/null 2>&1; then
  useradd --system --no-create-home --shell /usr/sbin/nologin "${SONIUM_USER}"
  ok "Created system user ${SONIUM_USER}"
fi

install -d -m 0755 -o "${SONIUM_USER}" "${CONF_DIR}"

if [[ ! -p "${FIFO_PATH}" ]]; then
  rm -f "${FIFO_PATH}"
  mkfifo "${FIFO_PATH}"
fi
chown "${SONIUM_USER}" "${FIFO_PATH}" 2>/dev/null || true

if [[ ! -f "${CONF_DIR}/sonium.toml" ]]; then
  cat > "${CONF_DIR}/sonium.toml" <<EOF
[server]
bind = "0.0.0.0"
stream_port = ${STREAM_PORT}
control_port = ${CONTROL_PORT}
mdns = true
snapcast_compat = false

[[streams]]
id = "default"
display_name = "Main"
source = "${FIFO_PATH}"
codec = "opus"
buffer_ms = 1000
silence_on_idle = true

[log]
level = "info"
EOF
  chown "${SONIUM_USER}" "${CONF_DIR}/sonium.toml" 2>/dev/null || true
  ok "Wrote ${CONF_DIR}/sonium.toml"
else
  info "${CONF_DIR}/sonium.toml already exists; leaving it untouched"
fi

if [[ "${INSTALL_SERVICE}" == "true" && -d /run/systemd/system && -x "$(command -v systemctl)" ]]; then
  cat > /etc/systemd/system/sonium-server.service <<EOF
[Unit]
Description=Sonium multiroom audio server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${SONIUM_USER}
ExecStart=${BIN_DIR}/sonium-server --config ${CONF_DIR}/sonium.toml
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${CONF_DIR} /tmp

[Install]
WantedBy=multi-user.target
EOF

  systemctl daemon-reload
  systemctl enable --now sonium-server
  ok "Enabled and started sonium-server.service"
else
  info "Skipping systemd service"
fi

HOST_IP="$(hostname -I 2>/dev/null | awk '{print $1}')"
[[ -n "${HOST_IP}" ]] || HOST_IP="127.0.0.1"

cat <<EOF

Sonium is installed.

Server UI:
  http://${HOST_IP}:${CONTROL_PORT}

Feed audio into the server:
  ffmpeg -re -i song.flac -f s16le -ar 48000 -ac 2 - > ${FIFO_PATH}

Run a client:
  sonium-client --discover
  sonium-client ${HOST_IP}

EOF
