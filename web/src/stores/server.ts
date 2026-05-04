import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import { api, subscribeEvents, type Client, type Group, type Stream, type Event } from '@/lib/api';

export const useServerStore = defineStore('server', () => {
  // ── State ──────────────────────────────────────────────────────────────
  const clients      = ref<Client[]>([]);
  const groups       = ref<Group[]>([]);
  const streams      = ref<Stream[]>([]);
  const uptime       = ref(0);
  const loading      = ref(true);
  const error        = ref<string | null>(null);
  const streamLevels = ref<Record<string, number>>({});

  // Connection & server health
  const connected    = ref(false);     // WebSocket open?
  const connecting   = ref(false);     // Currently trying?
  const serverOnline = ref(true);      // HTTP reachable?
  const serverVersion= ref<string>('');
  const lastEventAt  = ref<number>(0);

  // ── Getters ────────────────────────────────────────────────────────────
  const connectedClients = computed(() => clients.value.filter((c) => c.status === 'connected'));

  const clientsById = computed(() =>
    Object.fromEntries(clients.value.map((c) => [c.id, c])),
  );

  const streamsById = computed(() =>
    Object.fromEntries(streams.value.map((s) => [s.id, s])),
  );

  // ── Bootstrap ──────────────────────────────────────────────────────────
  async function loadAll() {
    loading.value = true;
    error.value   = null;
    try {
      const [c, g, s] = await Promise.all([api.clients(), api.groups(), api.streams()]);
      clients.value = c;
      groups.value  = g;
      streams.value = s;
      serverOnline.value = true;
    } catch (e) {
      error.value = String(e);
      serverOnline.value = false;
    } finally {
      loading.value = false;
    }
  }

  async function fetchStatus() {
    try {
      const st = await api.status();
      uptime.value = st.uptime_s;
      serverVersion.value = st.version;
      serverOnline.value = true;
      return true;
    } catch {
      serverOnline.value = false;
      return false;
    }
  }

  // ── WebSocket event handlers ───────────────────────────────────────────
  function applyEvent(event: Event) {
    lastEventAt.value = Date.now();
    switch (event.type) {
      case 'client_connected': {
        const idx = clients.value.findIndex((c) => c.id === event.client.id);
        if (idx >= 0) clients.value[idx] = event.client;
        else clients.value.push(event.client);
        break;
      }
      case 'client_disconnected':
        clients.value = clients.value.map((c) =>
          c.id === event.client_id ? { ...c, status: 'disconnected' } : c,
        );
        break;

      case 'client_deleted':
        clients.value = clients.value.filter((c) => c.id !== event.client_id);
        groups.value  = groups.value.map((g) => ({
          ...g,
          client_ids: g.client_ids.filter((id) => id !== event.client_id),
        }));
        break;

      case 'client_renamed':
        clients.value = clients.value.map((c) =>
          c.id === event.client_id ? { ...c, display_name: event.display_name || null } : c,
        );
        break;

      case 'volume_changed':
        clients.value = clients.value.map((c) =>
          c.id === event.client_id ? { ...c, volume: event.volume, muted: event.muted } : c,
        );
        break;

      case 'latency_changed':
        clients.value = clients.value.map((c) =>
          c.id === event.client_id ? { ...c, latency_ms: event.latency_ms } : c,
        );
        break;

      case 'client_observability_changed':
        clients.value = clients.value.map((c) =>
          c.id === event.client_id
            ? { ...c, observability_enabled: event.enabled, health: event.enabled ? c.health : null }
            : c,
        );
        break;

      case 'client_group_changed':
        clients.value = clients.value.map((c) =>
          c.id === event.client_id ? { ...c, group_id: event.group_id } : c,
        );
        groups.value = groups.value.map((g) => ({
          ...g,
          client_ids: g.id === event.group_id
            ? [...g.client_ids, event.client_id]
            : g.client_ids.filter((id) => id !== event.client_id),
        }));
        break;

      case 'group_created':
        groups.value.push(event.group);
        break;

      case 'group_deleted':
        groups.value = groups.value.filter((g) => g.id !== event.group_id);
        break;

      case 'group_renamed':
        groups.value = groups.value.map((g) =>
          g.id === event.group_id ? { ...g, name: event.name } : g,
        );
        break;

      case 'group_stream_changed':
        groups.value = groups.value.map((g) =>
          g.id === event.group_id ? { ...g, stream_id: event.stream_id } : g,
        );
        break;

      case 'stream_status':
        streams.value = streams.value.map((s) =>
          s.id === event.stream_id ? { ...s, status: event.status as Stream['status'] } : s,
        );
        break;

      case 'heartbeat':
        uptime.value = event.uptime_s;
        break;

      case 'stream_level':
        streamLevels.value = { ...streamLevels.value, [event.stream_id]: event.rms_db };
        break;

      case 'stream_eq_changed':
        streams.value = streams.value.map((s) =>
          s.id === event.stream_id
            ? { ...s, eq_bands: event.eq_bands, eq_enabled: event.enabled }
            : s,
        );
        break;
      case 'client_health':
        clients.value = clients.value.map((c) =>
          c.id === event.client_id ? { ...c, health: event.health } : c,
        );
        break;
    }
  }

  // ── Live updates via WebSocket ─────────────────────────────────────────
  let wsClose: (() => void) | null = null;
  let liveRequested = false;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let healthPollTimer: ReturnType<typeof setInterval> | null = null;
  let reconnectAttempt = 0;

  const MAX_BACKOFF_MS = 30000;

  function startLiveUpdates() {
    liveRequested = true;
    if (wsClose) return;
    if (connecting.value) return;
    connecting.value = true;
    connected.value = false;

    wsClose = subscribeEvents(
      (e) => {
        connecting.value = false;
        connected.value = true;
        reconnectAttempt = 0;
        applyEvent(e);
      },
      () => {
        wsClose = null;
        connected.value = false;
        connecting.value = false;
        if (liveRequested) scheduleReconnect();
      },
    );
  }

  function scheduleReconnect() {
    if (reconnectTimer) return;
    reconnectAttempt++;
    const delay = Math.min(1000 * Math.pow(1.5, reconnectAttempt - 1), MAX_BACKOFF_MS);
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      startLiveUpdates();
    }, delay);
  }

  function stopLiveUpdates() {
    liveRequested = false;
    wsClose?.();
    wsClose = null;
    connected.value = false;
    connecting.value = false;
    if (reconnectTimer) { clearTimeout(reconnectTimer); reconnectTimer = null; }
    if (healthPollTimer) { clearInterval(healthPollTimer); healthPollTimer = null; }
  }

  /** Call when the app mounts to begin both WS + health polling */
  function init() {
    loadAll();
    fetchStatus();
    startLiveUpdates();
    if (healthPollTimer) clearInterval(healthPollTimer);
    healthPollTimer = setInterval(async () => {
      const wasOffline = !serverOnline.value;
      const ok = await fetchStatus();
      if (ok && wasOffline) {
        // Server just came back (e.g. after restart) → reload everything
        await loadAll();
      }
      // If we haven't received an event in 30s but HTTP is fine, restart WS
      if (ok && connected.value && Date.now() - lastEventAt.value > 30000) {
        wsClose?.();
        wsClose = null;
        connected.value = false;
        startLiveUpdates();
      }
    }, 5000);
  }

  return {
    clients, groups, streams, uptime, loading, error, streamLevels,
    connected, connecting, serverOnline, serverVersion, lastEventAt,
    connectedClients, clientsById, streamsById,
    loadAll, fetchStatus, applyEvent, startLiveUpdates, stopLiveUpdates, init,
  };
});
