#!/usr/bin/env bash
# generate_click.sh — Create a repeating click track for loopback desync testing.
#
# Output: click_48k_s16le.pcm (raw PCM, 48 kHz, stereo, s16le)
#
# Each second contains a sharp 5 ms 1 kHz sine pulse followed by silence,
# repeated for DURATION seconds.  The impulse is easy to spot visually in
# Audacity and works well for cross-correlation measurements.

set -euo pipefail

DURATION=${1:-10}          # seconds (default: 10)
RATE=48000
CHANNELS=2
OUTPUT="click_48k_s16le.pcm"

if ! command -v ffmpeg &>/dev/null; then
    echo "ERROR: ffmpeg is required.  Install it with: brew install ffmpeg" >&2
    exit 1
fi

echo "Generating ${DURATION}s click track at ${RATE} Hz, ${CHANNELS}ch, s16le..."

ffmpeg -y -hide_banner -loglevel warning \
    -f lavfi -i "sine=frequency=1000:duration=0.005,apad=whole_dur=1" \
    -t "${DURATION}" \
    -ar "${RATE}" -ac "${CHANNELS}" \
    -f s16le -acodec pcm_s16le \
    "${OUTPUT}"

SIZE=$(stat -f%z "${OUTPUT}" 2>/dev/null || stat -c%s "${OUTPUT}")
SAMPLES=$((SIZE / 2 / CHANNELS))
echo "Written: ${OUTPUT} (${SIZE} bytes, ~${SAMPLES} samples per channel)"
echo ""
echo "Usage:"
echo "  cat ${OUTPUT} | cargo run --release -p sonium-server"
