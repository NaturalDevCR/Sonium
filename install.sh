#!/usr/bin/env bash
# Sonium installer for Linux hosts.
set -euo pipefail

REPO="${SONIUM_REPO:-NaturalDevCR/Sonium}"
VERSION="${SONIUM_VERSION:-latest}"
PREFIX="${PREFIX:-/usr/local}"
INSTALL_SERVICE=true
INSTALL_SERVER=true
INSTALL_CLIENT=true
UNINSTALL=false
SONIUM_USER="${SONIUM_USER:-sonium}"
CONF_DIR="${SONIUM_CONFIG_DIR:-/etc/sonium}"
FIFO_PATH="${SONIUM_FIFO:-/tmp/sonium.fifo}"
STREAM_PORT="${SONIUM_STREAM_PORT:-1710}"
CONTROL_PORT="${SONIUM_CONTROL_PORT:-1711}"

# Detect if we can be interactive
if [[ -t 0 ]]; then
  INTERACTIVE=true
  TTY_PATH="/dev/stdin"
elif [[ -c /dev/tty ]]; then
  INTERACTIVE=true
  TTY_PATH="/dev/tty"
else
  INTERACTIVE=false
  TTY_PATH="/dev/null"
fi

SCRIPT_VERSION="v0.1.17"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix) PREFIX="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --repo) REPO="$2"; shift 2 ;;
    --no-service) INSTALL_SERVICE=false; shift ;;
    --server-only) INSTALL_CLIENT=false; shift ;;
    --client-only) INSTALL_SERVER=false; shift ;;
    --uninstall) UNINSTALL=true; shift ;;
    -h|--help)
      cat <<EOF
Sonium Linux installer (${SCRIPT_VERSION})

Usage:
  curl -fsSL https://github.com/${REPO}/releases/latest/download/install.sh | sudo bash
  sudo bash install.sh [--version v0.1.0] [--prefix /usr/local] [--no-service]

Options:
  --version TAG    Release tag to install, for example v0.1.0
  --repo OWNER/REPO
  --prefix DIR    Install into DIR/bin
  --no-service    Install binaries and config only
  --server-only   Install only sonium-server
  --client-only   Install only sonium-client
  --uninstall     Remove Sonium from this system
EOF
      exit 0
      ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

info() { printf '  \033[34m->\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!!\033[0m %s\n' "$*"; }
ok() { printf '  \033[32mOK\033[0m %s\n' "$*"; }
die() { printf '  \033[31mERR\033[0m %s\n' "$*" >&2; exit 1; }

run_as_user() {
  local user="$1"
  shift

  if [[ "${EUID}" -ne 0 ]]; then
    "$@"
    return
  fi

  if command -v runuser >/dev/null 2>&1; then
    runuser -u "${user}" -- "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo -u "${user}" "$@"
  elif command -v su >/dev/null 2>&1; then
    su -s /bin/sh -c "$(printf '%q ' "$@")" "${user}"
  else
    return 1
  fi
}

if [[ "$(uname)" == "Darwin" && "${UNINSTALL}" == "false" ]]; then
  warn "You are running this on macOS. We recommend using the native Sonium Desktop Agent instead."
  warn "The Desktop Agent provides a premium tray-based GUI and automatic audio device management."
  warn "Download it here: https://github.com/${REPO}/releases/latest"
  echo
  if [[ "${INTERACTIVE}" == "true" ]]; then
    read -p "  -> Continue with CLI installation anyway? [y/N] " -n 1 -r < "$TTY_PATH"
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
      exit 0
    fi
  fi
fi

install_pkg() {
  local pkg="$1"
  if ! dpkg -l "$pkg" >/dev/null 2>&1; then
    info "Installing system dependency: $pkg"
    apt-get update -y >/dev/null
    apt-get install -y "$pkg" >/dev/null || warn "Could not install $pkg. You may need to install it manually."
  fi
}

