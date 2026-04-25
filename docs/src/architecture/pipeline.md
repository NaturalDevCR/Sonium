# Audio Pipeline

This page describes how audio travels from a source application to a speaker,
and the transformations it undergoes along the way.

## Encoding (server side)

```
Source application  (Spotify, MPD, ffmpeg, …)
         │
         │  raw PCM: interleaved i16 LE, 48 kHz, stereo
         ▼
    StreamReader         reads exactly one frame (20 ms) at a time
         │
         │  Vec<i16>  960 frames × 2 channels = 1920 samples
         ▼
       Encoder           sonium-codec::Encoder trait
         │
         │  Opus: ~3–8 kB/s per channel (configurable quality)
         │  PCM:  ~1.5 MB/s stereo (no compression)
         ▼
    WireChunk::new(timestamp, encoded_bytes)
         │
         │  timestamp = current server wall-clock time (µs)
         ▼
    Message::WireChunk.encode()    ← header (26 bytes) + payload
         │
         │  Bytes (reference-counted, zero-copy fan-out)
         ▼
    Broadcaster::publish()
         │
         ├──► Session[0] TCP write
         ├──► Session[1] TCP write
         └──► Session[N] TCP write
```

## Decoding (client side)

```
TCP socket
    │
    │  26-byte header
    ▼
MessageHeader::from_bytes()     identifies message type + payload size
    │
    │  payload bytes
    ▼
Message::from_payload()
    │
    ├─ CodecHeader  ─► instantiate Decoder + Player
    │
    ├─ ServerSettings ─► update volume / mute / buffer_ms
    │
    ├─ WireChunk
    │     │
    │     ▼
    │  Decoder::decode()          Opus → Vec<i16>
    │     │
    │     │  playout_us = chunk.timestamp - clock_offset + latency_ms
    │     ▼
    │  SyncBuffer::push(PcmChunk { playout_us, samples })
    │
    └─ Time  ─► TimeProvider::update()   (clock offset estimation)

── audio callback ──────────────────────────────────────────────────────
    SyncBuffer::pop_ready(now_server_us)
         │
         │  PcmChunk
         ▼
    Player::write(samples)        CPAL → OS audio driver → DAC → speaker
```

## Frame sizes

| Codec | Frame duration | Bytes per frame (stereo, 48 kHz) |
|-------|---------------|----------------------------------|
| Opus  | 20 ms         | ~250–3 200 (bitrate-dependent)  |
| PCM   | 20 ms         | 3 840 (960 frames × 2 ch × 2 B) |

Opus compresses roughly 8–16× compared to PCM.  The default bitrate is 128
kbps for stereo, giving frames of about 320 bytes.

## Timestamp precision

Every `WireChunk` carries an absolute playout timestamp in the **server's
clock** with 1-µs resolution.  The client converts it to local time using:

```
local_playout_us = chunk.timestamp_us - clock_offset_us + latency_offset_us
```

`clock_offset_us` is updated every second by the `TimeProvider` and represents
the difference between the server clock and the local clock.

## Latency budget

| Stage | Typical latency |
|-------|----------------|
| Source → server encoder | < 1 ms |
| Server → network | < 1 ms (LAN) |
| Network jitter absorbed by buffer | `buffer_ms` (default 1 000 ms) |
| Decoder | < 0.5 ms |
| OS audio output queue | 5–20 ms |
| **Total end-to-end** | **~1 010–1 025 ms** |

The large `buffer_ms` value is intentional — it guarantees sync even on
congested networks.  Reduce it to `100–200 ms` for lower latency if your
network is reliable.
