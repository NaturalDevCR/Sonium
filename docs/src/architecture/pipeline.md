# Audio Pipeline

This page describes how audio travels from a source application to a speaker,
and the transformations it undergoes along the way.

## Encoding (server side)

```
Source application  (Spotify, MPD, ffmpeg, …)
         │
         │  raw PCM: interleaved i16 LE, 48 kHz, stereo
         ▼
    StreamReader         reads one configured chunk at a time
         │
         │  Vec<i16>  default 20 ms: 960 frames × 2 channels = 1920 samples
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
    ├─ ServerSettings ─► update volume / mute / buffer_ms / latency
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
    Player::write(samples)        output prefill ring → CPAL → OS audio driver → DAC → speaker
```

## Frame sizes

| Codec | Frame duration | Bytes per frame (stereo, 48 kHz) |
|-------|---------------|----------------------------------|
| Opus  | 10, 20, 40, or 60 ms | bitrate-dependent |
| PCM   | configurable, default 20 ms | 3 840 at 20 ms (960 frames × 2 ch × 2 B) |

Opus compresses roughly 8–16× compared to PCM. `chunk_ms = 20` is the default.
Smaller chunks reduce scheduling granularity, while larger chunks reduce packet
rate and overhead.

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
| Client output prefill + OS audio queue | ~120–300 ms plus driver queue |
| **Total end-to-end** | **roughly `buffer_ms` + output/device latency** |

The large default `buffer_ms` value is intentional. Sonium still needs more
real-world tuning before very low buffers are reliable. Recent clients keep a
small explicit output prefill so the audio callback is less likely to underrun
when the async network/decoder task jitters.
