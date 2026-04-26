# REST Control API

The Sonium server exposes a REST + WebSocket API on port `1711` (configurable
via `control_port` in `server.toml`).  The built-in web UI is served from the
same port.

## Base URL

```
http://<server-host>:1711/api
```

## Authentication

No authentication by default.  Token-based auth is planned for Fase 10.

---

## Endpoints

### Server status

#### `GET /api/status`

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
    "codec":  "opus",
    "format": "48000Hz/16bit/2ch",
    "status": "playing"
  }
]
```

`status` is one of `"playing"`, `"idle"`, `"error"`.

---

### Clients

#### `GET /api/clients`

Returns all known clients (connected and disconnected).

```json
[
  {
    "id":               "living-room-pi-1",
    "hostname":         "living-room-pi",
    "client_name":      "Living Room",
    "os":               "linux",
    "arch":             "aarch64",
    "remote_addr":      "192.168.1.42:12345",
    "volume":           85,
    "muted":            false,
    "latency_ms":       0,
    "group_id":         "default",
    "status":           "connected",
    "connected_at":     "2024-11-01T12:00:00Z",
    "protocol_version": 2
  }
]
```

#### `PATCH /api/clients/{id}/volume`

Set volume (0–100) and mute state.

```json
{ "volume": 75, "muted": false }
```

Returns `204 No Content`.

#### `PATCH /api/clients/{id}/latency`

Set client-side playout latency in milliseconds.

```json
{ "latency_ms": 150 }
```

Returns `204 No Content`.

#### `PATCH /api/clients/{id}/group`

Move a client to a different group.

```json
{ "group_id": "kitchen" }
```

Returns `204 No Content`.

---

### Groups

#### `GET /api/groups`

```json
[
  {
    "id":         "default",
    "name":       "default",
    "stream_id":  "default",
    "client_ids": ["living-room-pi-1", "bookshelf-pi-1"]
  }
]
```

#### `POST /api/groups`

Create a new group.

```json
{ "name": "Kitchen", "stream_id": "default" }
```

Returns `201 Created` with:

```json
{ "id": "3f2a…" }
```

#### `DELETE /api/groups/{id}`

Delete a group.  The built-in `"default"` group cannot be deleted.

Returns `204 No Content`.

#### `PATCH /api/groups/{id}/stream`

Assign a stream to a group.

```json
{ "stream_id": "spotify" }
```

Returns `204 No Content`.

---

## Real-time WebSocket events

Connect to `ws://<server>:1711/api/events` to receive real-time push
notifications whenever state changes.  The web UI uses this endpoint to update
without polling.

Events are newline-delimited JSON objects with a `type` discriminant:

### Client events

```json
{ "type": "client_connected",    "client": { …full Client object… } }
{ "type": "client_disconnected", "client_id": "living-room-pi-1" }
{ "type": "volume_changed",      "client_id": "living-room-pi-1", "volume": 80, "muted": false }
{ "type": "latency_changed",     "client_id": "living-room-pi-1", "latency_ms": 50 }
{ "type": "client_group_changed","client_id": "living-room-pi-1", "group_id": "kitchen" }
```

### Group events

```json
{ "type": "group_created",       "group": { …full Group object… } }
{ "type": "group_deleted",       "group_id": "kitchen" }
{ "type": "group_stream_changed","group_id": "kitchen", "stream_id": "spotify" }
```

### Stream events

```json
{ "type": "stream_status", "stream_id": "default", "status": "playing" }
```

### Heartbeat

Sent every 5 seconds:

```json
{ "type": "heartbeat", "uptime_s": 3600 }
```

---

## mDNS discovery

The server advertises itself on the local network via mDNS so clients can
find it without manual configuration.  Three service types are announced:

| Service type | Port | Purpose |
|---|---|---|
| `_sonium._tcp` | `1710` | Sonium audio stream protocol |
| `_sonium-http._tcp` | `1711` | Web UI + REST API |
| `_snapcast._tcp` | configurable | Only when `snapcast_compat = true` — lets legacy Snapcast clients discover this server |

The TXT record includes `version=<server-version>`.

---

## Subnet scanner

For cross-subnet discovery, the server can scan CIDR ranges specified in
`server.toml` under `[server.discovery]`:

```toml
[server.discovery]
subnets = ["10.0.1.0/24", "10.0.2.0/24"]
```

The scanner probes each host on the Sonium control port (`1711`) with a
concurrent TCP connect, then records responding hosts as `DiscoveredServer`
entries.  A `/16` CIDR (65 536 hosts) is the maximum supported range.
