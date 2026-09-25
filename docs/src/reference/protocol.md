# Binary Protocol Reference

Sonium uses a **compact binary protocol** for session setup, settings, clock
synchronization, health reports, and TCP media streaming.  TCP is the stable
default. Newer real-time media transports keep the reliable TCP control plane
and move audio frames onto a separate media plane (`rtp_udp` and experimental
`rist` today; `quic_dgram` later).

## Connection lifecycle

```
Client                                 Server
  │── TCP connect :1710 ───────────────►│
  │── Hello ──────────────────────────►│
  │◄── CodecHeader ────────────────────│
  │◄── ServerSettings ─────────────────│
  │                                    │
  │◄══ WireChunk (continuous stream) ══│
  │                                    │
  │── Time request ───────────────────►│  (every ~1 second)
  │◄── Time response ──────────────────│
  │                                    │
  │◄── GroupSync ──────────────────────│  (periodic multi-room timeline)
  │── HealthReport ───────────────────►│  (periodic observability)
  │                                    │
  │── ClientInfo (volume change) ──────►│  (on user action)
```

When `ServerSettings.transport_mode` is `rtp_udp` or `rist`, `WireChunk`
payloads may be delivered over UDP media packets while control, settings, time
sync, health reports, and fallback decisions stay on the TCP session.

## Message framing

Every message starts with a **26-byte little-endian header**:

```
Offset  Bytes  Type    Field
──────  ─────  ──────  ──────────────────────────────────────
 0       2     u16     Message type (see table below)
 2       2     u16     Message ID (sender sequence number)
 4       2     u16     Refers-to ID (0, or echoed request ID)
 6       4     i32     Sent seconds      (UNIX timestamp)
10       4     i32     Sent microseconds (0–999 999)
14       4     i32     Received seconds  (filled by receiver)
18       4     i32     Received microseconds
22       4     u32     Payload size in bytes
26       N     u8[]    Payload
```

**Byte order:** All multi-byte integers are **little-endian**.

### Message type table

| Value | Name | Direction | Payload format |
|-------|------|-----------|----------------|
| 1 | `CodecHeader` | S→C | codec name (len-prefixed string) + init data (len-prefixed blob) |
| 2 | `WireChunk` | S→C | timestamp (i32×2) + encoded audio (len-prefixed blob) |
| 3 | `ServerSettings` | S→C | JSON string (len-prefixed) |
| 4 | `Time` | C↔S | latency sec (i32) + latency usec (i32) |
| 5 | `Hello` | C→S | JSON string (len-prefixed) |
| 7 | `ClientInfo` | C→S | JSON string (len-prefixed) |
| 8 | `Error` | S→C | code (u32) + message (len-prefixed string) + detail (len-prefixed string) |
| 9 | `HealthReport` | C→S | fixed little-endian telemetry payload |
| 10 | `GroupSync` | S→C | server/group timing target |

> Type 6 is not used. Type 0 is the internal base discriminant and is not a
> valid application message.

## Payload formats

### Length-prefixed fields

Strings and blobs are prefixed with their length as a `u32` (little-endian):

```
u32  length
u8[] data[length]
```

### `Hello` (type 5)

JSON object:

```json
{
  "MAC":                      "aa:bb:cc:dd:ee:ff",
  "HostName":                 "living-room-pi",
  "Version":                  "0.1.0",
  "ClientName":               "Sonium",
  "OS":                       "linux",
  "Arch":                     "aarch64",
  "Instance":                 1,
  "ID":                       "living-room-pi-1",
  "SnapStreamProtocolVersion": 2
}
```

### `CodecHeader` (type 1)

```
u32  codec_name_length
u8[] codec_name            "opus" | "flac" | "pcm"
u32  header_data_length
u8[] header_data
```

**Opus / PCM header data** (12 bytes):

```
u32  magic = 0x4F50_5553   ("OPUS" in memory, little-endian)
u32  sample_rate           e.g. 48000
u16  bits_per_sample       e.g. 16
u16  channel_count         e.g. 2
```

