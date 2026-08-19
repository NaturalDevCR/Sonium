# Roadmap

Sonium is moving quickly, but it is still an experimental project. This roadmap
is intentionally realistic: it lists what works, what is still shaky, and what
needs to be solved before Sonium can be recommended for production use.

::: danger Not production-ready
Do not deploy Sonium where dropouts, restarts, or configuration mistakes would
matter. It is currently best suited for local testing, development, and
adventurous home-lab experiments.
:::

## Working today

- Rust workspace with separate protocol, codec, sync, control, server, client,
  transport, and desktop-agent packages.
- Binary protocol with typed messages, unit tests, and fuzzing groundwork.
- Opus, FLAC, and PCM stream support.
- Server stream readers for stdin/FIFO/file paths, TCP, `pipe://` external
  processes, and meta streams.
- External `pipe://` recovery with restart backoff and captured ffmpeg stderr
  when a child process closes; file/FIFO reopening with bounded backoff and
  explicit `recovering` versus terminal `error` status.
- Groups, per-client volume/mute/latency, EQ, and live stream switching.
- Vue web UI with control view, admin dashboard, stream editor, raw config
  editor, users, dependency checks, logs, sync monitor, transport controls, and
  restart prompts.
- Authentication with Argon2 users, durable session-versioned JWT sessions,
  atomic owner-only account storage, and admin/operator/viewer roles.
- Fail-closed strict configuration: unknown keys and invalid semantic values
  prevent startup; control/metrics bind to loopback by default and client
  admission plus remembered identities are bounded.
- mDNS discovery, optional Snapcast mDNS advertising, and subnet scanning.
- Linux installer, systemd service, restart sudoers rule, Docker server flow,
  GitHub release packages, and macOS/Windows Desktop Agent builds.
- Client playback through CPAL with dedicated audio thread, output ring buffer,
  underrun crossfade, hotplug retry, output prefill, callback timing telemetry,
  and drift drop/dup counters.
- GroupSync for shared multi-room timelines, server-computed group offsets,
  source-quality reporting, and sync metrics in HealthReport.
- Prometheus metrics at `/metrics` for client health, sync error, transport
  state, ARQ/FEC, and group skew.
- Transport abstraction with stable `tcp`, implemented-but-validating `rtp_udp`,
  experimental Sonium-native ARQ/FEC over UDP as `rist`, and config-visible
  `quic_dgram` for a future QUIC DATAGRAM transport.
- Local-time structured logs, timezone config, and UI log filtering by recent
  time window.
- Home Assistant custom integration for groups/zones, speakers, stream status,
  latency controls, and health sensors.

## Known unresolved challenges

### Trusted-LAN deployment boundary

Phase 1 makes a local deployment safer to operate, but it is not an
Internet-safe deployment profile. Authentication covers the control plane;
Sonium still has no mutually authenticated or TLS-encrypted media path. Keep
audio and any deliberately exposed control endpoint inside a trusted LAN, and
apply host-firewall policy when changing `control_bind` away from loopback.

Existing `users.json` files without `session_version` are supported with a
default of `0` and are rewritten after a successful load. Old JWTs without the
claim require a new sign-in. Corrupt or unreadable existing account data is a
startup failure, not a request to create replacement credentials.

### Transport and low-latency playback reliability

Sonium is moving from a TCP-first design to a real-time-first design. TCP is the
safe default and has received substantial stability work. `rtp_udp` removes TCP
head-of-line blocking and `rist` adds Sonium-native NACK/FEC recovery over UDP,
but both paths still need broad validation on real hardware, Wi-Fi, and mixed
client fleets before they should be treated as production defaults.

### Authenticated media and UDP hardening

Authenticated/encrypted media, TLS deployment profiles, and hardening of the
UDP transports against untrusted traffic remain later work. They must be
designed and validated before Sonium can be recommended outside its trusted-LAN
boundary.

### Real-time callback certification

The callback-driven playback path exists, but scheduling, lock behavior, and
cross-device field measurements have not yet met a real-time certification bar.
It remains validation work rather than a production guarantee.

### Automatic buffer tuning

Today users tune `buffer_ms`, client latency, and `chunk_ms` manually. A better
experience observes jitter, underruns, stale drops, device callback timing,
playout percentiles, drift corrections, ARQ/FEC recovery, and network behavior,
then recommends or automatically adjusts safe values while still allowing manual
override. The config model and telemetry foundation are present; the tuning
policy still needs conservative field testing.

