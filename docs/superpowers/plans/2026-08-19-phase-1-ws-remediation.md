# Sonium Phase 1 WebSocket Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace WebSocket JWT handoff with bounded one-use tickets and remove
the client-state deletion deadlock.

**Architecture:** The control auth store owns ticket issuance and atomic
consumption. The HTTP API validates tickets before upgrade; browser and Home
Assistant clients pass only the ticket through WebSocket subprotocols. State
removal captures the event under lock and performs persistence/hooks after the
lock is released.

**Tech Stack:** Rust, Axum, Tokio, Vue, Home Assistant aiohttp.

**Spec:** `docs/superpowers/specs/2026-08-19-phase-1-ws-remediation.md`

## Global Constraints

- No JWT, password or reusable bearer secret in a URL, WebSocket message or log.
- Tickets are random, bounded, one-use, short-lived and session-version-bound.
- Validate/consume a ticket before upgrading a socket.
- Add real regression tests for replay, expiry, revocation and state deletion.

---

### Task 1: Ticketed WebSocket admission and state deletion

**Files:**
- Modify: `crates/control/src/auth.rs`, `crates/control/src/api.rs`,
  `crates/control/src/state.rs`, `web/src/lib/api.ts`, web socket client files,
  `custom_components/sonium/*.py`
- Test: affected Rust and Python/web test locations

- [ ] Write failing tests for ticket replay, expiry, session invalidation,
  pre-upgrade rejection and persisted client deletion.
- [ ] Implement minimal bounded ticket issuance/consumption, HTTP endpoint,
  subprotocol admission and client migration.
- [ ] Revalidate connected claims on event emission and release state locks
  before persistence/hooks.
- [ ] Run focused tests, format, type checks and commit.
