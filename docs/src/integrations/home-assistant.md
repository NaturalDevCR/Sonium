# Home Assistant Integration

Sonium includes a HACS-compatible Home Assistant integration under
`custom_components/sonium`. It connects to the Sonium control API and WebSocket
event stream, then exposes rooms, speakers, streams, and health signals as Home
Assistant entities.

## Current status

The integration is useful for local testing and home-lab automation, but should
track Sonium's early-project warning: APIs, auth flows, and health fields may
still change between releases.

## Features

- Group/zone `media_player` entities with source selection.
- Per-client speaker `media_player` entities for volume, mute, and group moves.
- Stream status sensors for `playing`, `idle`, and `error`.
- Client connected binary sensors.
- Client health sensors for jitter, buffer depth, underruns, and related
  telemetry when health reporting is enabled.
- Zone selector and latency offset controls per speaker.
- Domain services to rename clients/groups and create/delete groups.
- Bounded announcement and ducking support for automations and Music Assistant
  media-player calls.
- Real-time updates from `/api/events`.

## Installation

### HACS

1. In Home Assistant, open **HACS -> Integrations -> Custom repositories**.
2. Add `https://github.com/NaturalDevCR/Sonium` and choose category
   **Integration**.
3. Install **Sonium** and restart Home Assistant.

### Manual

Copy `custom_components/sonium/` into
`<home-assistant-config>/custom_components/sonium/`, then restart Home
Assistant.

## Configuration

In Home Assistant, go to **Settings -> Devices & Services -> Add Integration**,
search for **Sonium**, and enter:

| Field | Value |
| --- | --- |
| Host | Sonium server hostname or IP |
| Port | Control port, usually `1711` |
| HTTPS/WSS | Enable only when Sonium is behind an HTTPS reverse proxy |
| Username/password | A Sonium account with at least `operator` role |

Viewer accounts can read state, but write operations such as volume, group
changes, renames, and group creation require an operator/admin-capable account.

## Announcements and ducking

`sonium.play_announcement` schedules an authenticated, bounded announcement
through the Sonium control API. Target a zone by its Sonium `group_ids`, or by
using `target_entity_ids` containing Sonium group/client `media_player`
entities. The service requires an `idempotency_key`: preserve it when retrying
the same automation action so a network retry cannot replay audio.

```yaml
action: sonium.play_announcement
data:
  source: "https://home.example/local/doorbell.ogg"
  group_ids: ["living_room"]
  idempotency_key: "doorbell-{{ trigger.id }}-{{ trigger.to_state.last_changed.timestamp() }}"
  priority: announcement
  attenuation_db: -18
  attack_ms: 25
  release_ms: 150
  max_duration_ms: 15000
  resume: true
```

Sonium also accepts the Home Assistant/Music Assistant announcement convention
on its group and client entities: invoke `media_player.play_media` with
`announce: true`, a `media_content_id` that is an `http`, `https`, or `media`
URI, and optional `extra` metadata (`priority`, `duck`, `release_ms`,
`max_duration_ms`, `resume`, and `idempotency_key`). An idempotency key is
generated when omitted, so automations that need retry-safe behaviour should
provide one explicitly.

Sonium schedules the request and expires it after `max_duration_ms`; it does
not implement or depend on the Sendspin protocol. In this release the control
plane and synchronized ducking are implemented, but clients do not yet fetch
and decode `source` directly; provide announcement audio through an existing
Sonium stream/source. Direct URI playback remains a follow-up. Cancellation is
available as `sonium.cancel_announcement` with the server announcement ID.