### Clock sync validation

The NTP-like software sync path, GroupSync broadcast, server-computed group
offsets, and client drift nudging are implemented. Sonium still has not been
validated across enough real devices, operating systems, DACs, and network
conditions to claim stable sub-millisecond or Sonos-class sync reliability.

### Source diagnostics

Radio streams and ffmpeg processes can fail for many reasons: server disconnects,
HTTP stalls, codec changes, stderr-only failures, DNS issues, TLS errors, or
upstream silence. Sonium now restarts `pipe://` sources and captures ffmpeg
stderr, but it still needs a proper operator-facing source health view.

### Safe configuration reloads

Some config changes still require a full server restart. The UI now prompts for
that and the installer can grant a narrow restart permission, but Sonium should
eventually support more partial reloads without dropping clients.

### Packaging and upgrade hardening

Release packaging works, but upgrade paths are still young. Older systemd units,
manual installs, and distro differences can miss permissions or dependencies.
The installer and Desktop Agent need more migration checks.

Long-running fault-injection tests, signed artifacts, a supported-platform
matrix, and release certification are also still required.

### Snapcast compatibility

Sonium has a migration path and optional Snapcast mDNS advertising, but full
drop-in protocol compatibility with every Snapcast client/version is not
guaranteed.

## Near-term roadmap

1. **Playback stability**

   - Validate `tcp`, `rtp_udp`, and `rist` against documented profiles.
   - Improve client-side adaptive output prefill and drift correction behavior.
   - Use telemetry to warn when a stream buffer or transport choice is unsafe
     for the current network/device.

2. **Stream tuning UX**

   - Add an optional automatic mode for `buffer_ms` and `chunk_ms`.
   - Keep manual controls for advanced users and debugging.
   - Document practical tuning recipes for radio, local files, AirPlay,
     Spotify Connect helpers, Bluetooth sinks, and low-latency LAN tests.

3. **Operator diagnostics**

   - Surface ffmpeg stderr and child-process exit status in the admin UI.
   - Add clearer stream health states: starting, playing, idle, recovering,
     failed.
   - Add troubleshooting panels that explain likely causes and next actions.

4. **Config and restart flow**

   - Expand partial reload support where safe.
   - Preflight restart permissions in the UI before asking the user to restart.
   - Make config changes show exactly whether they apply immediately or require
     restart.

5. **Security and deployment hardening**

   - Design authenticated media and TLS profiles for routed deployments.
   - Harden and validate UDP ingress before treating UDP transports as safe
     beyond a trusted LAN.
   - Expand account/config migration checks and secure backup guidance.

6. **Hardware validation**

   - Test mixed Linux/macOS/Windows clients on the same LAN.
   - Validate Raspberry Pi and USB DAC behavior.
   - Measure drift and sync error with real microphones/loopback capture.
   - Publish repeatable baseline profiles for TCP, RTP/UDP, and ARQ/FEC.

7. **Home Assistant polish**

   - Validate entities and services against current auth/role behavior.
   - Add richer diagnostics for unavailable clients and stale WebSocket state.
   - Document HACS and manual installation in the main docs site.

## Longer-term roadmap

- QUIC DATAGRAM transport for encrypted routed/WAN deployments.
- PTPv2/hardware timestamp support through the existing `TimeSource` abstraction.
- Cross-subnet relays and remote-site streaming.
- TLS/HTTPS and stronger deployment profiles.
- More source integrations: AirPlay, Spotify Connect, MPD/library workflows,
  library browsing, and automation-friendly source supervision.
- Better per-room DSP, normalization, and calibration tools.
- More polished installers and auto-update flows for the Desktop Agent.
- Signed releases, supported-platform certification, and long-running
  fault-injection coverage.

## Production-readiness bar

Before calling Sonium production-ready, we should be able to demonstrate:

- Stable playback for many hours from common sources, including internet radio.
- Predictable behavior at documented buffer targets.
- Clear admin diagnostics for every common failure mode.
- Safe upgrades across at least one previous minor version.
- Measured sync behavior on real multi-device hardware.
- Recovery from server restart, network drop, client sleep/wake, and device
  hotplug without manual cleanup.
- Evidence that TLS/authenticated media and UDP hardening are safe for the
  intended deployment boundary; Phase 1 does not supply that evidence.
