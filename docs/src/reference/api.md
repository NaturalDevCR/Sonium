# REST Control API

> **Status:** Planned for Fase 7.  This page documents the intended API design.

The Sonium server exposes a REST + WebSocket API on port `1780` (configurable).
The built-in web UI is served from the same port.

## Base URL

```
http://<server-host>:1780/api
```

## Authentication

No authentication by default.  Token-based auth is planned for Fase 10.

---

## Endpoints

### Server

#### `GET /api/status`

Returns overall server health and version.

```json
{
  "version":  "0.1.0",
  "uptime_s": 3600,
  "streams":  1,
  "clients":  3,
  "groups":   1
}
```

---

### Streams

#### `GET /api/streams`

```json
[
  {
    "id":     "default",
    "status": "playing",
    "codec":  "opus",
    "format": "48000Hz/16bit/2ch"
  }
]
```

---

### Clients

#### `GET /api/clients`

```json
[
  {
    "id":        "living-room-pi-1",
    "hostname":  "living-room-pi",
    "connected": true,
    "volume":    85,
    "muted":     false,
    "latency_ms": 0,
    "group_id":  "living-room"
  }
]
```

#### `PATCH /api/clients/{id}/volume`

```json
{ "volume": 75, "muted": false }
```

#### `PATCH /api/clients/{id}/latency`

```json
{ "latency_ms": 150 }
```

#### `PATCH /api/clients/{id}/group`

```json
{ "group_id": "kitchen" }
```

---

### Groups

#### `GET /api/groups`

```json
[
  {
    "id":        "living-room",
    "name":      "Living Room",
    "stream_id": "default",
    "client_ids": ["living-room-pi-1", "bookshelf-pi-1"]
  }
]
```

#### `POST /api/groups`

```json
{ "name": "Kitchen", "stream_id": "default" }
```

#### `DELETE /api/groups/{id}`

Returns `204 No Content`.

#### `PATCH /api/groups/{id}/stream`

```json
{ "stream_id": "spotify" }
```

---

## WebSocket events

Connect to `ws://<server>:1780/api/events` to receive real-time notifications.

Events are JSON objects with a `type` field:

```json
{ "type": "client_connected",    "client_id": "living-room-pi-1" }
{ "type": "client_disconnected", "client_id": "living-room-pi-1" }
{ "type": "volume_changed",      "client_id": "living-room-pi-1", "volume": 80, "muted": false }
{ "type": "group_changed",       "group_id":  "living-room", "stream_id": "spotify" }
{ "type": "stream_status",       "stream_id": "default", "status": "playing" }
```

---

## JSON-RPC 2.0 compatibility

For compatibility with existing Snapcast tooling, a subset of the Snapcast
JSON-RPC API is available at `ws://<server>:1780/jsonrpc`.

This allows tools like [snapdroid](https://github.com/badaix/snapdroid) and
[snapweb](https://github.com/badaix/snapweb) to work with Sonium without
modification.
