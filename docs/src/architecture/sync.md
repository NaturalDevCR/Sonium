# Clock Synchronization

Getting audio to play in sync on multiple speakers is the hardest problem in
multiroom audio.  This page explains how Sonium solves it — and where it's
heading.

## The problem

Each client machine has its own oscillator.  Even if they agree on the time at
startup, clocks drift.  Two clients that are 10 ms apart will produce a
clearly audible echo effect.  The target is **< 1 ms** of desync.

## Current approach — NTP-like software sync

Sonium uses the same algorithm as Snapcast: a lightweight NTP-inspired
request–response loop that estimates the offset between the client clock and
the server clock.

### Exchange flow

```
Client                             Server
  │── Time { latency: 0 } ───────►│   (server records t_server_recv)
  │   (client records t_sent)      │
  │                                │   latency = t_server_recv − t_client_sent
  │◄── Time { latency: Δ } ────────│   (client records t_recv)
  │
  │  rtt  = t_recv − t_sent
  │  c2s  = Δ               (client→server, measured by server)
  │  s2c  = rtt − c2s       (server→client, estimated)
  │  diff = (c2s − s2c) / 2  ← signed offset (µs)
```

A positive `diff` means the server is *ahead* of the client.

### Median filter

Each `diff` value is pushed into a **200-entry circular buffer**.  The current
offset is the *median* of that buffer.  This makes the estimate robust against
transient network spikes and OS scheduling jitter — a single bad measurement
doesn't corrupt the offset.

The buffer fills in ~3 minutes at the default 1-second sync interval.  After
that it converges to a stable estimate.

```
Samples collected  │  Stability
─────────────────  │  ─────────────────────────────────
       1           │  Raw — no filtering
      10           │  Rough estimate ±few ms
      50           │  Good (±1 ms on typical LAN)
     100           │  Stable (±500 µs)
     200           │  Fully converged — median of full window
```

### Jitter buffer

Even with a good clock estimate, network packets arrive with variable delay
(jitter).  The [`SyncBuffer`][crate::sync::buffer] queues decoded PCM chunks
sorted by their playout timestamp and releases them at the right moment.

```
now_server = local_clock + offset
chunk is released when: chunk.playout_us ≤ now_server + target_latency
```

The `target_latency` (default: 1 000 ms) is the tradeoff between sync accuracy
and end-to-end latency.  Lower values give faster response to source changes;
higher values absorb more jitter.

## PTPv2 — future hardware sync

Software NTP-like sync achieves **~1 ms** accuracy on a quiet LAN, which is
excellent for most home setups.  However, modern Ethernet controllers support
**IEEE 1588v2 (PTPv2) hardware timestamping**, which gives **nanosecond-level**
accuracy — comparable to professional Dante/AES67 systems.

> This feature was [requested for Snapcast](https://github.com/badaix/snapcast/issues/1478)
> but has not been implemented.  Sonium is designed to support it.

### Why it matters

- Sub-microsecond accuracy vs ~1 ms for software sync
- Commodity hardware: USB adapters with PTP support are ~$10 (e.g. AX88179B chipset)
- Enables Sonos-quality synchronization without Sonos prices
- Opens the door to individual speaker nodes (Raspberry Pi Zero + USB adapter +
  speaker = a $25 perfectly-synchronized speaker)

### Architecture plan

The `sonium-sync` crate exposes a `TimeSource` trait (planned for Fase 2
hardening):

```rust
pub trait TimeSource: Send + Sync {
    /// Current server time in microseconds since UNIX epoch.
    fn now_server_us(&self) -> i64;
    /// True if the source has a valid lock on the clock.
    fn is_locked(&self) -> bool;
}
```

Two implementations will be provided:

| Implementation | Backend | Accuracy |
|---|---|---|
| `NtpTimeSource` | NTP-like request/response (current) | ~1 ms |
| `PtpTimeSource` | `/dev/ptp0` via `libptp` / `linuxptp` | ~1 µs |

The rest of the audio pipeline is identical — the `SyncBuffer` receives server
timestamps regardless of how they were obtained.

### Enabling PTP (planned CLI)

```bash
# Use hardware PTP on eth0
sonium-client --time-source ptp --ptp-interface eth0

# Fall back to NTP-like if no PTP hardware is found
sonium-client --time-source auto
```
