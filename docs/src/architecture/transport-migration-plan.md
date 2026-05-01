# Sonium Real-Time-First Transport & Playout Architecture Plan

## Purpose

This document defines an improved technical roadmap for evolving **Sonium** into a modern, robust, open-source, self-hosted, distributed multiroom audio system.

The goal is **not** to become a “Dante killer.” Dante is a useful reference point for stability, clock discipline, network behavior, and professional-grade reliability, but Sonium’s target is different:

> Sonium should become the best-designed, most robust, modern, self-hosted open-source distributed multiroom audio system.

Low-latency real-time behavior is important, not because Sonium must always operate at extremely low buffers, but because a system that remains stable at lower buffers becomes even more reliable when larger buffers are used. In other words:

> Real-time resilience at low buffer sizes makes higher-buffer operation more stable, predictable, and forgiving.

This plan migrates Sonium from a TCP-first transport model toward a **real-time-first architecture** based on:

- `RTP/UDP` for resilient LAN media transport.
- Shared timestamped media framing across transports.
- Adaptive jitter buffering and late-drop policy.
- Client-side playout discipline and clock drift correction.
- RTCP-style receiver feedback and transport health reporting.
- `QUIC DATAGRAM` for encrypted, internet-friendlier delivery.
- `TCP` retained as a compatibility and fallback mode.
- Future readiness for PTP/hardware-assisted clocking without depending on it initially.

---

## Core philosophy

Sonium should not merely “send audio packets.”

Sonium should manage a distributed audio system where every endpoint has:

- A clear media timeline.
- A known playout target.
- Measurable jitter, loss, drift, and underrun behavior.
- A bounded and observable jitter buffer.
- A predictable failure and fallback policy.
- Operator-visible health information.

The transport protocol matters, but the real architecture is:

```text
timestamped media
+ reliable control
+ adaptive playout
+ clock drift correction
+ loss-tolerant behavior
+ strong telemetry
+ safe fallback
```

---

## Why this migration is needed

Current dropouts at lower buffers are more consistent with real-time timing issues than with “not enough buffering” alone.

Likely dominant factors include:

- TCP head-of-line blocking under segment loss or retransmission.
- Scheduler jitter between network, decoder, and audio callback tasks.
- Audio device clock drift across clients.
- Lack of explicit media timestamping and playout timing policy.
- Insufficient transport-level telemetry.
- Limited visibility into queue depth, late packets, callback starvation, and underruns.

Increasing buffer size can hide many of these problems, but it does not fix the underlying architecture. The objective is to build a stronger real-time foundation so that larger buffers become an additional stability margin instead of the only defense.

---

## Scope and non-goals

## In scope

- Introduce a transport abstraction usable by server and client.
- Add `RTP/UDP` as the primary LAN media transport.
- Add RTCP-style feedback and receiver reports.
- Add adaptive jitter buffering and late-drop policy.
- Add clock drift estimation and correction.
- Add DSCP/QoS configuration for managed LANs.
- Add `QUIC DATAGRAM` for encrypted datagram-based delivery.
- Keep TCP as a compatibility fallback.
- Add metrics, health reporting, and controlled rollout toggles.
- Prepare the design for future PTP/hardware timestamping.

## Out of scope for the initial migration

- Replacing the complete codec pipeline.
- Full AES67 compatibility.
- Full Dante-like device ecosystem, routing, or certification.
- Mandatory PTP dependency.
- Multicast as a first implementation target.
- Hardware timestamping as a first implementation target.

---

## Architecture target state

## Control plane vs media plane

Sonium should separate reliable session/control behavior from media delivery.

### Control plane

Used for:

- Client registration.
- Authentication.
- Stream selection.
- Volume/group control.
- Capability negotiation.
- Transport selection.
- Session metadata.
- Health reports.
- Fallback decisions.
- Future clock configuration.

The control plane should remain reliable and ordered.

Initial implementation may keep the existing TCP control protocol. Later, control may move to a reliable QUIC stream if useful.

### Media plane

Used for:

- Audio packet delivery.
- Timestamped media frames.
- Packet sequence tracking.
- Low-latency playout.
- Loss-tolerant behavior.
- Late packet dropping.
- Jitter buffering.

The media plane should not require reliable ordered delivery. It should favor continuity over perfect packet delivery.

---

## Supported transport modes

## `tcp`

Reliable ordered fallback.

### Strengths

- Maximum compatibility.
- Simple firewall/NAT behavior.
- Easy debugging.
- Good fallback when UDP/QUIC is unavailable.

### Weaknesses

- Head-of-line blocking.
- Retransmission stalls.
- Poor behavior under packet loss when low latency is required.
- Less appropriate for real-time audio playout.

### Intended use

```text
Compatibility mode
Debug mode
Fallback mode
Conservative deployments
```

---

## `rtp_udp`

Primary LAN real-time media transport.

### Strengths

- No TCP head-of-line blocking.
- Better behavior under small packet loss.
- Natural fit for timestamped audio.
- Easier low-latency operation.
- Compatible with RTCP-style feedback.

### Weaknesses

- Requires UDP reachability.
- Requires explicit jitter/loss handling.
- NAT traversal is not automatic.
- Needs careful pacing and buffer control.

### Intended use

```text
Default LAN profile
Managed local networks
Wired deployments
Low-latency or medium-latency multiroom audio
```

---

## `quic_dgram`

Encrypted datagram media transport for internet-friendlier deployments.

### Strengths

- Encrypted by design.
- Better NAT/firewall behavior than raw UDP in some environments.
- Connection semantics.
- Supports unreliable datagrams.
- Can carry reliable control streams and unreliable media together.

### Weaknesses

- More complex than raw UDP.
- QUIC stack behavior must be understood and tuned.
- Congestion control and connection lifecycle matter.
- Not necessarily better than RTP/UDP on clean LAN.

### Intended use

```text
WAN profile
Routed networks
Encrypted media transport
Remote clients
Future internet-facing deployments
```

---

## Unifying media semantics

All transports must expose a common media packet model.

At the playout layer, the system should not care whether the packet arrived over TCP, RTP/UDP, or QUIC DATAGRAM.

Each media packet should expose:

- Stream identifier.
- Packet sequence number.
- Media timestamp.
- Codec/frame metadata where needed.
- Payload bytes.
- Optional absolute capture or playout reference.
- Transport arrival timestamp for diagnostics.

This enables shared logic for:

- Jitter buffering.
- Reordering.
- Late packet dropping.
- Packet loss concealment.
- Clock drift measurement.
- Metrics and health scoring.

---

## Recommended media packet model

Use real RTP semantics where possible instead of inventing a fully custom “RTP-like” protocol.

## RTP header

Use standard RTP-style fields:

```text
version
payload type
sequence number
timestamp
SSRC
marker bit
```

## Sonium session metadata

Keep mostly-static metadata out of every packet when possible.

Session metadata should include:

```text
stream_id
codec_id
sample_rate
channels
frame_duration_us
nominal_chunk_ms
clock_mode
transport_profile
```