check_dependencies() {
  if [[ "${INSTALL_SERVER}" == "true" ]]; then
    if ! command -v ffmpeg >/dev/null 2>&1; then
      warn "ffmpeg is recommended to feed audio streams to sonium-server."
      if [[ "${INTERACTIVE}" == "true" ]]; then
        read -p "  -> Install ffmpeg now? [Y/n] " -n 1 -r < "$TTY_PATH"
        echo
        if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
          install_pkg ffmpeg
        fi
      fi
    fi
  fi

  if [[ "${INSTALL_CLIENT}" == "true" ]]; then
    if ! dpkg -l libasound2 >/dev/null 2>&1; then
      info "sonium-client requires libasound2 for audio output."
      if [[ "${INTERACTIVE}" == "true" ]]; then
        read -p "  -> Install libasound2? [Y/n] " -n 1 -r < "$TTY_PATH"
        echo
        if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
          install_pkg libasound2
        fi
      fi
    fi
  fi
}

do_uninstall() {
  echo
  echo "Sonium Uninstaller (${SCRIPT_VERSION})"
  echo
  
  if [[ "${INTERACTIVE}" == "true" ]]; then
    read -p "  -> This will remove Sonium from your system. Continue? [y/N] " -n 1 -r < "$TTY_PATH"
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
      die "Aborted."
    fi
  fi
  if [[ "$(uname)" == "Darwin" ]]; then
    info "Cleaning up macOS background services..."
    for plist in ~/Library/LaunchAgents/com.sonium.client.*.plist; do
      if [[ -f "$plist" ]]; then
        launchctl unload "$plist" >/dev/null 2>&1 || true
        rm -f "$plist"
        ok "Removed $(basename "$plist")"
      fi
    done
    rm -rf ~/.sonium
    ok "Removed ~/.sonium configuration"
  fi

  if [[ -d /run/systemd/system ]] && systemctl is-active --quiet sonium-server; then
    info "Stopping sonium-server.service"
    systemctl stop sonium-server
  fi

  if [[ -f /etc/systemd/system/sonium-server.service ]]; then
    systemctl disable sonium-server >/dev/null 2>&1 || true
    rm -f /etc/systemd/system/sonium-server.service
    systemctl daemon-reload
    ok "Removed systemd service"
  fi

  rm -f "${BIN_DIR}/sonium-server" "${BIN_DIR}/sonium-client"
  ok "Removed binaries from ${BIN_DIR}"

  if id "${SONIUM_USER}" >/dev/null 2>&1; then
    userdel "${SONIUM_USER}" 2>/dev/null || true
    ok "Removed system user ${SONIUM_USER}"
  fi

  rm -f "${FIFO_PATH}"

  if [[ -d "${CONF_DIR}" ]]; then
    if [[ "${INTERACTIVE}" == "true" ]]; then
      read -p "  -> Delete configuration directory ${CONF_DIR}? [y/N] " -n 1 -r < "$TTY_PATH"
      echo
      if [[ $REPLY =~ ^[Yy]$ ]]; then
        rm -rf "${CONF_DIR}"
        ok "Deleted ${CONF_DIR}"
      else
        info "Kept ${CONF_DIR}"
      fi
    fi
  fi

  if [[ "${INTERACTIVE}" == "true" ]]; then
    read -p "  -> Uninstall system dependencies (ffmpeg, libasound2)? [y/N] " -n 1 -r < "$TTY_PATH"
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
      info "Removing system dependencies..."
      apt-get remove -y ffmpeg libasound2 >/dev/null 2>&1 || true
      info "To safely remove unused shared libraries, run: apt-get autoremove"
    fi
  fi

  echo
  ok "Sonium has been uninstalled."
  exit 0
}

if [[ "$(uname -s)" != "Linux" ]]; then
  die "This installer is for Linux. Download macOS/Windows packages from GitHub Releases."
fi

if [[ "${EUID}" -ne 0 ]]; then
  die "Run as root, for example: curl ... | sudo bash"
fi

BIN_DIR="${PREFIX}/bin"

if [[ "${UNINSTALL}" == "true" ]]; then
  do_uninstall
fi

if [[ "${INTERACTIVE}" == "true" ]]; then
  echo
  echo "Sonium Installer (${SCRIPT_VERSION})"
  echo "Select components to install:"
  echo "  1) Full (Server + Client) [Default]"
  echo "  2) Server only"
  echo "  3) Client only"
  echo "  4) Uninstall"
  while true; do
    if ! read -p "Selection [1-4] (default 1): " -n 1 -r < "$TTY_PATH"; then
      break # EOF
    fi
    echo
    case "$REPLY" in
      1|"") break ;;
      2) INSTALL_CLIENT=false; break ;;
      3) INSTALL_SERVER=false; break ;;
      4) do_uninstall; break ;;
      *) echo "Invalid option." ;;
    esac
  done
