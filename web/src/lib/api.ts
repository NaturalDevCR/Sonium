/**
 * Sonium REST API client.
 *
 * All functions target the same origin as the web UI (i.e. the sonium-server
 * control port).  During development the Vite proxy forwards `/api` to
 * `http://localhost:1711`.
 */

const BASE = '/api';

// ── Auth token ────────────────────────────────────────────────────────────

let _token: string | null = null;
export function setApiToken(t: string | null) { _token = t; }

let _onUnauthorized: (() => void) | null = null;
export function setUnauthorizedHandler(handler: (() => void) | null) {
  _onUnauthorized = handler;
}

let _onError: ((msg: string) => void) | null = null;
export function setErrorHandler(handler: ((msg: string) => void) | null) {
  _onError = handler;
}

function handleUnauthorized() {
  _onUnauthorized?.();
}

function emitError(msg: string) {
  _onError?.(msg);
}

function authHeaders(): Record<string, string> {
  return _token ? { Authorization: `Bearer ${_token}` } : {};
}

// ── Types ─────────────────────────────────────────────────────────────────

export interface UserView {
  id:       string;
  username: string;
  role:     'admin' | 'operator' | 'viewer';
}

export interface HealthReport {
  underrun_count:   number;
  overrun_count:    number;
  stale_drop_count: number;
  buffer_depth_ms:  number;
  jitter_ms:        number;
  latency_ms:       number;
}

export interface Client {
  id:               string;
  hostname:         string;
  display_name?:    string | null;
  observability_enabled: boolean;
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
  health?:          HealthReport | null;
}

export interface Group {
  id:         string;
  name:       string;
  stream_id:  string;
  client_ids: string[];
}

export type FilterType = 'peaking' | 'high_pass' | 'low_pass' | 'low_shelf' | 'high_shelf' | 'notch';

export interface EqBand {
  filter_type: FilterType;
  freq_hz:     number;
  gain_db:     number;
  q:           number;
  slope_db_per_oct?: number;
  enabled:     boolean;
}

export interface Stream {
  id:           string;
  display_name?: string | null;
  codec:        string;
  format:       string;
  source:       string;
  buffer_ms:    number;
  idle_timeout_ms?: number | null;
  silence_on_idle: boolean;
  status:       'playing' | 'idle' | 'error';
  eq_bands?:    EqBand[];
  eq_enabled?:  boolean;
}

export interface ScanResult {
  addr:      string;
  port:      number;
  is_sonium: boolean;
}

export interface ServerStatus {
  version:  string;
  uptime_s: number;
  clients:  number;
  groups:   number;
  streams:  number;
}

export interface DependencyInfo {
  id:           string;
  label:        string;
  binary:       string;
  installed:    boolean;
  version?:     string | null;
  purpose:      string;
  install_hint?: string | null;
  update_hint?:  string | null;
  remove_hint?:  string | null;
}

export interface SystemInfo {
  os:              string;
  arch:            string;
  audio_stack:     string[];
  package_manager?: string | null;
  dependencies:    DependencyInfo[];
}

export interface DependencyActionResult {
  command: string;
  success: boolean;
  status?: number | null;
  stdout: string;
  stderr: string;
}

export interface RestartResponse {
  message: string;
}

export interface ConfigReloadReport {
  added:            string[];
  removed:          string[];
  restarted:        string[];
  unchanged:        string[];
  restart_required: string[];
}

export type Event =
  | { type: 'client_connected';    client: Client }
  | { type: 'client_disconnected'; client_id: string }
  | { type: 'client_deleted';      client_id: string }
  | { type: 'client_renamed';      client_id: string; display_name: string }
  | { type: 'volume_changed';      client_id: string; volume: number; muted: boolean }
  | { type: 'latency_changed';     client_id: string; latency_ms: number }
  | { type: 'client_observability_changed'; client_id: string; enabled: boolean }
  | { type: 'client_group_changed';client_id: string; group_id: string }
  | { type: 'group_created';       group: Group }
  | { type: 'group_deleted';       group_id: string }
  | { type: 'group_renamed';       group_id: string; name: string }
  | { type: 'group_stream_changed';group_id: string; stream_id: string }
  | { type: 'stream_status';       stream_id: string; status: string }
  | { type: 'heartbeat';           uptime_s: number }
  | { type: 'stream_level';        stream_id: string; rms_db: number }
  | { type: 'stream_eq_changed';   stream_id: string; eq_bands: EqBand[]; enabled: boolean }
  | { type: 'client_health';       client_id: string; health: HealthReport };

// ── HTTP helpers ──────────────────────────────────────────────────────────

async function get<T>(path: string): Promise<T> {
  const r = await fetch(`${BASE}${path}`, { headers: authHeaders() });
  if (r.status === 401) handleUnauthorized();
  if (!r.ok) {
    const msg = `${path}: ${r.status} ${r.statusText}`;
    emitError(msg);
    throw new Error(msg);
  }
  return parseJson<T>(r, `GET ${path}`);
}

async function getText(path: string): Promise<string> {
  const r = await fetch(`${BASE}${path}`, { headers: authHeaders() });
  if (r.status === 401) handleUnauthorized();
  if (!r.ok) throw new Error(`GET ${path}: ${r.status}`);
  return r.text();
}