## Per-packet media data

Each packet should carry only what is needed for playout:

```text
RTP sequence number
RTP timestamp
payload bytes
minimal flags if needed
```

## Optional Sonium RTP extension

If Sonium needs extra information, use an RTP extension or compact custom payload prefix rather than replacing RTP entirely.

Optional fields:

```text
media_frame_index
absolute_capture_time_ns
intended_playout_time_ns
stream_generation_id
codec_flags
```

Avoid bloating every packet unless the field is needed for actual playout or diagnostics.

---

## Clocking and playout model

This is one of the most important parts of the system.

Transport alone does not make a multiroom system stable. Sonium needs a clear model for:

```text
server media timeline
client receive timeline
client audio device timeline
target playout timeline
clock drift correction
```

## Required concepts

### Server media timestamp

The timestamp assigned by the server to each audio frame or packet.

### Client receive timestamp

The local monotonic time when the client receives the packet.

### Target playout timestamp

The time when the client should begin playing the corresponding frame.

### Audio device clock position

The actual progress of the output audio device. This may drift from the server timeline.

### Clock drift estimate

The estimated difference between the server media timeline and the client’s audio device clock, expressed in parts per million.

Example:

```text
client=lobby-speaker-01 clock_drift_ppm=-18.4
```

## Clock reference abstraction

Prepare the system for future PTP or hardware timestamping without requiring it now.

Suggested model:

```rust
enum ClockReference {
    LocalMonotonic,
    ServerWallClockNtp,
    PtpDomain { domain: u8 },
    HardwareTimestamped,
}
```

Initial implementation can use `LocalMonotonic` plus server timestamp mapping. Future versions can improve the reference source.

---

## Packet loss and concealment policy

Loss handling should be codec-aware.

## PCM

For PCM streams:

```text
short loss:
- repeat last very short frame, or
- fade/crossfade into silence

longer loss:
- silence fill

always:
- avoid clicks with tiny fade ramps
- never block callback waiting for missing packets
```

## Opus

For Opus streams:

```text
use decoder-native PLC where available
prefer codec-level concealment over custom concealment
```

Opus should eventually become a first-class profile for:

- Wi-Fi clients.
- Lower bandwidth.
- Higher packet loss environments.
- WAN/QUIC profiles.

## Future FEC

Do not implement FEC in the first transport migration.

Add it later as an optional resilience profile:

```text
simple parity FEC
codec-aware FEC
configurable overhead: 5%, 10%, 20%
```

FEC should only be introduced after packet timing, drift correction, jitter buffer behavior, and metrics are stable.

---

## QoS and network behavior

To approach professional stability, Sonium should expose network-quality controls.

## DSCP/QoS

Add configurable DSCP marking for media packets.

Suggested configuration:

```toml
[network.qos]
enabled = true
media_dscp = "EF"
control_dscp = "AF21"
```

Implementation should:

- Attempt to set DSCP/socket priority.
- Log whether the operation succeeded.
- Expose the effective QoS mode in diagnostics.
- Document switch/router behavior caveats.

## Wi-Fi awareness

Wi-Fi should be supported but treated honestly.

Sonium should report warnings such as:

```text
High jitter detected
Burst loss detected
Wi-Fi client may need larger buffer
Client should use resilient profile
```

Do not pretend Wi-Fi behaves like wired Ethernet.

## Unicast vs multicast

Initial implementation should be unicast.

```text
server -> client A
server -> client B
server -> client C
```

Multicast should remain a future option.

Future multicast profile:

```text
server -> multicast stream group
clients subscribe
IGMP snooping required
wired LAN recommended
```

Do not implement multicast in the first RTP/UDP migration. Design stream/session IDs so multicast can be added later.

---

# Phase plan

---

## Progress log

Current stage:

```text
Phase 0 - Baseline and observability hardening
```

Completed in this pass:

- Confirmed the current TCP path already has client health reports for underruns, overruns, stale drops, jitter-buffer depth, jitter, and clock offset.
- Added a shared `AudioHealthState` model in the protocol crate with states for `buffering`, `stable`, `degraded`, `recovering`, `underrun`, `fallback`, and `offline`.
- Added server-side health transition logging for the current `tcp` transport.
- Added Prometheus metrics tagged with `transport="tcp"` for client health reports, state, buffer depth, jitter, underruns, stale drops, overruns, and clock offset.
- Extended health reports with output/player queue depth, jitter-buffer chunk count, and target playout latency while keeping decode compatibility for older health payloads.
- Added audio callback starvation and backend xrun/error telemetry from the client `Player`, exposed in Prometheus and the admin observability table.
- Added [TCP Baseline Profiles](./tcp-baseline-profiles.md) with clean, jitter, loss, burst-loss, and CPU-stress runs for Phase 0 comparison.
- Added `scripts/capture_tcp_baseline.sh` to capture metrics snapshots and run metadata into `run/baselines/...`.
- Captured the first live clean TCP smoke baseline on 2026-05-01 using Sonium v0.1.50:
  `tcp-clean-20260501T055634Z`, 600 seconds, 120 samples, one connected client, stream playing, final health state `stable`.
- The smoke baseline used the existing higher buffer configuration rather than the canonical 500 ms comparison target, but final counters were healthy:
  `underruns=0`, `stale_drops=0`, `overruns=0`, `callback_starvations=0`, `audio_callback_xruns=0`, `jitter_ms=0`.
- Retested clean TCP with global `buffer_ms=500`, no stream buffer override, `chunk_ms=20`, and `auto_buffer=false`.
  The client reached `target_playout_latency_ms=500`, but the run was not stable enough for the canonical baseline:
  over a 60 second observation window, `underruns` increased from 174 to 234 and `stale_drops` increased from 433 to 490, while buffer depth oscillated between 20 ms and 560 ms.
- Retested clean TCP with global `buffer_ms=700`, no stream buffer override, `chunk_ms=20`, and `auto_buffer=false`.
  The client reached `target_playout_latency_ms=700`, but the run was still rejected:
  over a 60 second observation window, `underruns` increased from 5 to 12 and `stale_drops` increased from 142 to 294.
- Confirmed global `buffer_ms=1000`, no stream buffer override, `chunk_ms=20`, and `auto_buffer=false` as the first clean TCP candidate:
  target reached 1000 ms, health reported `stable`, and during the 60 second precheck `underruns=0`, `stale_drops=101`, `overruns=0`, `callback_starvations=0`, and `audio_callback_xruns=0` stayed flat.
- Captured the 600 second TCP clean run at global `buffer_ms=1000`:
  `tcp-clean-buffer-1000ms-20260501T065459Z`, 120 samples, final health state `stable`, stream playing.
  Final counters were mostly healthy, with small increases from precheck to the final sample:
  `underruns=2`, `stale_drops=129`, `overruns=0`, `callback_starvations=0`, `audio_callback_xruns=0`, `jitter_ms=0`.
