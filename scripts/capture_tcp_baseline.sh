#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTROL_URL="${SONIUM_CONTROL_URL:-http://127.0.0.1:1711}"
PROFILE="tcp-clean"
DURATION_SECONDS=600
INTERVAL_SECONDS=5
OUT_DIR=""
NOTE=""

usage() {
  cat <<EOF
Usage: scripts/capture_tcp_baseline.sh [options]

Capture Prometheus snapshots and run metadata for a TCP baseline profile.

Options:
  --profile NAME       Profile name. Default: tcp-clean
  --duration SECONDS   Capture duration. Default: 600
  --interval SECONDS   Metrics scrape interval. Default: 5
  --control-url URL    Sonium control URL. Default: http://127.0.0.1:1711
  --out-dir DIR        Output directory. Default: run/baselines/PROFILE-TIMESTAMP
  --note TEXT          Free-form note stored in manifest.toml
  -h, --help           Show this help.

Environment:
  SONIUM_CONTROL_URL   Control URL override.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      PROFILE="$2"
      shift 2
      ;;
    --duration)
      DURATION_SECONDS="$2"
      shift 2
      ;;
    --interval)
      INTERVAL_SECONDS="$2"
      shift 2
      ;;
    --control-url)
      CONTROL_URL="$2"
      shift 2
      ;;
    --out-dir)
      OUT_DIR="$2"
      shift 2
      ;;
    --note)
      NOTE="$2"
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

case "$DURATION_SECONDS" in
  ''|*[!0-9]*)
    echo "--duration must be an integer number of seconds" >&2
    exit 1
    ;;
esac

case "$INTERVAL_SECONDS" in
  ''|*[!0-9]*)
    echo "--interval must be an integer number of seconds" >&2
    exit 1
    ;;
esac

if [[ "$DURATION_SECONDS" -lt 1 ]]; then
  echo "--duration must be at least 1 second" >&2
  exit 1
fi

if [[ "$INTERVAL_SECONDS" -lt 1 ]]; then
  echo "--interval must be at least 1 second" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

cd "$ROOT_DIR"

if [[ -z "$OUT_DIR" ]]; then
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  OUT_DIR="$ROOT_DIR/run/baselines/${PROFILE}-${stamp}"
elif [[ "$OUT_DIR" != /* ]]; then
  OUT_DIR="$ROOT_DIR/$OUT_DIR"
fi

METRICS_DIR="$OUT_DIR/metrics"
mkdir -p "$METRICS_DIR"

if ! curl -fsS "$CONTROL_URL/health" >/dev/null; then
  echo "Sonium control API is not healthy at $CONTROL_URL" >&2
  exit 1
fi

git_commit="$(git rev-parse --short HEAD 2>/dev/null || true)"
git_dirty="unknown"
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  if [[ -n "$(git status --porcelain)" ]]; then
    git_dirty="true"
  else
    git_dirty="false"
  fi
fi

start_epoch="$(date +%s)"
start_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
end_epoch=$((start_epoch + DURATION_SECONDS))

cat > "$OUT_DIR/manifest.toml" <<EOF
profile = "$PROFILE"
transport = "tcp"
control_url = "$CONTROL_URL"
duration_seconds = $DURATION_SECONDS
interval_seconds = $INTERVAL_SECONDS
start_utc = "$start_utc"
git_commit = "$git_commit"
git_dirty = "$git_dirty"
note = "$NOTE"
EOF

echo "Capturing TCP baseline:"
echo "  profile:  $PROFILE"
echo "  duration: ${DURATION_SECONDS}s"
echo "  interval: ${INTERVAL_SECONDS}s"
echo "  output:   $OUT_DIR"

while [[ "$(date +%s)" -lt "$end_epoch" ]]; do
  ts_epoch="$(date +%s)"
  ts_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  out="$METRICS_DIR/${ts_epoch}.prom"

  if curl -fsS "$CONTROL_URL/metrics" > "$out"; then
    printf '%s %s\n' "$ts_utc" "$out" >> "$OUT_DIR/samples.log"
  else
    printf '%s metrics scrape failed\n' "$ts_utc" >> "$OUT_DIR/errors.log"
    rm -f "$out"
  fi

  sleep "$INTERVAL_SECONDS"
done

end_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
{
  echo "end_utc = \"$end_utc\""
  echo "sample_count = $(find "$METRICS_DIR" -type f -name '*.prom' | wc -l | tr -d ' ')"
} >> "$OUT_DIR/manifest.toml"

if [[ -f "$ROOT_DIR/run/sonium.log" ]]; then
  cp "$ROOT_DIR/run/sonium.log" "$OUT_DIR/sonium.log"
fi

echo "Baseline capture complete: $OUT_DIR"
