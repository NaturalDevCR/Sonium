# Sonium Announcements and Ducking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add authenticated, bounded, observable announcements with synchronized ducking for Music Assistant-compatible control.

**Architecture:** Control state owns an idempotent priority queue per group. The
server schedules accepted intents and sends timestamped control/media metadata;
clients apply attack/hold/release envelopes locally without blocking their
real-time callback. Home Assistant maps its announcement service and media
metadata to the same API.

**Tech Stack:** Rust protocol/control/server/client, Vue, Home Assistant Python, Tokio.

**Spec:** `docs/src/architecture/announcement-ducking-design.md`

## Global Constraints

- No unbounded announcement queues or audio allocations.
- Every operation is authenticated, idempotent and has an expiry.
- Existing music playback resumes exactly once after completion/cancellation.
- Do not introduce Sendspin protocol compatibility or a Sendspin dependency.
- Add real tests before implementation for queue, priority, timing and recovery.

---

### Task 1: Intent protocol and bounded control state

Define versioned announcement messages, queue limits, priority arbitration,
idempotency and observable lifecycle transitions. Add REST/WS API and tests.

### Task 2: Server scheduling and client duck envelope

Schedule server timestamps, deliver intent transitions, implement bounded
client-side attack/release ducking outside the audio callback, and test timing,
interruption, resume and dropped acknowledgements.

### Task 3: Music Assistant/Home Assistant adapter

Expose `sonium.play_announcement`, map media-player announcement metadata,
handle cancellation/expiry and add Python tests/docs.

### Task 4: UI, metrics and conformance

Show active/queued announcements, expose lifecycle/duck metrics, add loopback
and soak scenarios, and document compatibility boundaries.