- Captured the accepted 600 second clean TCP baseline at global `buffer_ms=1200`:
  `tcp-clean-buffer-1200ms-20260501T070823Z`, 120 samples, final health state `stable`, stream playing.
  This is the first strict clean reference: during the long capture `underruns=0` and `stale_drops=91` stayed flat from precheck to final sample, with `overruns=0`, `callback_starvations=0`, `audio_callback_xruns=0`, and `jitter_ms=0`.
- Captured the 600 second TCP jitter profile at global `buffer_ms=1200` with Linux `netem delay 20ms 10ms distribution normal`:
  `tcp-jitter-20ms-buffer-1200ms-20260501T071941Z`, 120 samples, final health state `stable`, stream playing.
  The run stayed healthy: `underruns=0`, `stale_drops=92`, `overruns=0`, `callback_starvations=0`, `audio_callback_xruns=0`, and `jitter_ms=0`.
- Captured a 180 second exploratory TCP jitter profile at global `buffer_ms=1200` with Linux `netem delay 50ms 20ms distribution normal`:
  `tcp-jitter-50ms-buffer-1200ms-quick-20260501T073105Z`, 36 samples, final health state `stable`, stream playing.
  The run showed mild degradation compared with the previous jitter run: final `underruns=2`, `stale_drops=101`, `jitter_ms=2`, with `overruns=0`, `callback_starvations=0`, and `audio_callback_xruns=0`.
- Captured a 180 second exploratory TCP jitter profile at global `buffer_ms=1200` with Linux `netem delay 100ms 50ms distribution normal`:
  `tcp-jitter-100ms-buffer-1200ms-quick-20260501T073617Z`, 36 samples, final health state `stable`, stream playing.
  The run stayed connected and stable but showed higher measured jitter and lower final buffer depth:
  `jitter_ms=36`, `buffer_depth_ms=520`, `underruns=2`, `stale_drops=105`, with audio backend counters still at zero.
- Captured a 180 second exploratory TCP jitter profile at global `buffer_ms=1200` with Linux `netem delay 200ms 100ms distribution normal`:
  `tcp-jitter-200ms-buffer-1200ms-quick-20260501T074052Z`, 36 samples, final health state `underrun`, stream playing.
  This is the first clear reproducible jitter failure threshold: final `buffer_depth_ms=240`, `jitter_buffer_chunks=12`, `underruns=34`, `stale_drops=146`, while audio backend counters stayed at zero.
- Captured a 180 second exploratory TCP jitter profile at global `buffer_ms=1200` with Linux `netem delay 150ms 75ms distribution normal`:
  `tcp-jitter-150ms-buffer-1200ms-quick-20260501T074634Z`, 36 samples, final health state `stable`, stream playing.
  Because client counters were cumulative from the previous session, read this as a delta from the 200 ms run: approximately `underruns +3` and `stale_drops +2`, with final `jitter_ms=39` and `buffer_depth_ms=640`.
- Captured a 180 second exploratory TCP packet-loss profile at global `buffer_ms=1200` with Linux `netem loss 1%`:
  `tcp-loss-1pct-buffer-1200ms-quick-20260501T075405Z`, 36 samples, final health state `stable`, stream playing.
  The run stayed clean after reconnect/reset: final `underruns=0`, `stale_drops=0`, `jitter_ms=0`, `overruns=0`, `callback_starvations=0`, and `audio_callback_xruns=0`.
- Captured a 180 second exploratory TCP packet-loss profile at global `buffer_ms=1200` with Linux `netem loss 3%`:
  `tcp-loss-3pct-buffer-1200ms-quick-20260501T080926Z`, 36 samples, final health state `stable`, stream playing.
  The run stayed fully clean: `underruns=0`, `stale_drops=0`, `jitter_ms=0`, `buffer_depth_ms=800`. TCP retransmissions and the 1200 ms buffer together absorb 3% loss without any audible impact.
- Captured a 180 second exploratory TCP packet-loss profile at global `buffer_ms=1200` with Linux `netem loss 5%`:
  `tcp-loss-3pct-buffer-1200ms-quick-20260501T081321Z` (profile name reflects script label), 36 samples, final health state `stable`, stream playing.
  Still fully clean: `underruns=0`, `stale_drops=0`, `jitter_ms=0`. Buffer depth rose to 1160 ms, approaching the 1200 ms ceiling, as TCP retransmissions introduce queuing delay but remain within the configured budget.
- Captured a 180 second exploratory TCP packet-loss profile at global `buffer_ms=1200` with Linux `netem loss 10%`:
  `tcp-loss-10pct-buffer-1200ms-quick-20260501T081739Z`, 36 samples, final health state `stable`, stream playing.
  Still clean: `underruns=0`, `stale_drops=0`, `jitter_ms=0`, `buffer_depth_ms=900`. TCP retransmission overhead is becoming visible in buffer depth variance but the 1200 ms target continues to absorb it.
- Captured a 180 second exploratory TCP packet-loss profile at global `buffer_ms=1200` with Linux `netem loss 20%`:
  `tcp-loss-20pct-buffer-1200ms-quick-20260501T082249Z`, 36 samples, final health state `stable`, stream playing.
  First signs of real degradation: `underruns=4`, `stale_drops=10`, `jitter_ms=3`, `buffer_depth_ms=1040`. This is the transition zone where retransmission latency begins to exceed the jitter buffer target.
- Captured a 180 second exploratory TCP packet-loss profile at global `buffer_ms=1200` with Linux `netem loss 30%`:
  `tcp-loss-30pct-buffer-1200ms-quick-20260501T082616Z`, 36 samples, final health state `stable` (reported), stream playing.
  Catastrophic degradation: `underruns=69`, `stale_drops=2148`, `jitter_ms=25`, `buffer_depth_ms=920`.
  The stale_drops count exploded 215x versus the 20% run — this is the TCP head-of-line blocking signature: segments stuck in retransmit queues arrive long after the playout deadline, causing the jitter buffer to overflow with stale chunks.
  **Monitoring gap identified**: `health_state` remained `stable` throughout despite 69 underruns and 2148 stale drops. The health state transition logic did not require sustained clean signal before returning to Stable.
- Fixed the health state monitoring gap by adding hysteresis (`stable_streak`) to `HealthTransitionTracker` in `server/src/session.rs`:
  - Added `STABLE_STREAK_REQUIRED = 5` constant (5 × 2-second reporting interval = 10 seconds of clean signal required).
  - `HealthTransitionTracker` now tracks `stable_streak`: consecutive clean health-report intervals while in `Recovering`.
  - State machine after fix: first clean interval after `Degraded`/`Underrun` → `Recovering`; stays `Recovering` until streak reaches threshold; any degradation resets streak; clean sequences with no prior bad state promote to `Stable` immediately.
  - Transition log now includes `stable_streak` for observability.
  - Added 7 unit tests: first-report stable, underrun detection, single-clean-stays-recovering, full-streak-promotes-to-stable, degradation-resets-streak, degraded-after-stale-drops, stable-without-prior-bad-state.
  - All 65 tests passing (8 `sonium-server`, 51 + 14 `sonium-protocol`).

