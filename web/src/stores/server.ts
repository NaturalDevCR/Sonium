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
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
    }
  }

  // ── WebSocket event handlers ───────────────────────────────────────────
  function applyEvent(event: Event) {
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

      case 'eq_changed':
        clients.value = clients.value.map((c) =>
          c.id === event.client_id ? { ...c, eq_bands: event.eq_bands } : c,
        );
        break;
    }
  }

  // ── Live updates via WebSocket ─────────────────────────────────────────
  let wsClose: (() => void) | null = null;

  function startLiveUpdates() {
    wsClose = subscribeEvents(applyEvent, () => {
      setTimeout(() => { loadAll(); startLiveUpdates(); }, 3000);
    });
  }

  function stopLiveUpdates() {
    wsClose?.();
    wsClose = null;
  }

  return {
    clients, groups, streams, uptime, loading, error, streamLevels,
    connectedClients, clientsById, streamsById,
    loadAll, applyEvent, startLiveUpdates, stopLiveUpdates,
  };
});
