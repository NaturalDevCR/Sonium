# Sonium Stability Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the P0 production blockers in persistence, authentication,
configuration, ingress limits and source recovery.

**Architecture:** The control crate becomes the single owner of strict
configuration and durable identity invariants. The server consumes validated
configuration, applies bounded admission before creating session state, and
supervises stream readers with explicit recovery state.

**Tech Stack:** Rust, Tokio, Serde/TOML, Argon2/JWT, CPAL, cargo test.

**Spec:** `docs/superpowers/specs/2026-08-18-stability-phase-1-design.md`

## Global Constraints

- Preserve protocol compatibility unless a message is explicitly versioned.
- Add a regression test for every corrected audit finding.
- No secrets in logs, URLs or world-readable configuration.
- No silent fallback after an explicit configuration or account-file failure.
- Keep each task independently buildable and run its focused tests before merge.

---

### Task 1: Atomic, fail-closed account persistence and session versions

**Files:**
- Modify: `crates/control/src/auth.rs`
- Modify: `crates/control/src/auth_api.rs`
- Test: `crates/control/src/auth.rs`

**Interfaces:**
- Produces `User.session_version: u64` and JWT `Claims.session_version: u64`.
- Produces a fallible `UserStore::load_or_init(...) -> anyhow::Result<Arc<UserStore>>`.

- [ ] Write tests proving corrupt existing account data returns an error without
  modification; password/role/delete invalidates earlier tokens; persistence
  survives reload.
- [ ] Run the tests and observe failure against current permissive behavior.
- [ ] Add atomic temp-file, fsync and rename persistence with `0600` files and
  `0700` directory; distinguish missing file from invalid existing data.
- [ ] Add session version to users and claims; increment it on credential and
  authorization changes; validate it against the loaded user on every token.
- [ ] Run `cargo test -p sonium-control auth -- --test-threads=1`.

### Task 2: Strict server/client configuration and canonical installer output

**Files:**
- Modify: `crates/common/src/config.rs`
- Modify: `server/src/main.rs`
- Modify: `client/src/main.rs`
- Modify: `install.sh`
- Test: `crates/common/src/config.rs`

**Interfaces:**
- Produces `ServerConfig::from_file(path) -> anyhow::Result<ServerConfig>` and
  `ServerConfig::validate() -> anyhow::Result<()>`.

- [ ] Write tests for unknown TOML fields, malformed explicit files, invalid
  port/buffer/chunk combinations, and the installer audio-table shape.
- [ ] Run focused common-config tests and observe failures.
- [ ] Deny unknown Serde fields, return explicit errors, validate ranges and
  update callers to report startup failure.
- [ ] Move installer values to `[server.audio]` and verify its rendered TOML
  parses through the same configuration type.
- [ ] Run `cargo test -p sonium-common config` and server/client compile checks.

### Task 3: Bounded client identity and secure listener defaults

**Files:**
- Modify: `crates/common/src/config.rs`
- Modify: `server/src/main.rs`
- Modify: `server/src/control_server.rs`
- Modify: `server/src/session.rs`
- Modify: `crates/control/src/state.rs`
- Test: `server/src/session.rs`, `crates/control/src/state.rs`

**Interfaces:**
- Produces `ServerConfig.server.control_bind` and `max_clients`.
- Produces `validate_client_id(&str) -> Result<(), ProtocolError>`.

- [ ] Write tests rejecting oversized/unsafe identifiers and excess sessions.
- [ ] Run focused tests and observe the current acceptance behavior.
- [ ] Add validated configured binds, loopback defaults and global session
  permits; validate the Hello identity before persisting client state.
- [ ] Bound persistent client state and metric-label creation to admitted IDs.
- [ ] Run server/control test suites.

### Task 4: Supervised file/FIFO source recovery

**Files:**
- Modify: `server/src/streamreader.rs`
- Modify: `server/src/main.rs`
- Modify: `crates/control/src/state.rs`
- Test: `server/src/streamreader.rs`

**Interfaces:**
- Produces `StreamStatus::Recovering` and `run_reopening_reader(...)`.

- [ ] Write deterministic reader tests with a temporary FIFO/file replacement
  proving EOF transitions to recovering and resumes output.
- [ ] Run the test and observe current terminal-idle behavior.
- [ ] Implement cancellation-aware reopen with bounded exponential backoff and
  fail status only after configuration/permission errors that cannot recover.
- [ ] Preserve source timestamps and expose recovery/retry context in state.
- [ ] Run streamreader and server tests.

### Task 5: Phase integration and operator documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/src/contributing/roadmap.md`
- Modify: deployment templates affected by Tasks 1–4
- Test: workspace targeted tests and installer/config smoke test

- [ ] Update operating guidance to state the trusted-LAN boundary and supported
  authentication configuration.
- [ ] Add migration notes for existing account files lacking session versions.
- [ ] Run format, Clippy, focused integration tests and a rendered installer
  config parse smoke test.
- [ ] Record remaining Phase 2/3 work: real-time callback, authenticated media,
  UDP hardening, packaging and certification.
