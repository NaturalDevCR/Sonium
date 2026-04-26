# Binary Protocol Reference

Sonium uses a **compact binary protocol** over TCP for audio streaming and
clock synchronization.  This page is a complete specification suitable for
implementing a third-party client or server.

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
  │── ClientInfo (volume change) ──────►│  (on user action)
```

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

> Type 6 is not used.  Parsers must treat it as an error.

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
  "bufferMs": 1000,
  "latency":  0,
  "volume":   100,
  "muted":    false
}
```

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

## Validation rules

Implementations **must** reject messages that violate these constraints:

| Rule | Value |
|------|-------|
| Maximum payload size | 1 000 000 bytes |
| Maximum message type | 8 |
| Codec name length | ≤ 64 bytes |
| JSON fields | UTF-8 encoded |

## Snapcast compatibility

Sonium's wire encoding is binary-compatible with the Snapcast v2 protocol.
When configured with matching ports (`stream_port = 1704`, `control_port = 1780`)
and `snapcast_compat = true`, legacy Snapcast clients can connect to a Sonium
server without modification — useful as a migration path.

- `SnapStreamProtocolVersion` in `Hello` must be `2`.
- The server ignores unknown JSON fields in `Hello`, `ServerSettings`, and
  `ClientInfo` — forward-compatible.
