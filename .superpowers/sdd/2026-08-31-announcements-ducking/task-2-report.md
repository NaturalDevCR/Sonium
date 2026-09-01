# Task 2 — Server scheduling and client duck envelope

## Scope delivered

- Added a deterministic server-side scheduler over the bounded Task 1 coordinator. It assigns an absolute server `scheduled_at_ms`, advances `scheduled` / `started` / terminal ACK deadlines, expires stale intents, fails dropped acknowledgements, and schedules the next queued intent without requiring REST traffic.
- Added bounded, additive intent metadata to `AnnouncementControlV1`: source URI, priority, attack/release duck parameters, expiry, and resume policy. The field is optional with a serde default, so existing V1 payloads still decode and older JSON readers can ignore the new field.
- Routed Type 11 controls through a dedicated bounded server broadcast to the matching media-session group. A reconnecting client receives the current pending control with its original timestamp; offline clients are recovered by the scheduler ACK timeout.
- Added client Type 11 ACK handling on the existing TCP control writer. Server sessions reject acknowledgements carrying intent metadata or naming a group other than the authenticated session group.
- Added a one-slot client duck envelope owned by the network/control task. Attack, hold, completion, cancellation, and release timing run outside CPAL. The audio callback performs only one relaxed atomic gain load per buffer and sample multiplication; it does not lock or evaluate envelope state for ducking.
- Preserved the running music program and sync buffer while ducking. Terminal controls start release from the gain actually published to CPAL, duplicate terminal controls do not restart release, and server resume transitions remain exactly-once.
- Covered higher-priority interruption, bounded lower-priority drops, same-priority server queue handoff, cancellation during attack, terminal-before-start, dropped and late ACKs, expiry, reconnect replay, offline timeout, group routing, and wire compatibility.

## RED → GREEN evidence

| Cycle | RED result | GREEN result |
|---|---|---|
| Additive wire metadata | Protocol integration test did not compile because intent metadata types and `AnnouncementControlV1::intent` did not exist. | Metadata round-trip, legacy V1 decode, and invalid/unbounded metadata tests passed after the additive contract and validation were implemented. |
| Deterministic scheduler | Control integration test did not compile because `announcement_scheduler` did not exist. | Timestamp, priority, queue, cancel, expiry, ACK-deadline, reconnect, and offline-tick tests passed after scheduler/state integration. |
| Client envelope | Client integration test did not compile because `DuckEnvelope` and `DuckGain` did not exist. | Deterministic attack/hold/release, preemption, drop, cancellation, and exactly-once completion tests passed. |
| CPAL boundary | Player unit test did not compile because callback gain helpers did not exist. | All `i16`, `u16`, and `f32` callback paths load one precomputed atomic gain and scale output after rendering. |
| Terminal before start | Regression failed with gain `0.55` instead of `1.0`: the client inferred attack progress that had never been published. | Terminal handling now releases from the currently published atomic gain until `Started` has occurred. |
| Late ACK after timeout | Scheduler regression returned `InvalidLifecycle` after the operation had already failed and advanced the queue. | ACKs for an already-terminal group are idempotent and emit no duplicate transition/control. |
| Server-ordered queue handoff | `Cancelled(old)` followed by equal-priority `Scheduled(next)` was acknowledged as `Failed`. | A completed/cancelled release may hand off to the server-selected next intent without a second cancel ACK or an unbounded client queue. |

## Final verification

- `cargo test -p sonium-protocol -p sonium-control -p sonium-client -p sonium-server --no-fail-fast` — passed: 222 tests, 0 failed; one existing ignored doc-test.
- `cargo fmt --all -- --check` — passed.
- `git diff --check` — passed.
- `cargo clippy -p sonium-protocol -p sonium-control -p sonium-client -p sonium-server --all-targets -- -D warnings` — blocked only by three pre-existing warnings: `items_after_test_module` in `crates/control/src/auth.rs`, plus `result_large_err` and `needless_borrow` in `crates/control/src/config_api.rs`.
- The same Clippy command with only those three pre-existing lints allowed passed with no warnings in Task 2 code.

## Main files

- `crates/protocol/src/messages/announcement.rs`
- `crates/control/src/announcement_scheduler.rs`
- `crates/control/src/state.rs`
- `server/src/main.rs`
- `server/src/session.rs`
- `client/src/ducking.rs`
- `client/src/controller.rs`
- `client/src/player.rs`
- `crates/protocol/tests/protocol_wire.rs`
- `crates/control/tests/announcement_scheduler.rs`
- `client/tests/ducking.rs`

## Decisions and boundaries

- Server queueing remains bounded by the Task 1 per-group coordinator. The client intentionally has capacity for one active envelope; unexpected lower/equal-priority controls fail rather than create another queue.
- The scheduler has no internal clock or timer. Production supplies a 50 ms wall-clock tick; tests inject exact millisecond timestamps.
- Source URI metadata is transported and validated in this task. Fetching/mixing announcement media is not added to the CPAL callback.
- No Home Assistant adapter or full announcement UI was added.