**FLAC header data:** raw FLAC `STREAMINFO` metadata block.

### `WireChunk` (type 2)

```
i32  timestamp_sec         absolute playout time (server clock)
i32  timestamp_usec
u32  data_size
u8[] data[data_size]       encoded audio bytes
```

### `ServerSettings` (type 3)

JSON object:

```json
{
  "buffer_ms": 1000,
  "output_prefill_ms": 0,
  "latency":  0,
  "volume":   100,
  "muted":    false,
  "eq_bands": [],
  "eq_enabled": false,
  "observability_enabled": true,
  "transport_mode": "tcp",
  "server_udp_port": 0
}
```

`transport_mode` is an empty string or `"tcp"` for the legacy TCP media path.
Recognized config values are `"tcp"`, `"rtp_udp"`, `"rist"`, and
`"quic_dgram"`. `quic_dgram` is reserved for a future QUIC DATAGRAM
implementation.

### `Time` (type 4)

```
i32  latency_sec           client→server transit time (filled by server on echo)
i32  latency_usec          0 in the initial request
```

### `ClientInfo` (type 7)

JSON object:

```json
{
  "volume": 75,
  "muted":  false
}
```

### `Error` (type 8)

```
u32  error_code
u32  message_length
u8[] message[message_length]
u32  detail_length
u8[] detail[detail_length]
```

### `HealthReport` (type 9)

Fixed little-endian telemetry payload sent by the client when observability is
enabled. The payload is append-only for backward compatibility; older clients
may omit later fields, which decode to zero on the server.

Core fields include underruns, overruns, stale drops, buffer depth, jitter,
latency, output buffer depth, queued jitter-buffer chunks, callback starvation,
audio callback xruns, RTP packet/gap/decode/concealment counters, drift
drop/dup counters, ARQ NACK/retransmit/FEC counters, clock/group/total offset,
output latency, playout error p50/p95/p99, callback duration p99, group sync
error, commanded/applied resample ppm, and combined ARQ/FEC recovery rate.

### `GroupSync` (type 10)

```text
i64  server_now_us      server timestamp in microseconds
i64  group_offset_us    shared group offset target
i32  rate_ppm           playback-rate correction in parts per million
f32  source_quality     0.0-1.0 confidence in the sync source
```

For compatibility, a 20-byte payload without `source_quality` is accepted and
decodes with `source_quality = 0.0`.

## Media transport modes

| Mode | Status | Notes |
|------|--------|-------|
| `tcp` | Stable default | Control and media share the ordered TCP connection. |
| `rtp_udp` | Implemented, validating | UDP media plane using RTP-style packets; avoids TCP head-of-line blocking. |
| `rist` | Experimental | Sonium-native ARQ/FEC/NACK over UDP inspired by RIST concepts; not libRIST wire-compatible. |
| `quic_dgram` | Reserved | Planned encrypted datagram transport for routed/WAN deployments. |

For UDP media modes, the server advertises `server_udp_port` in
`ServerSettings`. TCP remains the control and fallback path.

## Validation rules

Implementations **must** reject messages that violate these constraints:

| Rule | Value |
|------|-------|
| Maximum payload size | 1 000 000 bytes |
| Maximum message type | 10 |
| Codec name length | ≤ 64 bytes |
| JSON fields | UTF-8 encoded |

## Snapcast compatibility

Sonium offers a migration-oriented Snapcast discovery mode. Setting matching
ports (`stream_port = 1704`, `control_port = 1780`) and
`snapcast_compat = true` advertises the legacy mDNS service, but does **not**
claim binary or drop-in compatibility with every Snapcast client or version.
Test each client/server combination before relying on it in a migration.

- Where a peer speaks the supported compatibility subset,
  `SnapStreamProtocolVersion` in `Hello` must be `2`.
- The server ignores unknown JSON fields in `Hello`, `ServerSettings`, and
  `ClientInfo` as a best-effort forward-compatibility measure, not as proof of
  complete interoperability.