Adjustment from implementation review:

- The existing telemetry is a useful Phase 0 foundation, so the next work should enrich and calibrate it instead of duplicating it.
- Some current counters are mixed between cumulative jitter-buffer counters and periodic audio-device counters. Server-side health transitions now compute deltas for cumulative fields, but benchmark gates should still normalize report semantics before being treated as final.
- UDP work should remain blocked until a TCP baseline report and at least one reproducible dropout profile exist.
- The 30% loss run is the reproducible catastrophic TCP baseline required by Phase 0 exit criteria. TCP head-of-line blocking at this level makes the comparison case for RTP/UDP clear.
- The health state monitoring gap has been resolved. Phase 0 exit criteria are met.

Phase 0 is complete.

---

Phase 1 — transport abstraction layer — implemented on 2026-05-01:

- Created `crates/transport` (`sonium-transport`) workspace crate with:
  - `TransportMode` enum: `tcp` (default), `rtp_udp`, `quic_dgram` — serde `snake_case`, `Display`.
  - `TransportConfig` struct: `{ mode: TransportMode, udp_port: u16 }` — TOML-deserializable under `[server.transport]`.
  - `MediaSender` trait: `send_wire_bytes(&mut self, bytes: &[u8]) -> BoxFuture<Result<()>>` with `BoxFuture` return type for future `dyn`-compatibility without `async_trait`.
  - `TcpMediaSender<'w>`: borrows `&'w mut OwnedWriteHalf`; implements `MediaSender`; writes pre-encoded `WireChunk` bytes with 5-second timeout.
  - `RtpUdpMediaSender` placeholder for Phase 2.
  - `QuicDgramMediaSender` stub: `unimplemented!()` — placeholder for Phase 5.
- Added `transport: TransportConfig` field to `ServerNet` in `crates/common/src/config.rs`. Default is `tcp`. Config key: `[server.transport] / mode = "tcp"`.
- `sonium-transport` added to workspace `Cargo.toml` members. `sonium-common` and `sonium-server` depend on it.
- `server/src/session.rs` updated:
  - Audio frame delivery now routed through `TcpMediaSender::new(stream).send_wire_bytes(&f.wire_bytes).await` instead of the inline `write_all_with_timeout` call.
  - Session start logs `transport_mode` alongside stream and peer info.
  - If a non-`tcp` mode is configured, a `warn!` is emitted and the session falls back to TCP (only supported mode in Phase 1).
  - Control-plane writes (CodecHeader, ServerSettings, Time, etc.) continue to use `&mut OwnedWriteHalf` directly — they always stay on TCP regardless of media transport mode.
- All workspace tests passing: 160+ tests across 8 crates.

Phase 1 exit criteria met:
- Existing TCP behavior is functionally identical through the abstraction.
- Transport mode is config-driven (`[server.transport] mode = "tcp"`).
- Codec/sync/playout modules do not depend on the transport implementation.
- No regression in existing tests.

Phase 2 — RTP/UDP unicast media path — implemented on 2026-05-01:

- Created `crates/transport/src/rtp.rs` with:
  - Standard 12-byte RTP header encode/decode (V=2, PT=96, seq u16 BE, timestamp u32 BE, SSRC u32 BE).
  - `SONIUM_RTP_PAYLOAD_TYPE = 96` (dynamic range), `RTP_CLOCK_RATE = 90_000` Hz.
  - `rtp_from_wire_bytes(wire_bytes, seq, ssrc)`: extracts Sonium WireChunk payload from wire bytes (offset 26, after the 26-byte `MessageHeader`) and derives RTP timestamp from the chunk's `i32` sec/usec fields (`sec * 90000 + usec * 90000 / 1_000_000`).
  - 11 RTP unit tests: encode/decode round-trip, header byte layout, timestamp conversion, payload-type/version validation, and `rtp_from_wire_bytes`.
- Replaced `RtpUdpMediaSender` stub in `crates/transport/src/sender.rs` with a real implementation:
  - Fields: `socket: Arc<UdpSocket>`, `peer_addr: SocketAddr`, `ssrc: u32`, `sequence: u16`.
  - `send_wire_bytes`: calls `rtp_from_wire_bytes`, wraps sequence with `wrapping_add`, sends via `socket.send_to` with a 2-second timeout.
- Added `udp_port: u16` to `TransportConfig` in `crates/transport/src/lib.rs` (default 0 = disabled).
- Added `udp_port: u16` to `Hello` message (`#[serde(rename = "UdpPort", default)]`). Client advertises the ephemeral port it bound for RTP reception.
- Added `transport_mode: String` and `server_udp_port: u16` to `ServerSettings` (both `#[serde(default)]`). Server echoes effective transport so clients know which path is active.
- Added `TransportModeChanged { mode: String, server_udp_port: u16 }` WebSocket event so the UI can react to live transport switches.
- Added `transport: Mutex<TransportState>` to `ServerState` (`crates/control/src/state.rs`) with methods:
  - `set_transport_config(mode, udp_port)` — called at startup from loaded config.
  - `transport_mode()`, `server_udp_port()` — read-only accessors.
  - `set_transport_mode(mode)` — runtime mutation, emits `TransportModeChanged` event.
- Added `GET /api/server/transport` and `PATCH /api/server/transport` (operator-protected) REST endpoints in `crates/control/src/api.rs`. Accepted modes: `tcp`, `rtp_udp`, `quic_dgram`.
- Updated `server/src/main.rs`:
  - Binds a shared UDP socket on `bind:transport.udp_port` at startup (skipped if port is 0).
  - Calls `state.set_transport_config(mode, udp_port)` to initialize runtime state from config.
  - Passes `Arc<UdpSocket>` to every session via `session::handle`.
- Updated `server/src/session.rs`:
  - Reads `state.transport_mode()` at session start (snapshot; client reconnects to get live changes).
  - If `RtpUdp` and both server socket and `hello.udp_port > 0`: creates `RtpUdpMediaSender` targeting `peer.ip():hello_udp_port`. SSRC derived from peer address via `DefaultHasher` (deterministic, unique per client).
  - Audio frame dispatch: if `rtp_sender.is_some()` → send via RTP/UDP; otherwise → TCP.
  - `send_server_settings` now includes `transport_mode` and `server_udp_port` so clients learn the effective path immediately.
- Updated `client/src/controller.rs`:
  - Binds ephemeral UDP socket (`0.0.0.0:0`) before sending Hello; advertises port in `hello.udp_port`.
  - On `ServerSettings` with `transport_mode == "rtp_udp"`: spawns a UDP receiver task that reads datagrams, decodes the RTP packet, and sends the payload over an unbounded channel (`udp_chunk_rx`).
  - Added `recv_optional_udp` helper (returns `pending()` when channel absent) for a clean `tokio::select!` arm.
  - UDP select arm processes received WireChunk payloads identically to the TCP path (decode, volume, EQ, playout scheduling).