fi

check_dependencies

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
echo "Sonium installer (${SCRIPT_VERSION})"
echo
info "Downloading ${PACKAGE_NAME}"
curl -fsSL "${BASE_URL}/${PACKAGE_NAME}" -o "${TMP_DIR}/sonium.tar.gz" \
  || die "Could not download ${BASE_URL}/${PACKAGE_NAME}"

tar -xzf "${TMP_DIR}/sonium.tar.gz" -C "${TMP_DIR}"
PACKAGE_DIR="$(find "${TMP_DIR}" -maxdepth 1 -type d -name 'sonium-*' | head -n 1)"
[[ -n "${PACKAGE_DIR}" ]] || die "Release archive did not contain a sonium package directory"

install -d "${BIN_DIR}"
if [[ "${INSTALL_SERVER}" == "true" ]]; then
  install -m 0755 "${PACKAGE_DIR}/sonium-server" "${BIN_DIR}/sonium-server"
fi
if [[ "${INSTALL_CLIENT}" == "true" ]]; then
  install -m 0755 "${PACKAGE_DIR}/sonium-client" "${BIN_DIR}/sonium-client"
fi
ok "Installed binaries to ${BIN_DIR}"

if [[ "${INSTALL_SERVER}" == "true" ]]; then
  if ! id "${SONIUM_USER}" >/dev/null 2>&1; then
    useradd --system --no-create-home --shell /usr/sbin/nologin "${SONIUM_USER}"
    ok "Created system user ${SONIUM_USER}"
  fi

  mkdir -p "${CONF_DIR}"
  chown "${SONIUM_USER}" "${CONF_DIR}"
  chmod 0755 "${CONF_DIR}"

  # Pre-initialize admin account if users.json doesn't exist
  if [[ ! -f "${CONF_DIR}/users.json" ]]; then
    set +o pipefail
    GEN_PASS=$(tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 16)
    set -o pipefail
    info "Initializing admin account..."
  else
    info "Existing user database found in ${CONF_DIR}/users.json; preserving it."
  fi

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

  if [[ -n "${GEN_PASS:-}" ]]; then
    if run_as_user "${SONIUM_USER}" "${BIN_DIR}/sonium-server" --config "${CONF_DIR}/sonium.toml" --init-admin "${GEN_PASS}" >/dev/null 2>&1; then
      ok "Initialized default admin credentials"
    else
      warn "Could not initialize default admin credentials automatically"
    fi
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
    systemctl enable sonium-server
    
    info "Stopping any lingering sonium-server processes..."
    pkill -x sonium-server 2>/dev/null || true
    
    systemctl restart sonium-server
    ok "Enabled and restarted sonium-server.service"
  else
    info "Skipping systemd service"
  fi
fi

HOST_IP="$(hostname -I 2>/dev/null | awk '{print $1}')"
[[ -n "${HOST_IP}" ]] || HOST_IP="127.0.0.1"

cat <<EOF

Sonium is installed.
EOF

if [[ "${INSTALL_SERVER}" == "true" ]]; then
  cat <<EOF

Server UI:
  http://${HOST_IP}:${CONTROL_PORT}
EOF

  if [[ -n "${GEN_PASS:-}" ]]; then
    cat <<EOF

Admin credentials:
  Username: admin
  Password: ${GEN_PASS}
(You will be asked to change this on your first login)
EOF
  else
    cat <<EOF

Admin credentials:
  Use your existing credentials.
  (Default was admin/admin in previous versions)
EOF
  fi

  cat <<EOF

Feed audio into the server:
  ffmpeg -re -i song.flac -f s16le -ar 48000 -ac 2 - > ${FIFO_PATH}
EOF
fi

if [[ "${INSTALL_CLIENT}" == "true" ]]; then
  cat <<EOF

Run a client:
  sonium-client --discover
  sonium-client ${HOST_IP}
EOF
fi

echo
