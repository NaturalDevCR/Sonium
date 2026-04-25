# Architecture Overview

Sonium is organized as a Cargo workspace of focused crates.  Each crate has
a single responsibility and can be tested in isolation.

```
┌─────────────────────────────────────────────────────────────┐
│                      sonium-server                          │
│                                                             │
│  stdin/FIFO → encoder → broadcaster → session × N clients  │
│                              │                              │
│                       control API (Fase 7)                  │
└─────────────────────────────────────────────────────────────┘
           │ TCP :1704       │ HTTP :1780
           ▼                 ▼
┌─────────────────┐   ┌──────────────────┐
│  sonium-client  │   │  browser / app   │
│                 │   └──────────────────┘
│  TCP → decoder  │
│      → sync     │
│      → speaker  │
└─────────────────┘

Shared library crates (no I/O):
  sonium-protocol  —  wire serialisation / deserialisation
  sonium-codec     —  Encoder / Decoder traits + Opus + PCM
  sonium-sync      —  clock offset estimation + jitter buffer
  sonium-common    —  SampleFormat, SoniumError, Config
```

## Data flow

### Server side

```
stdin / named pipe
      │
      ▼  raw interleaved i16 LE PCM  (configurable: 48kHz / 16-bit / stereo)
  StreamReader
      │
      ▼  Vec<i16>
  Encoder (Opus / PCM / FLAC)
      │
      ▼  encoded bytes + Timestamp
  Broadcaster  ──────────────────────────┐
      │                                  │
      │ tokio broadcast channel          │
      ▼                                  ▼
  Session[0]               ...      Session[N]
  (per-client TCP task)              (per-client TCP task)
```

### Client side

```
TCP socket
      │
      ▼  wire bytes
  MessageReader  ─────────────────────────────────┐
      │                                            │
  CodecHeader              WireChunk           Time echo
      │                        │                   │
      ▼                        ▼                   ▼
  Decoder               PcmChunk           TimeProvider
  (Opus / PCM)               │              (offset update)
                             ▼
                        SyncBuffer
                             │
                             ▼  at scheduled playout time
                          Player (CPAL)
                             │
                             ▼
                          speakers
```

## Design principles

1. **No config required** — all defaults are production-ready for a home network.
2. **One task per client** — Tokio `select!` loop, no thread-per-client.
3. **Encode once, fan out** — the broadcaster serialises each frame once and clones
   a reference-counted `Bytes` handle to every session.
4. **Clock sync isolated** — `sonium-sync` has no I/O; it is pure computation,
   making it trivially unit-testable.
5. **Snapcast-compatible by default** — protocol type IDs and payload formats
   match Snapcast v2, enabling incremental migration.