- Updated `web/src/views/admin/StreamsTab.vue`:
  - Added Media transport controls for `tcp`, `rtp_udp`, and `quic_dgram`.
  - Added Server UDP port input and an RTP/UDP preset (`rtp_udp`, UDP port `1712`). This keeps the control API at `1711/TCP`, client/session TCP at `1710/TCP`, and RTP media on a distinct UDP socket.
  - Saving global tuning now persists `[server.transport]` in TOML and attempts to apply the selected runtime transport mode through `PATCH /api/server/transport`.

Design decisions:
- RTP payload = `wire_bytes[26..]` (the WireChunk bytes after the 26-byte Sonium `MessageHeader`). The client calls `WireChunk::decode()` directly on the payload — no extra framing needed.
- Transport mode is snapshotted at session start. Live `PATCH /api/server/transport` changes take effect for new connections only. This is the simplest correct behavior for Phase 2; Phase 6 will add per-session negotiation and fallback.
- Client UDP socket is bound once per connection attempt. On reconnect the socket is recreated and a fresh port is advertised.
- Server UDP socket is shared across all sessions (one `Arc<UdpSocket>`); each session uses `send_to` with its client's address.

Phase 2 exit criteria status:
- RTP/UDP path compiles and builds cleanly (all workspace tests pass: 173+ tests, 0 failures).
- Transport mode is configurable via `[server.transport] mode = "rtp_udp"` in TOML and via `PATCH /api/server/transport` at runtime.
- Backend can broadcast transport changes via WebSocket (`TransportModeChanged`) and expose them through REST.
- Web UI can persist and apply transport mode and UDP port from the Streams global tuning panel.
- TCP fallback path unchanged and still working.

Remaining for Phase 2 validation:
- Live RTP/UDP audio playback soak test on clean LAN.
- Controlled loss comparison vs. TCP baseline (Phase 0 data: TCP fails at ~30% with head-of-line blocking).

Phase 2 validation findings from first live `v0.1.51` run:

- RTP/UDP session activation worked: server bound the UDP media socket and logged `RTP/UDP media path active`; client stayed connected through the TCP control/session socket and received media over the advertised client UDP port.
- Clean-LAN playback was audibly unstable even at high configured buffers (`1200–2000 ms`), while TCP remained stable at `1200 ms`. This means Phase 2 is functionally wired but not ready as a recommended transport.
- Metrics were initially misleading because server health observations still hardcoded the label `transport="tcp"` even when the effective session mode was `rtp_udp`.
- The UI/config was confusing because the preset used UDP port `1711`, the same number as the TCP control API. TCP and UDP can share a numeric port without colliding, but Sonium should prefer clearer defaults: `1710/TCP` client session, `1711/TCP` control API, `1712/UDP` RTP media.

Validation hardening applied after the first live run:

- Server health metrics now use the effective session transport label (`tcp` or `rtp_udp`) instead of hardcoding `tcp`.
- Client health reports now include RTP receive diagnostics: `rtp_packets_received`, `rtp_sequence_gaps`, and `rtp_decode_error_count`.
- Server Prometheus metrics expose those fields as `sonium_client_rtp_packets_received`, `sonium_client_rtp_sequence_gaps`, and `sonium_client_rtp_decode_errors`.
- Health classification treats new RTP sequence gaps and RTP decode errors as degradation signals.
- Web UI RTP/UDP preset now uses UDP port `1712`; the UDP port input is disabled while TCP is selected and the help text explains that the value is ignored for TCP.

Next work: publish the validation-hardening release, rerun clean RTP/UDP with the new metrics, then decide whether the bug is packet loss/gaps, RTP decode/path issues, or buffer/playout scheduling.

---

## Phase 0 - Baseline and observability hardening

## Goal

Make current behavior measurable before changing transport.

No UDP implementation should begin before this phase is complete.

## Tasks

1. Add server and client metrics for:

```text
packet/chunk inter-arrival jitter
decoder queue depth
playout queue depth
late chunk drops
stale drops
underruns
callback starvation events
audio callback xruns
end-to-end playout estimate
```

2. Add transport-tagged metrics:

```text
transport=tcp
transport=rtp_udp
transport=quic_dgram
```

3. Add audio clock metrics:

```text
audio_clock_vs_network_clock_drift_ppm
clock_drift_ppm_estimate
```

4. Add debug logging for health transitions:

```text
buffering
stable
degraded
recovering
underrun
resync
fallback
```

5. Publish a common audio health model.

Example:

```json
{
  "client_id": "lobby-speaker-01",
  "transport": "tcp",
  "state": "degraded",
  "playout_queue_ms": 72,
  "underruns_last_minute": 2,
  "jitter_p95_ms": 9.4,
  "clock_drift_ppm": -16.2
}
```

6. Add reproducible test profiles:

```text
clean LAN
LAN with 1% random loss
LAN with 3% random loss
burst loss
high jitter
CPU stress
Wi-Fi variation
```

## Exit criteria

- A single dashboard/report can compare two runs by transport.
- At least one controlled dropout reproduction exists on the current TCP path.
- Health states appear in logs/UI with actionable counts.
- Baseline TCP behavior is documented before transport changes.

---

## Phase 1 - Transport abstraction layer

## Goal

Isolate transport concerns behind stable interfaces.

## Tasks

1. Introduce transport traits/interfaces for audio send/receive.

Server-side concept:

```rust
trait AudioTransportPublisher {
    fn start_session(&mut self, session: TransportSession) -> Result<()>;
    fn send_media_packet(&mut self, packet: MediaPacket) -> Result<()>;
    fn stop_session(&mut self, client_id: ClientId) -> Result<()>;
}
```

Client-side concept:

```rust
trait AudioTransportReceiver {
    fn connect(&mut self, config: TransportConfig) -> Result<()>;
    fn receive_media_packet(&mut self) -> Result<MediaPacket>;
    fn health(&self) -> TransportHealth;
}
```

2. Wrap existing TCP behavior:

```text
TcpAudioTransport
```

3. Add placeholders:

```text
RtpUdpAudioTransport
QuicDgramAudioTransport
```

4. Add static config transport selection:

```toml
[audio.transport]
mode = "tcp"
```

5. Prepare capability negotiation without requiring it immediately.

## Exit criteria

- Existing TCP behavior remains functionally identical through the abstraction.
- Transport can be switched via config.
- Codec/sync/playout modules do not depend directly on the transport implementation.
- No regression in existing integration tests.

---

## Phase 2 - RTP/UDP unicast media path

## Goal

Deliver audio over UDP using RTP semantics and robust client-side receive behavior.

## Tasks

1. Implement RTP/UDP packet format.

Prefer:

```text
standard RTP header
Sonium payload/session metadata
optional RTP extension only if required
```

2. Implement server UDP sender:

```text
socket lifecycle
per-client destination mapping
MTU-safe packet sizing
packet pacing
sequence number management
timestamp mapping
DSCP marking
```

