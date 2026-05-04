# Clock Synchronization

Getting audio to play in sync on multiple speakers is the hardest problem in
multiroom audio. This page explains how Sonium solves it — from the simple
case to enterprise-grade setups.

## The Problem

Each client machine has its own oscillator. Even if they agree on the time at
startup, clocks drift. Two clients that are 10 ms apart will produce a clearly
audible echo effect. The target is **< 5 ms** of desync for casual listening,
and **< 1 ms** for perfect unison.

## Three Levels of Sync

Sonium provides three levels of time synchronization, from simplest to most
precise:

| Level | Method | Accuracy | Setup Effort |
|-------|--------|----------|-------------|
| **Basic** | Protocol `Time` messages | ~10–50 ms | Zero config |
| **Good** | Protocol + Chrony/NTP | ~1–5 ms | Install chrony |
| **Excellent** | PTPv2 hardware timestamping | ~1 µs | Special hardware |

Most home users will be happy with **Good**. Only professional installations
need **Excellent**.

---

## Level 1: Basic (Built-in Protocol)

Sonium includes a lightweight **NTP-inspired** request–response loop that
estimates the offset between the client clock and the server clock. This works
out-of-the-box with zero configuration.

### Exchange Flow

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

### Median Filter

Each `diff` value is pushed into a **200-entry circular buffer**. The current
offset is the *median* of that buffer. This makes the estimate robust against
transient network spikes and OS scheduling jitter.

The buffer fills in ~3 minutes at the default 1-second sync interval.

```
Samples collected  │  Stability
─────────────────  │  ─────────────────────────────────
       1           │  Raw — no filtering
      10           │  Rough estimate ±few ms
      50           │  Good (±1 ms on typical LAN)
     100           │  Stable (±500 µs)
     200           │  Fully converged — median of full window
```

### Same-Machine Optimization

If the client and server run on the **same machine**, Sonium detects this
automatically (via `--on-server` flag or localhost detection) and **skips all
network sync**. The offset is exactly zero because both processes share the
same system clock.

```bash
# Explicit flag (also auto-detected for localhost)
sonium-client --on-server 192.168.1.50
```

---

## Level 2: Good (Chrony/NTP Recommended)

For multi-room setups with **multiple physical devices**, the built-in protocol
isn't enough. Network jitter and OS scheduling can cause 10–50 ms of drift.

### Solution: System-Level Time Sync

Install **chrony** on every Linux device:

```bash
sudo apt-get install chrony   # Debian/Ubuntu
sudo dnf install chrony       # Fedora
sudo pacman -S chrony         # Arch
```

Verify sync quality:

```bash
chronyc tracking
# Look for "System time" — should be within ±0.005 seconds
```

### How It Helps

- Chrony keeps all device clocks synchronized to within **±1 ms**
- Sonium's built-in protocol becomes a verification layer, not the primary sync
- The Web UI shows a green "Sync OK" indicator when drift is low

### GroupSync Protocol

When chrony (or any NTP) is active, Sonium's **GroupSync** message provides a
shared timeline reference:

- Server broadcasts `GroupSync` every **500 ms** to all clients
- Contains: `server_now_us`, `group_offset_us`, `rate_ppm`, `source_quality`
- Clients nudge their playout timing with **25% damping** to avoid audible pitch
  shifts

```
Server ──GroupSync────► Client A
   │                    (adjusts offset)
   └──GroupSync────► Client B
                      (adjusts offset)
```

The `source_quality` field (0.0–1.0) lets the UI show how confident the sync
is. When chrony is running, this approaches 1.0.

### Timezone Configuration

Set the timezone on each device so logs and the Web UI display correct local
time:

```toml
# sonium.toml
timezone = "America/Costa_Rica"
```

Or configure via the Sonium Agent UI (client-side only).

---

## Level 3: Excellent (PTPv2 Hardware)

Software sync achieves **~1 ms** accuracy on a quiet LAN. For **sub-microsecond**
accuracy (professional Dante/AES67 quality), modern Ethernet controllers support
**IEEE 1588v2 (PTPv2) hardware timestamping**.

### Hardware

- USB adapters with PTP support: ~$10 (e.g. AX88179B chipset)
- Raspberry Pi Zero + USB adapter + speaker = $25 perfectly-synchronized node

### Architecture (Planned)

The `sonium-sync` crate will expose a `TimeSource` trait:

```rust
pub trait TimeSource: Send + Sync {
    fn now_server_us(&self) -> i64;
    fn is_locked(&self) -> bool;
    fn quality(&self) -> f32; // 0.0–1.0
}
```

Implementations:

| Implementation | Backend | Accuracy |
|---|---|---|
| `NtpTimeSource` | Built-in protocol (current) | ~1 ms |
| `ChronyTimeSource` | chronyc status | ~100 µs |
| `PtpTimeSource` | `/dev/ptp0` via `linuxptp` | ~1 µs |

### Enabling PTP (Future CLI)

```bash
# Use hardware PTP on eth0
sonium-client --time-source ptp --ptp-interface eth0

# Auto-select best available
sonium-client --time-source auto
```

---

## Sync Health in the Web UI

The Sonium Web UI provides real-time sync monitoring:

| Indicator | Meaning | Action |
|-----------|---------|--------|
| 🟢 **Sync OK** | Drift < 10 ms | Nothing — enjoy your music |
| 🟡 **Sync Fair** | Drift 10–50 ms | Check chrony status |
| 🔴 **Sync Poor** | Drift > 50 ms | Install/configure chrony |
| ⚪ **Unknown** | No data yet | Wait for clients to connect |

The **Sync Monitor** page (`/sync`) shows per-client drift, buffer depth, and
latency. If sync is poor, it provides a one-click copy of the chrony install
command.

---

## Troubleshooting

### "Sync Poor" on all clients

1. Check chrony is installed and running on every device
2. Verify all devices are on the same network subnet
3. Check firewall rules (chrony uses UDP port 123)
4. Try restarting chrony: `sudo systemctl restart chronyd`

### One client is out of sync

1. Check that client's chrony status: `chronyc tracking`
2. Verify network connection to the server
3. Check for WiFi interference (switch to wired if possible)
4. Increase client latency offset: `sonium-client --latency 100`

### High jitter after TCP streaming fixes

If you see high jitter values after upgrading to v0.1.78+, this is expected
temporarily. The new TCP streaming stack eliminates head-of-line blocking, which
means packets arrive more evenly. Give the median filter ~3 minutes to converge.

---

## References

- [Snapcast time sync discussion](https://github.com/badaix/snapcast/issues/1478)
- [Chrony documentation](https://chrony-project.org/documentation.html)
- [IEEE 1588 PTP overview](https://www.ni.com/en-us/innovations/white-papers/14/what-is-ieee-1588-.html)
