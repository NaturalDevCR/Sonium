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

- Group/zone `media_player` entities with source selection and group mute/unmute.
- Per-client speaker `media_player` entities for individual volume, mute, and
  group moves. Group mute fans out to members while preserving each client's
  independent volume.
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

Home Assistant's own announcement convention (`media_player.play_media` with
`announce: true`, used by `tts.speak` and Music Assistant) plays the audio
directly, as described below.

### Playing announcements, TTS and URLs (0.1.93+)

`media_player.play_media` (with or without `announce: true`), `tts.speak` and
the media browser now **play the audio** on the target speaker or zone through
`POST /api/media/play`, then every speaker returns to its previous source and
volume. The server decodes the URL with `ffmpeg` (install it on the Sonium
host; the Docker image includes it), waits for the first decoded audio before
switching (so a broken URL or slow TTS never interrupts the music), and plays
it on the normal synchronized timeline. Optional `extra: {volume: 0.0–1.0}`
(or 0–100) sets a temporary volume. `media_player.media_stop` cancels it, and
selecting a zone source interrupts it. The Sonium server must be able to reach
Home Assistant's URL (TTS/media-source links are made absolute with HA's
internal URL).

```yaml
action: tts.speak
target:
  entity_id: tts.google_translate_en_com
data:
  media_player_entity_id: media_player.kitchen
  message: "Dinner is ready"
```

```yaml
action: media_player.play_media
target:
  entity_id: media_player.living_room
data:
  media_content_id: "http://192.168.1.10:8123/local/doorbell.mp3"
  media_content_type: music
  announce: true
  extra:
    volume: 0.6
```

#### Duck or replace, and routing

By default the music keeps playing **under** the announcement (`duck` mode):
it fades down, the announcement plays on top, and the music fades back up,
without interrupting playback. Use `replace` mode to silence the music
completely while the announcement plays. Set the default in `sonium.toml`
(`[announcements] mode = "duck"` or `"replace"`) and override it per call:

```yaml
action: media_player.play_media
target:
  entity_id: [media_player.kitchen, media_player.patio]   # route to these only
data:
  media_content_id: "http://192.168.1.10:8123/local/doorbell.mp3"
  media_content_type: music
  announce: true
  extra:
    mode: replace          # or duck
    volume: 0.6            # optional temporary volume
    duck_db: -24           # duck mode only: how much to lower the music
    attack_ms: 150         # fade-down time
    release_ms: 600        # fade-up time
```

Routing is per call: target any speakers (client players) and/or zones
(group players). Only those play the announcement; every other zone keeps
its music untouched. Speakers whose music is idle simply play the
announcement.

`sonium.play_announcement` keeps its duck-only behaviour: it schedules
synchronized ducking but does not fetch `source`; provide that audio through an
existing Sonium stream. Cancellation is available as
`sonium.cancel_announcement` with the server announcement ID. Sonium does not
implement or depend on the Sendspin protocol.

Other improvements: zeroconf discovery (`_sonium-http._tcp`), zone volume
(shifts all speakers, keeps their balance), volume step, transparent re-login
when the 24 h token expires, and `media_player.join` moves the listed speakers
into the leader's zone.
