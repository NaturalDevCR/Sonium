# Task 2 Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve the last client-published duck gain on delayed natural completion and make every bounded server control enqueue outcome explicit.

**Architecture:** The duck envelope starts both explicit and natural releases from the atomically published gain at the current tick time. The server routes bounded control writes through one policy helper: decoder-critical frames fail the session on `Full` or `Closed`, while supersedable timing frames explicitly report coalescing on `Full` and fail on `Closed`.

**Tech Stack:** Rust, Tokio bounded MPSC, Sonium protocol framing, Cargo tests

**Spec:** Re-review findings supplied for Task 2 on 2026-09-01.

## Global Constraints

- Use strict RED/GREEN TDD for both findings.
- Do not delegate work.
- Run the 232+ workspace suite, focal tests, `cargo fmt`, and `cargo clippy` before commit.
- Preserve wire-format decoding assertions for queued control frames.

---

### Task 1: Delayed natural duck release

**Files:**
- Modify: `client/src/ducking.rs`
- Test: `client/tests/ducking.rs`

**Interfaces:**
- Consumes: `DuckEnvelope::tick(now_ms)` and `DuckGain::load()`.
- Produces: a natural `Release { started_at_ms: now_ms, start_gain: self.gain.load() }`.

- [x] **Step 1: Write the failing delayed-tick regression**

Add a test that publishes gain `0.55` during attack, delays the next tick past natural completion, asserts no gain jump at that tick, then asserts release interpolation from `0.55` over the configured 200 ms.

- [x] **Step 2: Verify RED**

Run: `cargo test -p sonium-client --test ducking delayed_natural_completion`

Expected: FAIL because the current implementation reconstructs and backdates release from the unpublished end-of-attack gain.

- [x] **Step 3: Implement the minimum fix**

In the natural-completion branch, load the published scalar once, set `started_at_ms` to `now_ms`, and use that scalar as `start_gain`.

- [x] **Step 4: Verify GREEN**

Run: `cargo test -p sonium-client --test ducking`

Expected: all ducking integration tests pass.

### Task 2: Bounded server control policy

**Files:**
- Modify and test: `server/src/session.rs`

**Interfaces:**
- Consumes: `mpsc::Sender<Vec<u8>>::try_send`.
- Produces: `ControlQueueResult::{Enqueued, Coalesced}` and a helper that returns an error for critical `Full`/all `Closed`, but `Coalesced` for supersedable `Full`.

- [x] **Step 1: Write failing queue-policy tests**

Add tests that fill/close a capacity-one channel and assert critical CodecHeader/ServerSettings sends return errors, supersedable Time/GroupSync sends return explicit coalescing only on `Full`, and enqueued frames still decode with the expected protocol message type.

- [x] **Step 2: Verify RED**

Run: `cargo test -p sonium-server session::tests::bounded_control`

Expected: compilation/test failure because the explicit policy API does not exist.

- [x] **Step 3: Implement and apply the policy**

Route initial/switch CodecHeader and all ServerSettings through the critical path and propagate failures so the session disconnects/retries. Route Time and GroupSync through the supersedable path, log explicit coalescing, and propagate a closed writer as a session error.

- [x] **Step 4: Verify GREEN and regressions**

Run: `cargo test -p sonium-server session::tests`

Expected: all session tests pass.

### Task 3: Full verification and commit

**Files:**
- Verify all modified files above.

**Interfaces:**
- Consumes: completed Task 1 and Task 2 changes.
- Produces: formatted, lint-clean, committed review fixes.

- [x] **Step 1: Format and lint**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Result: global Clippy reached pre-existing warnings in `crates/control`; strict
`--no-deps` Clippy passed for all client/server targets changed by this plan.

- [x] **Step 2: Run full tests**

Run: `cargo test --workspace --all-targets --all-features`

Expected: the complete 232+ suite passes with zero failures.

- [x] **Step 3: Review and commit**

Inspect `git diff`, stage only the plan and Task 2 review files, and commit with `fix(audio): preserve critical control state`.
