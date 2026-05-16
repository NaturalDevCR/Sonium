# Sonium Home Assistant Integration

A Home Assistant integration for [Sonium](https://github.com/your-org/sonium), a Snapcast-compatible multiroom audio server. Exposes groups, zones, and clients as Home Assistant entities with real-time updates via WebSocket.

## Features

- **Media Players** for each group (zone) and each client (speaker)
- **Source selection** — assign a stream to a group, or move a client between groups
- **Volume & mute** control per client
- **Grouping** — move speakers into zones via the HA media player grouping feature
- **Stream status sensors** — detect when a stream is playing, idle, or in error
- **Client health sensors** — jitter, buffer depth, and underrun count per speaker
- **Connected binary sensor** — per-speaker online/offline state
- **Zone select** — change a speaker's zone directly
- **Latency offset** — fine-tune audio sync per speaker (±1000 ms)
- **Real-time updates** via WebSocket (events propagate instantly)
- **Domain services** — create/delete/rename groups, rename clients

## Installation

### Via HACS (recommended)

1. In Home Assistant, open **HACS → Integrations → ⋮ → Custom Repositories**
2. Add this repository URL and select category **Integration**
3. Search for **Sonium** and install
4. Restart Home Assistant

### Manual

1. Copy the `custom_components/sonium/` folder to your `<config>/custom_components/` directory
2. Restart Home Assistant

## Configuration

1. Go to **Settings → Devices & Services → Add Integration**
2. Search for **Sonium**
3. Enter your server details:
   - **Host** — IP address or hostname of the Sonium server
   - **Port** — Control port (default: `1711`)
   - **Use HTTPS/WSS** — Enable if behind an HTTPS reverse proxy
   - **Username / Password** — A Sonium account with at least `operator` role

## Entities

### Per Group (zone)
| Platform | Description |
|---|---|
| `media_player` | Zone player — source = active stream, group_members = clients in zone |

### Per Client (speaker)
| Platform | Description |
|---|---|
| `media_player` | Speaker player — volume, mute, source = current zone |
| `binary_sensor` | Connected — `on` when the client is connected |
| `select` | Zone — dropdown to move the speaker to a different zone |
| `number` | Latency Offset — audio sync trim in milliseconds |
| `sensor` | Jitter — audio jitter in ms (requires health telemetry enabled on client) |
| `sensor` | Buffer Depth — jitter buffer depth in ms |
| `sensor` | Underruns — cumulative audio underrun count |

### Per Stream
| Platform | Description |
|---|---|
| `sensor` | Status — `playing`, `idle`, or `error` |

## Services

| Service | Description |
|---|---|
| `sonium.rename_client` | Set a client's display name |
| `sonium.rename_group` | Rename a group/zone |
| `sonium.create_group` | Create a new group and assign it a stream |
| `sonium.delete_group` | Delete a group (clients move to `default`) |

## Notes

- Health sensors (`Jitter`, `Buffer Depth`, `Underruns`) show `unknown` until health telemetry is enabled for the client in the Sonium web UI (Settings → Client → Enable Health Telemetry).
- New clients and groups that appear after HA startup are picked up automatically via the WebSocket event stream.
- The integration uses `operator` role API calls. If you use a `viewer` account, write operations (volume, group change, etc.) will fail.
