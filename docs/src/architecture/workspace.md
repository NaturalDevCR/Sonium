# Workspace Layout

```
sonium/
├── Cargo.toml                  ← workspace root, shared dependency versions
│
├── crates/
│   ├── common/                 ← sonium-common
│   │   └── src/
│   │       ├── sample_format.rs   SampleFormat — PCM stream descriptor
│   │       ├── error.rs           SoniumError + Result alias
│   │       └── config.rs          ServerConfig / ClientConfig (zero-conf defaults)
│   │
│   ├── protocol/               ← sonium-protocol
│   │   └── src/
│   │       ├── header.rs          26-byte MessageHeader + MessageType + Timestamp
│   │       ├── wire.rs            WireRead / WireWrite — little-endian helpers
│   │       └── messages/
│   │           ├── hello.rs       Client → Server greeting
│   │           ├── server_settings.rs  Server → Client volume/mute/buffer
│   │           ├── client_info.rs      Client → Server volume/mute update
│   │           ├── codec_header.rs     Server → Client codec init
│   │           ├── wire_chunk.rs       Server → Client audio frame
│   │           ├── time.rs             Clock sync request/response
│   │           └── error.rs            Server → Client error notification
│   │
│   ├── codec/                  ← sonium-codec
│   │   └── src/
│   │       ├── traits.rs          Encoder / Decoder traits
│   │       ├── pcm.rs             Raw PCM passthrough
│   │       └── opus.rs            Opus encoder/decoder (via audiopus)
│   │
│   └── sync/                   ← sonium-sync
│       └── src/
│           ├── time_provider.rs   NTP-like offset estimator (200-sample median)
│           └── buffer.rs          SyncBuffer — jitter buffer, playout scheduling
│
├── server/                     ← sonium-server binary
│   └── src/
│       ├── main.rs                Tokio runtime, TCP listener, config loading
│       ├── broadcaster.rs         tokio broadcast channel fan-out
│       ├── session.rs             Per-client async task (Hello → audio → sync)
│       └── streamreader.rs        stdin/FIFO → encode → broadcast
│
├── client/                     ← sonium-client binary
│   └── src/
│       ├── main.rs                Tokio runtime, config, auto-reconnect loop
│       ├── controller.rs          TCP connect → Hello → decode → sync → play
│       ├── decoder.rs             Wraps sonium-codec for runtime codec selection
│       └── player.rs              CPAL output abstraction (stub until Fase 4)
│
├── docs/                       ← This documentation (mdBook → GitHub Pages)
│
└── .github/
    └── workflows/
        ├── ci.yml              ← cargo test + clippy on every PR
        └── docs.yml            ← build + deploy mdBook on push to main
```

## Dependency graph

```
sonium-server ─┬─► sonium-common
               ├─► sonium-protocol
               ├─► sonium-codec ──► sonium-protocol
               └─► sonium-sync  ──► sonium-common

sonium-client ─┬─► sonium-common
               ├─► sonium-protocol
               ├─► sonium-codec
               └─► sonium-sync
```

`sonium-common` and `sonium-protocol` have no Sonium-internal dependencies —
they are the foundation everything else builds on.

## Crate responsibilities

| Crate | Depends on | Has I/O? |
|---|---|---|
| `sonium-common` | external only | No |
| `sonium-protocol` | `sonium-common` | No |
| `sonium-codec` | `sonium-common`, `sonium-protocol` | No |
| `sonium-sync` | `sonium-common` | No |
| `sonium-server` | all crates | Yes (TCP, stdin) |
| `sonium-client` | all crates | Yes (TCP, audio) |

The "No I/O" crates are fully unit-testable without network or audio hardware.
