/**
 * Sonium REST API client.
 *
 * All functions target the same origin as the web UI (i.e. the sonium-server
 * control port).  During development the Vite proxy forwards `/api` to
 * `http://localhost:1711`.
 */

const BASE = '/api';

// ── Types ─────────────────────────────────────────────────────────────────

export interface Client {
  id:               string;
  hostname:         string;
  client_name:      string;
  os:               string;
  arch:             string;
  remote_addr:      string;
  volume:           number;
  muted:            boolean;
  latency_ms:       number;
  group_id:         string;
  status:           'connected' | 'disconnected';
  connected_at:     string;
  protocol_version: number;
}

export interface Group {
  id:         string;
  name:       string;
  stream_id:  string;
  client_ids: string[];
}

export interface Stream {
  id:     string;
  codec:  string;
  format: string;
  status: 'playing' | 'idle' | 'error';
}

export interface ServerStatus {
  version:  string;
  uptime_s: number;
  clients:  number;
  groups:   number;
  streams:  number;
}

export type Event =
  | { type: 'client_connected';    client: Client }
  | { type: 'client_disconnected'; client_id: string }
  | { type: 'volume_changed';      client_id: string; volume: number; muted: boolean }
  | { type: 'latency_changed';     client_id: string; latency_ms: number }
  | { type: 'client_group_changed';client_id: string; group_id: string }
  | { type: 'group_created';       group: Group }
  | { type: 'group_deleted';       group_id: string }
  | { type: 'group_stream_changed';group_id: string; stream_id: string }
  | { type: 'stream_status';       stream_id: string; status: string }
  | { type: 'heartbeat';           uptime_s: number };

// ── HTTP helpers ──────────────────────────────────────────────────────────

async function get<T>(path: string): Promise<T> {
  const r = await fetch(`${BASE}${path}`);
  if (!r.ok) throw new Error(`GET ${path}: ${r.status}`);
  return r.json();
}

async function patch(path: string, body: unknown): Promise<void> {
  const r = await fetch(`${BASE}${path}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!r.ok) throw new Error(`PATCH ${path}: ${r.status}`);
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const r = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!r.ok) throw new Error(`POST ${path}: ${r.status}`);
  return r.json();
}

async function del(path: string): Promise<void> {
  const r = await fetch(`${BASE}${path}`, { method: 'DELETE' });
  if (!r.ok) throw new Error(`DELETE ${path}: ${r.status}`);
}

// ── API functions ─────────────────────────────────────────────────────────

export const api = {
  status:      ()                                           => get<ServerStatus>('/status'),
  clients:     ()                                           => get<Client[]>('/clients'),
  groups:      ()                                           => get<Group[]>('/groups'),
  streams:     ()                                           => get<Stream[]>('/streams'),

  setVolume:   (id: string, volume: number, muted: boolean) =>
    patch(`/clients/${id}/volume`, { volume, muted }),

  setLatency:  (id: string, latency_ms: number) =>
    patch(`/clients/${id}/latency`, { latency_ms }),

  setGroup:    (client_id: string, group_id: string) =>
    patch(`/clients/${client_id}/group`, { group_id }),

  createGroup: (name: string, stream_id: string) =>
    post<{ id: string }>('/groups', { name, stream_id }),

  deleteGroup: (id: string)                              => del(`/groups/${id}`),

  setGroupStream: (group_id: string, stream_id: string)  =>
    patch(`/groups/${group_id}/stream`, { stream_id }),
};

// ── WebSocket event stream ─────────────────────────────────────────────────

export function subscribeEvents(
  onEvent: (e: Event) => void,
  onClose?: () => void,
): () => void {
  const protocol = location.protocol === 'https:' ? 'wss' : 'ws';
  const ws = new WebSocket(`${protocol}://${location.host}/api/events`);

  ws.onmessage = (msg) => {
    try {
      onEvent(JSON.parse(msg.data) as Event);
    } catch {
      console.warn('Failed to parse event:', msg.data);
    }
  };

  ws.onclose = () => onClose?.();
  ws.onerror = (e) => console.error('WS error', e);

  return () => ws.close();
}