3. Implement client UDP receiver:

```text
out-of-order handling
duplicate detection
sequence wrap handling
reordering window
packet arrival timestamping
bounded receive queue
```

4. Add basic jitter buffer:

```text
fixed target depth initially
late packet drop
never block audio callback
silence/concealment on missing frame
```

5. Add operational docs:

```text
UDP port requirements
LAN assumptions
firewall guidance
Wi-Fi caveats
recommended initial buffer ranges
```

## Exit criteria

- RTP/UDP path plays continuously on clean LAN.
- RTP/UDP continues playback under controlled 1–3% random packet loss.
- No global stall behavior equivalent to TCP head-of-line blocking is observed.
- The audio callback is protected from network receive jitter.
- TCP fallback still works.

---

## Phase 2.5 - RTCP-style receiver reports and transport feedback

## Goal

Give the server and UI real visibility into receiver-side transport quality.

## Tasks

1. Add receiver reports from client to server.

Minimum fields:

```json
{
  "client_id": "lobby-speaker-01",
  "transport": "rtp_udp",
  "packets_received": 120000,
  "packets_lost": 34,
  "packets_late": 12,
  "packets_duplicate": 3,
  "packets_reordered": 18,
  "jitter_ms_p50": 1.2,
  "jitter_ms_p95": 3.8,
  "jitter_ms_p99": 7.4,
  "jitter_buffer_target_ms": 80,
  "underruns_last_minute": 0,
  "clock_drift_ppm": -18.4
}
```

2. Add sender-side report support.

Useful fields:

```text
sender packet count
sender byte count
media timestamp
wallclock/monotonic timestamp reference
```

3. Correlate server and client stats.

4. Expose receiver quality in admin UI/logs.

5. Add selection reason and fallback reason logging.

Example:

```text
client=lobby-speaker-01 transport=rtp_udp state=stable loss=0.02% jitter_p95=2.8ms buffer=80ms
```

## Exit criteria

- Server can see receiver-side loss/jitter/late packet stats.
- UI/logs can explain whether a client is healthy, degraded, or unstable.
- Operators can distinguish packet loss, jitter, underruns, and clock drift.

---

## Phase 3 - Adaptive jitter buffer and playout policy

## Goal

Make low-buffer operation stable across varying jitter and CPU pressure.

## Tasks

1. Implement adaptive jitter buffer controller.

Behavior:

```text
increase target quickly on sustained late/loss/underrun events
decrease target slowly when stable
respect minimum and maximum bounds
```

2. Define hard bounds:

```toml
[jitter_buffer]
mode = "adaptive"
min_ms = 40
initial_ms = 100
max_ms = 500
decrease_step_ms = 5
increase_step_ms = 20
```

3. Define late packet policy:

```text
if packet arrives after playout deadline:
    drop
    count late drop
    do not block callback
```

4. Couple controller with audio callback telemetry:

```text
callback starvation has priority over latency reduction
underrun causes buffer target increase
stable callback allows slow reduction
```

5. Expose policy knobs in config and admin UI:

```text
auto
manual
low-latency
balanced
resilient
```

## Exit criteria

- System converges to stable playout depth under variable network conditions.
- Underruns decrease versus fixed-buffer policy at similar median latency.
- Audio callback remains protected from network jitter.
- Buffer behavior is visible and explainable.

---

## Phase 3.5 - Clock drift correction and playout discipline

## Goal

Prevent long-running clients from drifting out of sync due to audio device clock differences.

This phase is essential for serious multiroom audio.

## Tasks

1. Estimate drift between server media timeline and client audio device timeline.

Track:

```text
server media timestamp progression
client playout progression
audio device callback progression
jitter buffer level trend
```

2. Report drift in ppm per client.

Example:

```text
client=pool-speaker-02 drift=-21.7ppm
```

3. Separate network jitter from audio clock drift.

Network jitter is short-term arrival variation.

Clock drift is long-term rate mismatch.

4. Implement correction strategy.

Preferred:

```text
high-quality adaptive resampling
```

Fallback:

```text
very occasional sample slip/drop/insert with smoothing
```

5. Ensure correction is gradual and inaudible.

6. Add diagnostics:

```text
drift_ppm
correction_ppm
resampler_active
buffer_trend_ms_per_min
```

7. Prepare future clock reference modes:

```text
LocalMonotonic
ServerWallClockNtp
PtpDomain
HardwareTimestamped
```

## Exit criteria

- Long-running clients remain aligned without frequent hard resyncs.
- Drift correction is visible in metrics.
- System can distinguish “bad network” from “bad clock.”
- Adding larger buffers improves stability instead of hiding uncontrolled drift.

---

## Phase 4 - QoS, network profiles, and operational hardening

## Goal

Make Sonium predictable on real managed networks.

## Tasks

1. Implement DSCP/QoS config for media and control traffic.

Example:

```toml
[network.qos]
enabled = true
media_dscp = "EF"
control_dscp = "AF21"
```

2. Log effective socket/network priority status.

Example:

```text
media_dscp=EF applied=true
socket_priority=6 applied=true
```

3. Add network profile presets:

```text
wired_lan_low_latency
wired_lan_balanced
wifi_resilient
wan_encrypted
tcp_compatibility
```

4. Add warnings for poor conditions:

```text
high jitter
burst loss
late packets increasing
buffer target near maximum
clock drift high
```

5. Document managed network recommendations:

```text
prefer wired clients for critical zones
avoid congested Wi-Fi for low buffer operation
enable proper switch behavior where applicable
keep TCP fallback available
```

## Exit criteria

- QoS settings are configurable and observable.
- Operators can choose profiles by environment.
- Poor network conditions produce actionable warnings.

---

## Phase 5 - QUIC DATAGRAM transport

## Goal

Add encrypted datagram-based media transport while preserving shared media semantics.

## Tasks

1. Select QUIC stack.

Evaluation criteria:

```text
Rust maturity
DATAGRAM support
TLS/auth integration
connection migration behavior
performance
cross-platform support
maintenance health
```

2. Implement QUIC session establishment:

```text
handshake
auth binding
client identity
stream identity
capability negotiation
```

3. Implement DATAGRAM media channel:

```text
same MediaPacket model
same jitter buffer
same late-drop policy
same metrics
same packet loss handling
```

4. Use reliable QUIC stream only for:

```text
control/session metadata
transport negotiation
health reports if appropriate
```

5. Tune reconnect and timeout behavior.

6. Validate behavior across:

```text
LAN
routed networks
NAT
temporary connection loss
client reconnect
```

## Exit criteria

- QUIC DATAGRAM path works end-to-end.
- Media delivery is encrypted.
- Loss behavior is similar to UDP, without TCP-style media stalls.
- Control/data plane interaction is stable across reconnects.
- RTP/UDP remains preferred for simple LAN unless QUIC is explicitly chosen.

---

## Phase 6 - Negotiation, fallback, and compatibility policy

## Goal

Enable deterministic multi-transport runtime behavior.

