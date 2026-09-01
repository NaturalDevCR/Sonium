# Music Assistant / Announcements

Sonium models announcements as a first-class playback intent rather than a
temporary replacement stream. The server admits an announcement against one or
more groups, schedules it, and each client applies a bounded duck envelope to
its current program while the announcement is active. See the
[announcement and ducking design](./../architecture/announcement-ducking-design.md)
for the full contract.

This page describes the announcement capability as it exists in the control
API and client. It is written from the perspective of a Music Assistant-style
caller that wants to play a chime or message over music.

## Authenticated announcement service

Announcements are submitted through the control-plane REST API and require an
authenticated account with at least the `operator` role. The announcement
routes live on the write router and are gated by `require_operator`
(`crates/control/src/api.rs`). A bounded body limit (16 KB) applies so an
authenticated caller cannot create an oversized control payload.

| Route | Method | Purpose |
| --- | --- | --- |
| `/announcements` | `POST` | Admit an announcement intent |
| `/announcements` | `GET` | List announcement records (also expires stale intents) |
| `/announcements/:id` | `DELETE` | Cancel an announcement |
| `/announcements/:id/lifecycle` | `POST` | Acknowledge a lifecycle transition per group |

The intent schema (`crates/control/src/announcements.rs`) is versioned
(`version = 1`), and every accepted intent carries an `idempotency_key` so the
same announcement can be replayed safely.

## Announcement lifecycle

Each announcement moves through a bounded set of lifecycle states
(`AnnouncementLifecycle`): `scheduled`, `started`, `completed`, `cancelled`,
and `failed`. Clients must acknowledge transitions; a missing acknowledgement
or an expired intent causes the operation to be cancelled, and cancellation or
expiry restores the prior music program exactly once per group. Terminal
records keep their idempotency keys until evicted oldest-first.

## Ducking parameters

Ducking is described by the `Ducking` struct and validated on admission:

| Field | Constraint |
| --- | --- |
| `attenuation_db` | `-60..=0` dB (must be finite) |
| `attack_ms` | `<= 5000` |
| `release_ms` | `<= 5000` |

Attack and release ramps are applied in the client's audio path, outside the
real-time callback, so the envelope does not block audio processing.

## Priority and idempotency

Priorities are ordered `music < chime < announcement < emergency`. A higher
priority interrupts lower-priority audio; equal priority is queued. Per-group
limits keep announcements bounded: queue depth (16), queued duration budget
(10 minutes per group), maximum intent duration (120 s), and a global retained
record cap (1024). Replaying the same `idempotency_key` with an identical
intent returns the existing admission marked as a duplicate; reusing a key
with a different intent is rejected.

## Home Assistant integration

The in-tree Home Assistant integration (`custom_components/sonium`) connects to
the Sonium control API and WebSocket event stream and exposes the announcement
operation as domain services backed by the authenticated REST endpoint.

### `sonium.play_announcement`

Registers an announcement with `custom_components/sonium/__init__.py`
(`handle_play_announcement`). It builds an intent with
`build_announcement_intent` (`custom_components/sonium/announcement.py`) and
submits it via `POST /api/announcements`.

Service data:

| Field | Notes |
| --- | --- |
| `source` | Required `http(s)`/`media` URI of the announcement audio |
| `group_ids` or `target_entity_ids` | Target Sonium groups or Sonium media player entity IDs |
| `idempotency_key` | Required replay key (1..=128 bytes) |
| `priority` | `music`, `chime`, `announcement` (default), or `emergency` |
| `attenuation_db` | Default `-18`, range `-60..=0` |
| `attack_ms` | Default `25`, `0..=5000` |
| `release_ms` | Default `100`, `0..=5000` |
| `max_duration_ms` | Default `30_000`, `1..=120_000` |
| `resume` | Default `true` (restore previous program after completion) |

Targets are resolved through the entity registry: a group entity maps to its
group, a client entity maps to its current group. Invalid, unknown, or
duplicate targets raise a Home Assistant error. Validation errors from the
adapter surface as `HomeAssistantError` before any request is sent.

### `sonium.cancel_announcement`

Cancels an active or queued announcement (`handle_cancel_announcement` in
`__init__.py`, backed by `DELETE /api/announcements/:id`). Takes a single
`announcement_id`.

### `media_player` announcement metadata

`announcement_options_from_media_kwargs` in `announcement.py` turns Home
Assistant media metadata into announcement options. An announcement is
recognized only when the `announce` flag is set to `true`; a normal
`play_media` call is deliberately not treated as an announcement. The call
looks for options in `metadata`, `extra`, and top-level media kwargs, honoring
`priority`, `duck` (with `attenuation_db`/`attack_ms`/`release_ms`),
`resume`, `max_duration_ms`, and `idempotency_key`. An idempotency key is
generated when omitted; automations that need retry-safe behaviour should
provide one explicitly. The group `media_player` entity targets the group's
zone; the client entity targets its current group.

## Sendspin

Sendspin was analyzed for design patterns only. It is **not integrated** —
Sonium does not depend on Sendspin or emulate its wire format.
