#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_PATH="${SONIUM_CONFIG:-$ROOT_DIR/run/sonium.toml}"
EFFECTIVE_CONFIG_PATH=""
SERVER_HOST="${SONIUM_SERVER_HOST:-127.0.0.1}"
STREAM_PORT="${SONIUM_STREAM_PORT:-1710}"
LOG_LEVEL="${SONIUM_LOG:-debug}"
CLIENT_DEVICE="${SONIUM_DEVICE:-}"
WITH_CLIENT=false
SKIP_WEB_BUILD=false
TEMP_CONFIG=""

usage() {
  cat <<EOF
Usage: ./dev.sh [options]

Builds the embedded web UI and starts sonium-server with the local dev config.

Options:
  --with-client       Also start a local sonium-client.
  --skip-web-build    Reuse the existing web/dist bundle.
  --config FILE       Config file to use. Default: run/sonium.toml
  --log LEVEL         Log level. Default: debug
  --client-device DEV Audio output device substring for --with-client.
  -h, --help          Show this help.

Environment:
  SONIUM_CONFIG       Config path override.
  SONIUM_LOG          Log level override.
  SONIUM_DEVICE       Client audio output device for --with-client.
  SONIUM_SERVER_HOST  Client target host when using --with-client.
  SONIUM_STREAM_PORT  Client/server stream port. Default: 1710.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --with-client)
      WITH_CLIENT=true
      shift
      ;;
    --skip-web-build)
      SKIP_WEB_BUILD=true
      shift
      ;;
    --config)
      CONFIG_PATH="$2"
      shift 2
      ;;
    --log)
      LOG_LEVEL="$2"
      shift 2
      ;;
    --client-device)
      CLIENT_DEVICE="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

cd "$ROOT_DIR"

if [[ ! -f "$CONFIG_PATH" ]]; then
  echo "Config not found: $CONFIG_PATH" >&2
  exit 1
fi

has_configured_streams() {
  grep -Eq '^[[:space:]]*\[\[streams\]\]' "$CONFIG_PATH"
}

make_dev_config() {
  if has_configured_streams; then
    EFFECTIVE_CONFIG_PATH="$CONFIG_PATH"
    return
  fi

  TEMP_CONFIG="$(mktemp "$(dirname "$CONFIG_PATH")/sonium-dev.XXXXXX")"
  cp "$CONFIG_PATH" "$TEMP_CONFIG"

  {
    echo ""
    echo "# Added by ./dev.sh because the selected config has no active streams."
    echo "[[streams]]"
    echo 'id           = "default"'
    echo 'display_name = "Dev test tone"'
    if command -v ffmpeg >/dev/null 2>&1; then
      local ffmpeg_path
      ffmpeg_path="$(command -v ffmpeg)"
      echo "source       = \"pipe://${ffmpeg_path}?-hide_banner&-loglevel&error&-f&lavfi&-i&sine=frequency=440:sample_rate=48000&-f&s16le&-ar&48000&-ac&2&-\""
    else
      echo 'source       = "tcp-listen://127.0.0.1:4953"'
    fi
    echo 'codec        = "opus"'
    echo 'buffer_ms    = 1000'
  } >> "$TEMP_CONFIG"

  EFFECTIVE_CONFIG_PATH="$TEMP_CONFIG"
  echo "==> No active streams found in $CONFIG_PATH"
  echo "    Using temporary dev config: $EFFECTIVE_CONFIG_PATH"
  if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "    ffmpeg was not found, so the temporary stream listens on tcp://127.0.0.1:4953"
  fi
}

make_dev_config

if [[ "$SKIP_WEB_BUILD" == false ]]; then
  echo "==> Building embedded web UI"
  pnpm --dir web build
fi

if [[ "$WITH_CLIENT" == false ]]; then
  echo "==> Starting sonium-server"
  echo "    Web UI: http://127.0.0.1:1711"
  cargo run -p sonium-server -- --config "$EFFECTIVE_CONFIG_PATH" --stream-port "$STREAM_PORT" --log "$LOG_LEVEL"
  exit $?
fi

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
  fi
  if [[ -n "${CLIENT_PID:-}" ]]; then
    kill "$CLIENT_PID" 2>/dev/null || true
  fi
  if [[ -n "$TEMP_CONFIG" ]]; then
    rm -f "$TEMP_CONFIG"
  fi
}
trap cleanup EXIT INT TERM

echo "==> Starting sonium-server"
cargo run -p sonium-server -- --config "$EFFECTIVE_CONFIG_PATH" --stream-port "$STREAM_PORT" --log "$LOG_LEVEL" &
SERVER_PID=$!

echo "==> Waiting for control API"
for _ in {1..80}; do
  if curl -fsS http://127.0.0.1:1711/health >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done

if ! curl -fsS http://127.0.0.1:1711/health >/dev/null 2>&1; then
  echo "Server did not become ready on http://127.0.0.1:1711" >&2
  exit 1
fi

echo "==> Starting local sonium-client"
CLIENT_ARGS=("$SERVER_HOST" "--port" "$STREAM_PORT" "--log" "$LOG_LEVEL")
if [[ -n "$CLIENT_DEVICE" ]]; then
  CLIENT_ARGS+=("--device" "$CLIENT_DEVICE")
fi
cargo run -p sonium-client -- "${CLIENT_ARGS[@]}" &
CLIENT_PID=$!

echo "==> Sonium dev stack is running"
echo "    Web UI: http://127.0.0.1:1711"
echo "    Press Ctrl+C to stop server and client."

wait "$SERVER_PID" "$CLIENT_PID"