## Tasks

1. Define transport profiles.

Example:

```toml
[audio.transport_profiles.lan]
preferred = ["rtp_udp", "tcp"]

[audio.transport_profiles.wan]
preferred = ["quic_dgram", "tcp"]

[audio.transport_profiles.compatibility]
preferred = ["tcp"]
```

2. Add capability advertisement.

Client reports:

```text
supports_tcp
supports_rtp_udp
supports_quic_dgram
supports_dscp
supports_adaptive_jitter
supports_drift_correction
```

3. Add selection reason logging.

Example:

```text
client=lobby-speaker-01 selected_transport=rtp_udp reason=lan_profile_preferred
```

4. Add automatic fallback triggers:

```text
session setup failure
UDP timeout
QUIC handshake failure
excessive late packets
excessive underruns
operator override
```

5. Add sticky-session behavior:

```text
stay on selected transport for current session
optionally retry preferred transport after cooldown
```

6. Add operator controls:

```text
force global transport
force per-client transport
disable transport
fallback to TCP now
```

## Exit criteria

- Clients connect with deterministic selection.
- Fallback happens automatically and is clearly logged.
- Operator can pin transport quickly during incidents.
- Existing TCP-only clients remain supported if desired.

---

## Phase 7 - Validation matrix and performance gates

## Goal

Define objective go/no-go thresholds before broad enablement.

## Test matrix

## Platforms

```text
Linux
Windows
macOS
embedded Linux targets if applicable
```

## Networks

```text
clean wired LAN
wired LAN with induced loss
wired LAN with induced jitter
burst loss
Wi-Fi good signal
Wi-Fi poor signal
routed VLAN
WAN/VPN simulation
```

## Load

```text
idle CPU
moderate CPU
stressed CPU
disk/network pressure
UI/admin activity during playback
```

## Streams

```text
PCM 48kHz 16-bit stereo
PCM 48kHz 24-bit if supported
Opus low-latency
multiple chunk_ms values
multiple simultaneous zones
```

## Metrics

Track:

```text
mean packet jitter
95p packet jitter
99p packet jitter
underruns per hour
late-drop ratio
packet loss ratio
concealment frames per hour
effective playout latency
clock drift ppm
resync events
fallback events
reconnect time
CPU usage
memory usage
```

## Example gates

Tune these with real data.

```text
No catastrophic freezes in 8-hour wired LAN soak.
RTP/UDP underruns per hour below TCP baseline at equivalent or lower latency.
Under 1–3% random loss, RTP/UDP produces fewer audible freezes than TCP.
Drift correction keeps clients aligned over long-running tests.
Fallback success rate above 99% in disruption tests.
No audio callback starvation under moderate CPU load.
```

## Exit criteria

- Transport decisions are based on data, not assumptions.
- RTP/UDP is proven better than TCP under representative LAN impairment.
- QUIC DATAGRAM is proven useful for encrypted/routed scenarios.
- Rollout can proceed with confidence.

---

## Phase 8 - Rollout strategy

## Goal

Ship incrementally with low blast radius.

## Rollout sequence

1. Hidden feature flags.

```text
rtp_udp_enabled=false
quic_dgram_enabled=false
adaptive_jitter_enabled=false
drift_correction_enabled=false
```

2. Internal/dev opt-in for observability and TCP abstraction.

3. Internal/dev opt-in for RTP/UDP on known wired test rigs.

4. Broader LAN opt-in with telemetry collection.

5. Adaptive jitter enabled for selected clients.

6. Drift correction enabled for selected clients.

7. QUIC DATAGRAM experimental opt-in.

8. Recommended profiles by environment:

```text
wired LAN: RTP/UDP balanced
Wi-Fi: RTP/UDP resilient or Opus resilient
WAN: QUIC DATAGRAM
compatibility: TCP
```

9. Keep TCP fallback available indefinitely.

## Operational safeguards

```text
one-command rollback to TCP-only
per-client transport override
transport failure alarms
release notes with limitations
clear migration docs
```

## Exit criteria

- New transport stack can be disabled immediately.
- Operators understand which profile to choose.
- Rollout failures do not take down the whole system.

---

## Phase 9 - Future PTP and professional timing readiness

## Goal

Prepare Sonium for stronger clocking without making it a dependency today.

## Tasks

1. Keep clock reference abstraction clean.

2. Add optional PTP discovery/diagnostics later.

3. Investigate hardware timestamping support.

4. Add admin visibility:

```text
clock_source=local_monotonic
clock_source=ptp
ptp_domain=0
ptp_offset_ns=...
hardware_timestamping=true/false
```

5. Explore AES67-inspired compatibility only after Sonium’s native timing model is stable.

## Exit criteria

- Sonium can integrate better clock references without redesigning media transport.
- PTP can improve the system where available.
- Non-PTP deployments remain fully supported.

---

## Phase 10 - Optional multicast profile

## Goal

Improve efficiency for large same-stream deployments.

## Not part of initial migration.

## Tasks

1. Add multicast stream mode.

2. Document network requirements:

```text
managed switches
IGMP snooping
wired recommended
Wi-Fi caution
```

3. Add per-stream multicast group assignment.

4. Keep unicast fallback.

## Exit criteria

- Multicast works only where network conditions support it.
- Operators can choose unicast or multicast intentionally.
- Multicast does not complicate the core unicast use case.

---

# Work breakdown by area

---

## Server

- Transport abstraction integration.
- RTP/UDP sender.
- QUIC endpoint/session manager.
- RTP timestamp generation.
- RTCP-style receiver report handling.
- Capability negotiation.
- Transport fallback logic.
- Per-client health state tracking.
- Metrics emission.
- DSCP/QoS application.
- Admin/UI diagnostics API.

---

## Client

- Multi-transport receive loop.
- RTP/UDP receiver.
- QUIC DATAGRAM receiver.
- Shared packet reorder/loss handling.
- Shared jitter buffer.
- Adaptive playout controller.
- Audio callback starvation detection.
- Packet loss concealment.
- Clock drift estimation.
- Drift correction/resampling.
- Health reporting.
- Transport fallback response.

---

## Protocol and configuration

- Protocol versioning.
- Capability advertisement.
- Media packet schema.
- RTP payload mapping.
- Optional RTP extensions.
- Transport profile config.
- Jitter buffer config.
- Clock correction config.
- QoS config.
- Troubleshooting docs.
- Benchmark and test docs.

---

## UI/admin

Expose:

```text
client transport
transport selection reason
fallback reason
jitter p50/p95/p99
packet loss
late packets
underruns
jitter buffer target
effective playout latency
clock drift ppm
drift correction ppm
health state
QoS status
```

Suggested client health states:

```text
excellent
stable
degraded
unstable
recovering
fallback
offline
```

Example UI summary:

```text
Lobby Speaker 01
Transport: RTP/UDP
State: Stable
Jitter p95: 2.8 ms
Loss: 0.02%
Buffer target: 80 ms
Clock drift: -18.4 ppm
Underruns last hour: 0
QoS: EF applied
```

