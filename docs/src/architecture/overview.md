# Architecture Overview

Sonium is organized as a Cargo workspace of focused crates.  Each crate has
a single responsibility and can be tested in isolation.

```
┌─────────────────────────────────────────────────────────────┐
│                      sonium-server                          │
│                                                             │
│  source readers → encoder → broadcaster → session × N      │
│                              │                              │
│                       control API + embedded UI             │
└─────────────────────────────────────────────────────────────┘
           │ TCP :1710       │ UDP media       │ HTTP :1711
           ▼                 ▼
┌─────────────────┐   ┌──────────────────┐
│  sonium-client  │   │  browser / app   │
│                 │   └──────────────────┘
│  transport → decoder
│      → sync     │
│      → speaker  │
└─────────────────┘

Shared library crates (no I/O):
  sonium-protocol  —  wire serialisation / deserialisation
  sonium-codec     —  Encoder / Decoder traits + Opus + PCM
  sonium-sync      —  clock offset estimation + jitter buffer
  sonium-transport —  TCP/RTP/ARQ sender abstractions
  sonium-common    —  SampleFormat, SoniumError, Config
```

## Data flow

### Server side

```
stdin / FIFO / file / TCP / pipe:// / meta stream
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
  control TCP + MediaSender          control TCP + MediaSender
```

### Client side

```
TCP control socket + optional UDP media socket
      │
      ▼  wire bytes
  MessageReader  ─────────────────────────────────┐
      │                                            │
  CodecHeader              WireChunk           Time / GroupSync
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

1. **No config required** — the server can start with defaults for local
   testing, while production readiness is still a project goal.
2. **One task per client** — Tokio `select!` loop, no thread-per-client.
3. **Encode once, fan out** — the broadcaster serialises each frame once and clones
   a reference-counted `Bytes` handle to every session.
4. **Clock sync isolated** — `sonium-sync` has no I/O; it is pure computation,
   making it trivially unit-testable.
5. **Transport migration-friendly** — TCP stays as the stable compatibility path
   while RTP/UDP, ARQ/FEC, and future QUIC DATAGRAM evolve behind the
   `MediaSender` abstraction.
6. **Migration-friendly** — optional Snapcast discovery mode can assist a
   migration; verify each client/version because full drop-in compatibility is
   not claimed (see configuration docs).
