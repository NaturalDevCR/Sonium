# Sonium Stability Phase 1 Design

## Purpose

Make the server safe to operate on a trusted production LAN by removing the
known P0 failures in identity, configuration, persistence, source recovery
and resource exhaustion. This phase does not claim Internet-safe media
transport or Snapcast-equivalent timing; those are later, separately verified
phases.

## Decisions

1. **Fail closed for persistent identity and configuration.** A missing users
   file is a first-run condition. An existing unreadable or invalid users file
   is fatal and must not create an administrator or overwrite data. Explicit
   configuration files must parse and validate or prevent startup.
2. **Make session validity user-scoped and durable.** Each user has a session
   version. JWTs carry that version, and verification requires a current,
   enabled user with the same role and version. Password, role and deletion
   changes invalidate all earlier sessions. The signing secret and account
   file use atomic replace and restrictive permissions.
3. **Make all externally supplied identity bounded.** Client IDs have a small
   validated character set and length. The server places a global cap on active
   sessions and does not persist or export metric labels for unknown arbitrary
   identifiers indefinitely.
4. **Do not let a producer restart silence a stream.** File/FIFO readers use
   a supervised reopen loop with capped exponential backoff. State exposes
   recovering versus failed; a reader error never masquerades as idle.
5. **Generate one canonical strict configuration shape.** The installer writes
   `[server.audio]`; serde rejects unknown fields, and semantic validation
   checks ports, format and buffer/chunk relations before binding sockets.
6. **Secure control-plane defaults.** The control listener follows an explicit
   `server.control_bind`, defaulting to loopback. Metrics stay loopback by
   default. Tokens are not accepted through URL query parameters in Sonium's
   web client; WebSocket authentication is a short-lived protocol message in a
   later compatibility-preserving task.

## Non-goals

- Replacing TCP media with mutually authenticated TLS/QUIC.
- Lock-free device callback and RTP/ARQ/FEC redesign.
- Signed release artifacts, full supported-platform matrix and long-running
  fault-injection certification.

## Acceptance criteria

- Corrupt account/config files fail without replacing persisted data.
- Credential/role/deletion changes invalidate every existing token for that
  user, including after restart.
- Generated install configuration is accepted exactly as written and unsupported
  keys fail startup.
- A FIFO writer disconnect/reconnect recovers automatically without restarting
  the server.
- Invalid client IDs and active-client capacity are rejected with tests.
- Control and metrics defaults do not expose an unauthenticated interface on
  all network adapters.