---

# Suggested milestones

## M1 - Observability and abstraction

Includes:

```text
Phase 0
Phase 1
```

Result:

```text
No behavior change, but Sonium becomes measurable and transport-ready.
```

---

## M2 - RTP/UDP functional LAN prototype

Includes:

```text
Phase 2
```

Result:

```text
RTP/UDP can play audio on LAN with basic jitter buffering.
```

---

## M3 - RTP/UDP measurable and diagnosable

Includes:

```text
Phase 2.5
Phase 3
```

Result:

```text
RTP/UDP has receiver reports, adaptive jitter buffer, late-drop policy, and useful health metrics.
```

---

## M4 - Long-running sync stability

Includes:

```text
Phase 3.5
```

Result:

```text
Clients can remain stable over long sessions with drift estimation and correction.
```

---

## M5 - Network hardening

Includes:

```text
Phase 4
```

Result:

```text
Sonium has QoS, network profiles, and operator-facing diagnostics.
```

---

## M6 - QUIC DATAGRAM experimental

Includes:

```text
Phase 5
```

Result:

```text
Encrypted datagram delivery works and shares the same media/playout logic.
```

---

## M7 - Negotiation and fallback production hardening

Includes:

```text
Phase 6
Phase 7
```

Result:

```text
Sonium chooses transports deterministically, falls back safely, and passes validation gates.
```

---

## M8 - Stable rollout

Includes:

```text
Phase 8
```

Result:

```text
RTP/UDP becomes recommended for LAN profiles where testing supports it.
TCP remains available.
QUIC DATAGRAM remains available for encrypted/routed scenarios.
```

---

## M9 - Future professional timing options

Includes:

```text
Phase 9
Phase 10 as optional
```

Result:

```text
Sonium can evolve toward PTP/multicast/professional timing without redesigning the core architecture.
```

---

# Recommended default profiles

## `wired_lan_balanced`

Recommended future default for normal LAN deployments.

```toml
[audio.profile.wired_lan_balanced]
transport_order = ["rtp_udp", "tcp"]
codec = "pcm"
jitter_mode = "adaptive"
jitter_initial_ms = 100
jitter_min_ms = 60
jitter_max_ms = 300
drift_correction = true
qos = true
```

---

## `wired_lan_low_latency`

For carefully controlled wired networks.

```toml
[audio.profile.wired_lan_low_latency]
transport_order = ["rtp_udp", "tcp"]
codec = "pcm"
jitter_mode = "adaptive"
jitter_initial_ms = 50
jitter_min_ms = 30
jitter_max_ms = 150
drift_correction = true
qos = true
```

---

## `wifi_resilient`

For Wi-Fi clients.

```toml
[audio.profile.wifi_resilient]
transport_order = ["rtp_udp", "tcp"]
codec = "opus"
jitter_mode = "adaptive"
jitter_initial_ms = 200
jitter_min_ms = 120
jitter_max_ms = 800
drift_correction = true
qos = true
```

---

## `wan_encrypted`

For routed or remote usage.

```toml
[audio.profile.wan_encrypted]
transport_order = ["quic_dgram", "tcp"]
codec = "opus"
jitter_mode = "adaptive"
jitter_initial_ms = 300
jitter_min_ms = 150
jitter_max_ms = 1200
drift_correction = true
qos = false
```

---

## `tcp_compatibility`

For maximum compatibility.

```toml
[audio.profile.tcp_compatibility]
transport_order = ["tcp"]
codec = "pcm"
jitter_mode = "fixed"
jitter_initial_ms = 500
drift_correction = true
qos = false
```

---

# Risks and mitigations

## Risk: UDP packet bursts overwhelm the client

Mitigation:

```text
server pacing
bounded receive queue
adaptive jitter buffer
late-drop policy
callback never waits on network
```

---

## Risk: Transport migration does not fix dropouts

Mitigation:

```text
complete Phase 0 first
measure callback starvation
measure clock drift
separate network jitter from audio device drift
do not blame transport blindly
```

---

## Risk: Custom RTP-like protocol becomes technical debt

Mitigation:

```text
use RTP semantics
use optional extensions
avoid unnecessary custom packet format
```

---

## Risk: QUIC complexity delays core stability

Mitigation:

```text
ship RTP/UDP first
keep QUIC behind same transport interface
reuse media packet and jitter logic
do not make QUIC the default LAN transport prematurely
```

---

## Risk: Metric noise masks real regressions

Mitigation:

```text
fixed benchmark scenarios
before/after comparison
8-hour soak tests
impairment testing
transport-tagged metrics
```

---

## Risk: Clock drift causes long-term desync

Mitigation:

```text
implement drift estimation
implement correction/resampling
expose ppm per client
prepare future PTP reference
```

---

## Risk: Wi-Fi expectations are unrealistic

Mitigation:

```text
add Wi-Fi resilient profile
warn about high jitter
recommend wired for critical zones
support Opus and larger buffers
```

---

# Definition of done

This migration is complete when all are true:

1. Sonium supports `tcp`, `rtp_udp`, and `quic_dgram` transports with documented behavior and config controls.
2. RTP/UDP uses real RTP-style timestamping and sequencing.
3. RTCP-style receiver feedback or equivalent health reporting exists.
4. Adaptive jitter buffering works and is visible.
5. Late packets are dropped safely without blocking the audio callback.
6. Packet loss concealment exists and is codec-aware.
7. Clock drift is estimated and corrected per client.
8. QoS/DSCP can be configured and observed.
9. Automatic fallback works reliably and is explainable.
10. RTP/UDP shows lower freeze incidence than TCP under representative LAN loss/jitter scenarios.
11. QUIC DATAGRAM works for encrypted/routed scenarios without changing playout logic.
12. Operators can diagnose transport and audio health from logs/UI.
13. TCP fallback remains available.
14. Future PTP/hardware timestamping can be added without redesigning the core media model.

---

# Resume checklist for future work

When resuming this project:

1. Confirm the latest completed phase.
2. Run the current benchmark scenarios.
3. Compare against the last accepted metrics.
4. Do not skip observability updates.
5. Implement only the next logical phase.
6. Re-run exit criteria before merging.
7. Update this document with actual results and changed assumptions.
8. Keep TCP fallback working.
9. Keep transport, jitter buffer, and clock correction logic separated.
10. Treat Dante as a stability reference, not as the product identity.

---

# Final positioning

Sonium should be positioned as:

> A modern, self-hosted, open-source distributed multiroom audio system focused on robustness, observability, flexible transport, and long-running stability.

The technical inspiration from professional systems is valid, but the product identity should remain clear:

```text
open source
self-hosted
operator-friendly
modern transport architecture
robust multiroom playback
measurable health
safe fallback
future-ready clocking
```

The strongest version of Sonium is not merely “Snapcast with UDP.”

It is:

> A distributed audio platform where transport, timing, playout, drift correction, and diagnostics are designed together from the beginning.
