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