async function patch(path: string, body: unknown): Promise<void> {
  const r = await fetch(`${BASE}${path}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json', ...authHeaders() },
    body: JSON.stringify(body),
  });
  if (r.status === 401) handleUnauthorized();
  if (!r.ok) {
    const msg = `${path}: ${r.status} ${r.statusText}`;
    emitError(msg);
    throw new Error(msg);
  }
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const r = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...authHeaders() },
    body: JSON.stringify(body),
  });
  if (r.status === 401) handleUnauthorized();
  if (!r.ok) throw new Error(`POST ${path}: ${r.status}`);
  return parseJson<T>(r, `POST ${path}`);
}

async function parseJson<T>(response: Response, label: string): Promise<T> {
  const contentType = response.headers.get('content-type') || '';
  if (!contentType.includes('application/json')) {
    const body = await response.text();
    throw new Error(`${label}: expected JSON, got ${contentType || 'unknown content type'} (${body.slice(0, 80)})`);
  }
  return response.json();
}

async function put(path: string, body: string, contentType = 'text/plain'): Promise<void> {
  const r = await fetch(`${BASE}${path}`, {
    method: 'PUT',
    headers: { 'Content-Type': contentType, ...authHeaders() },
    body,
  });
  if (r.status === 401) handleUnauthorized();
  if (!r.ok) throw new Error(`PUT ${path}: ${r.status}`);
}

async function del(path: string): Promise<void> {
  const r = await fetch(`${BASE}${path}`, { method: 'DELETE', headers: authHeaders() });
  if (r.status === 401) handleUnauthorized();
  if (!r.ok) throw new Error(`DELETE ${path}: ${r.status}`);
}

/** POST plain text body; returns the error message on 4xx/5xx, null on success. */
async function postText(path: string, body: string): Promise<string | null> {
  const r = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'text/plain', ...authHeaders() },
    body,
  });
  if (r.status === 401) { handleUnauthorized(); return null; }
  if (!r.ok) return r.text();
  return null;
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

  setObservability: (id: string, enabled: boolean) =>
    patch(`/clients/${id}/observability`, { enabled }),

  setGroup:    (client_id: string, group_id: string) =>
    patch(`/clients/${client_id}/group`, { group_id }),

  setClientName: (id: string, display_name: string | null) =>
    patch(`/clients/${id}/name`, { display_name }),

  setEq:       (id: string, bands: EqBand[], enabled: boolean) =>
    patch(`/streams/${id}/eq`, { bands, enabled }),

  deleteClient: (id: string) => del(`/clients/${id}`),

  createGroup: (name: string, stream_id: string) =>
    post<{ id: string }>('/groups', { name, stream_id }),

  deleteGroup: (id: string)                              => del(`/groups/${id}`),

  renameGroup: (id: string, name: string) =>
    patch(`/groups/${id}`, { name }),

  setGroupStream: (group_id: string, stream_id: string) =>
    patch(`/groups/${group_id}/stream`, { stream_id }),

  // ── Users ───────────────────────────────────────────────────────────────
  users: () => get<UserView[]>('/users'),

  createUser: (username: string, password: string, role: UserView['role']) =>
    post<UserView>('/users', { username, password, role }),

  updateUser: (id: string, data: Partial<{ password: string; role: UserView['role'] }>) =>
    put(`/users/${id}`, JSON.stringify(data), 'application/json'),

  deleteUser: (id: string) => del(`/users/${id}`),

  // ── Config ──────────────────────────────────────────────────────────────
  configRaw:      ()             => getText('/config/raw'),
  saveConfigRaw:  (toml: string) => put('/config/raw', toml, 'text/plain'),
  validateConfig: (toml: string) => postText('/config/validate', toml),
  reloadConfig:   ()             => post<ConfigReloadReport>('/config/reload', {}),

  // ── Discovery ───────────────────────────────────────────────────────────
  scanSubnet: (cidr: string, port = 1710) =>
    get<ScanResult[]>(`/discover/scan?cidr=${encodeURIComponent(cidr)}&port=${port}`),
  localSubnet: () => get<{ cidr: string | null }>('/discover/local-subnet'),

  // ── Auth ────────────────────────────────────────────────────────────────
  logout: () => post<void>('/auth/logout', {}),

  // ── System ──────────────────────────────────────────────────────────────
  systemInfo: () => get<SystemInfo>('/system/info'),
  systemLogs: () => getText('/system/logs'),
  restartServer: () => post<RestartResponse>('/system/restart', {}),
  dependencyAction: (id: string, action: 'install' | 'update' | 'remove') =>
    post<DependencyActionResult>(`/system/dependencies/${id}/${action}`, {}),
};

// ── WebSocket event stream ─────────────────────────────────────────────────

export function subscribeEvents(
  onEvent: (e: Event) => void,
  onClose?: () => void,
): () => void {
  const protocol = location.protocol === 'https:' ? 'wss' : 'ws';
  const tokenParam = _token ? `?token=${encodeURIComponent(_token)}` : '';
  const ws = new WebSocket(`${protocol}://${location.host}/api/events${tokenParam}`);

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
