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
  and desktop-agent packages.
- Binary protocol with typed messages, unit tests, and fuzzing groundwork.
- Opus, FLAC, and PCM stream support.
- Server stream readers for stdin/FIFO/file paths, TCP, `pipe://` external
  processes, and meta streams.
- External `pipe://` recovery with restart backoff when a child process closes.
- Groups, per-client volume/mute/latency, EQ, and live stream switching.
- Vue web UI with control view, admin dashboard, stream editor, raw config
  editor, users, dependency checks, logs, and restart prompts.
- Authentication with Argon2 users, JWT sessions, and admin/operator/viewer
  roles.
- mDNS discovery, optional Snapcast mDNS advertising, and subnet scanning.
- Linux installer, systemd service, restart sudoers rule, Docker server flow,
  GitHub release packages, and macOS/Windows Desktop Agent builds.
- Client playback through CPAL with dedicated audio thread, output ring buffer,
  underrun crossfade, hotplug retry, and output prefill.
- Local-time structured logs and UI log filtering by recent time window.

## Known unresolved challenges

### Low-latency playback reliability

Sonium can still stutter at lower `buffer_ms` values on some setups. Recent work
added client output prefill and manual `chunk_ms`, but the system still needs
more telemetry and real-world tuning before it behaves as confidently as mature
multiroom systems at 500-800 ms buffers.

### Automatic buffer tuning

Today users tune `buffer_ms`, client latency, and `chunk_ms` manually. A better
experience would observe jitter, underruns, stale drops, device callback timing,
and network behavior, then recommend or automatically adjust safe values while
still allowing manual override.

### Clock sync validation

The NTP-like software sync path works in tests, but Sonium has not yet been
validated across enough real devices, operating systems, DACs, and network
conditions to claim stable sub-millisecond sync.

### Source diagnostics

Radio streams and ffmpeg processes can fail for many reasons: server disconnects,
HTTP stalls, codec changes, stderr-only failures, DNS issues, TLS errors, or
upstream silence. Sonium now restarts `pipe://` sources, but it still needs a
proper operator-facing source health view.

### Safe configuration reloads

Some config changes still require a full server restart. The UI now prompts for
that and the installer can grant a narrow restart permission, but Sonium should
eventually support more partial reloads without dropping clients.

### Packaging and upgrade hardening

Release packaging works, but upgrade paths are still young. Older systemd units,
manual installs, and distro differences can miss permissions or dependencies.
The installer and Desktop Agent need more migration checks.

### Snapcast compatibility

Sonium has a migration path and optional Snapcast mDNS advertising, but full
drop-in protocol compatibility with every Snapcast client/version is not
guaranteed.

## Near-term roadmap

1. **Playback stability**

   - Add richer health telemetry for output buffer depth, underruns, stale
     chunk drops, jitter, and source restarts.
   - Improve client-side adaptive output prefill.
   - Use telemetry to warn when a stream buffer is too low for the current
     network/device.

2. **Stream tuning UX**

   - Add an optional automatic mode for `buffer_ms` and `chunk_ms`.
   - Keep manual controls for advanced users and debugging.
   - Document practical tuning recipes for radio, local files, Bluetooth
     sinks, and low-latency LAN tests.

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

5. **Hardware validation**

   - Test mixed Linux/macOS/Windows clients on the same LAN.
   - Validate Raspberry Pi and USB DAC behavior.
   - Measure drift and sync error with real microphones/loopback capture.

## Longer-term roadmap

- PTPv2/hardware timestamp support through the existing `TimeSource` abstraction.
- Cross-subnet relays and remote-site streaming.
- TLS/HTTPS and stronger deployment profiles.
- More source integrations: AirPlay, Spotify Connect, MPD/library workflows.
- Better per-room DSP, normalization, and calibration tools.
- More polished installers and auto-update flows for the Desktop Agent.

## Production-readiness bar

Before calling Sonium production-ready, we should be able to demonstrate:

- Stable playback for many hours from common sources, including internet radio.
- Predictable behavior at documented buffer targets.
- Clear admin diagnostics for every common failure mode.
- Safe upgrades across at least one previous minor version.
- Measured sync behavior on real multi-device hardware.
- Recovery from server restart, network drop, client sleep/wake, and device
  hotplug without manual cleanup.
