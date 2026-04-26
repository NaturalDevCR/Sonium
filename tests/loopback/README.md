# Loopback Desync Test

Measures the synchronization accuracy between two Sonium clients running on the
same machine by recording their outputs side-by-side in Audacity.

## Prerequisites

| Tool | Purpose |
|------|---------|
| **BlackHole** (macOS) or **PulseAudio null-sink** (Linux) | Two virtual audio cables to capture each client's output separately |
| **ffmpeg** or **sox** | Generate a click track (sharp impulse) for easy visual alignment |
| **Audacity** | Record both virtual cables and measure time delta |

### macOS — BlackHole

Install two instances with different channel counts (they register as separate
devices):

```bash
brew install blackhole-2ch blackhole-16ch
```

After installation you should see both devices in *Audio MIDI Setup*.

### Linux — PulseAudio

```bash
pactl load-module module-null-sink sink_name=sonium_a sink_properties=device.description=SoniumA
pactl load-module module-null-sink sink_name=sonium_b sink_properties=device.description=SoniumB
```

## 1 · Generate the click track

A repeating sharp impulse makes cross-correlation trivial — you can visually spot
the offset in the waveform.

```bash
./generate_click.sh          # writes click_48k_s16le.pcm
```

Or manually:

```bash
ffmpeg -f lavfi -i "sine=frequency=1000:duration=0.005,apad=whole_dur=1" \
       -t 10 -ar 48000 -ac 2 -f s16le -acodec pcm_s16le \
       click_48k_s16le.pcm
```

This produces 10 seconds of silence with a 5 ms 1 kHz click once per second,
encoded as raw PCM (48 kHz, stereo, signed 16-bit little-endian).

## 2 · Start the server

Feed the click track into the server via stdin:

```bash
cat click_48k_s16le.pcm | cargo run --release -p sonium-server -- \
    --stream-port 1710 \
    --log debug
```

Or configure a named pipe:

```bash
mkfifo /tmp/sonium-test.fifo
cargo run --release -p sonium-server -- --stream-port 1710 &
cat click_48k_s16le.pcm > /tmp/sonium-test.fifo
```

## 3 · Start two clients on different virtual devices

```bash
# Terminal 1 — Client A
cargo run --release -p sonium-client -- 127.0.0.1 \
    --device "BlackHole 2ch" \
    --name "loopback-A" \
    --log debug

# Terminal 2 — Client B
cargo run --release -p sonium-client -- 127.0.0.1 \
    --device "BlackHole 16ch" \
    --name "loopback-B" \
    --log debug
```

On Linux, replace the `--device` values with `SoniumA` / `SoniumB`.

## 4 · Record in Audacity

1. Open Audacity.
2. Set the **recording device** to a multi-output aggregate device that includes
   both virtual cables, or record each cable in a separate Audacity instance.
3. Press Record.
4. Let it run for at least 10 seconds.
5. Stop recording.

## 5 · Measure the delta

1. Zoom into a click impulse on both tracks.
2. Use the Selection Tool to select from the peak of one click to the peak of
   the other (across the two tracks).
3. The **Selection Toolbar** at the bottom shows the duration — that is your
   desync (Δt).

### Pass criteria

| Level | Desync |
|-------|--------|
| ✅ Pass | < 50 ms |
| 🏆 Stretch goal | < 1 ms |

## Troubleshooting

- **No sound on a virtual device?**  Make sure the CPAL device name substring
  matches.  Run `sonium-client --log debug` and look for the `"Matched audio
  device"` log line.
- **Audacity shows flat silence?**  Check that the Audacity recording device is
  set to the correct virtual cable, not the built-in microphone.
- **Clicks are offset by exactly one buffer length?**  This suggests the jitter
  buffer or latency offset is misconfigured — try `--latency 0` on both clients.
