# Quick Start

This path gets one server and one client playing a test tone. It is the best way
to prove that networking, decoding, and audio output all work before wiring in a
real music source.

## 1. Install or Build

Use a release package when possible:

```bash
curl -fsSL https://github.com/NaturalDevCR/Sonium/releases/latest/download/install.sh | sudo bash
```

Or build locally:

```bash
git clone https://github.com/NaturalDevCR/Sonium
cd sonium
pnpm --dir web install
pnpm --dir web build
cargo build --release --bin sonium-server --bin sonium-client
```

The binaries are:

```text
target/release/sonium-server
target/release/sonium-client
```

## 2. Start the Server

Create a config that reads from a FIFO:

```bash
mkfifo /tmp/sonium.fifo
cat > sonium.toml <<'EOF'
[server]
bind = "0.0.0.0"
stream_port = 1710
control_port = 1711
mdns = true

[[streams]]
id = "default"
display_name = "Main"
source = "/tmp/sonium.fifo"
codec = "opus"
buffer_ms = 1000
silence_on_idle = true

[log]
level = "info"
EOF

sonium-server --config sonium.toml
```

Open the web UI:

```text
http://127.0.0.1:1711
```

## 3. Feed Audio

In a second terminal, write a test tone into the FIFO:

```bash
ffmpeg -re -f lavfi -i "sine=frequency=440" \
  -f s16le -ar 48000 -ac 2 - > /tmp/sonium.fifo
```

Any tool that can output signed 16-bit little-endian PCM at 48 kHz stereo can
feed Sonium.

## 4. Connect a Client

On the same machine:

```bash
sonium-client 127.0.0.1
```

On another machine on the same LAN:

```bash
sonium-client --discover
```

If discovery is blocked by your network:

```bash
sonium-client 192.168.1.50
```

Replace `192.168.1.50` with the server IP.

## 5. What to Check

In the web UI, you should see:

- the connected client
- the default group
- stream status and level activity when audio is flowing
- controls for volume, mute, latency, group assignment, and EQ

If the client connects but you hear nothing, check the local audio device:

```bash
sonium-client --discover --device "USB"
```

The device value is a case-insensitive substring of the output device name.
