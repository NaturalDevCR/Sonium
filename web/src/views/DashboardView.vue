<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useRouter } from 'vue-router';
import { useServerStore } from '@/stores/server';
import { useAuthStore }   from '@/stores/auth';
import { api }            from '@/lib/api';
import StreamBadge        from '@/components/StreamBadge.vue';
import VolumeControl      from '@/components/VolumeControl.vue';
import LevelMeter         from '@/components/LevelMeter.vue';
import HealthStatus       from '@/components/HealthStatus.vue';
import SyncIndicator      from '@/components/SyncIndicator.vue';
import ExpertToggle       from '@/components/ExpertToggle.vue';
import type { Client } from '@/lib/api';

const store  = useServerStore();
const auth   = useAuthStore();
const router = useRouter();

onMounted(async () => {
  await store.loadAll();
  store.startLiveUpdates();
});
onUnmounted(() => store.stopLiveUpdates());

// ── Expert mode ────────────────────────────────────────────────────────────
const expertMode = ref(localStorage.getItem('sonium-expert-mode') === 'true');

// ── Grouped view ───────────────────────────────────────────────────────────
const groupedClients = computed(() =>
  store.groups.map(g => ({
    group:   g,
    clients: g.client_ids.map(id => store.clientsById[id]).filter(Boolean),
    stream:  store.streamsById[g.stream_id],
  })),
);

// ── Sync health (mock for now — will be real when API provides it)
const syncHealth = computed(() => {
  const health: Record<string, { status: 'good' | 'fair' | 'poor' | 'unknown'; drift_ms: number }> = {};
  for (const c of store.connectedClients) {
    // Derive from health report jitter when available
    const jitter = c.health?.jitter_ms ?? 0;
    if (jitter === 0) {
      health[c.id] = { status: 'unknown', drift_ms: 0 };
    } else if (jitter < 10) {
      health[c.id] = { status: 'good', drift_ms: jitter };
    } else if (jitter < 50) {
      health[c.id] = { status: 'fair', drift_ms: jitter };
    } else {
      health[c.id] = { status: 'poor', drift_ms: jitter };
    }
  }
  return health;
});

const overallSync = computed(() => {
  const clients = store.connectedClients;
  if (clients.length === 0) return { status: 'unknown' as const, issueCount: 0, totalCount: 0 };
  const healths = clients.map(c => syncHealth.value[c.id]?.status ?? 'unknown');
  const poor = healths.filter(h => h === 'poor').length;
  const fair = healths.filter(h => h === 'fair').length;
  if (poor > 0) return { status: 'poor' as const, issueCount: poor + fair, totalCount: clients.length };
  if (fair > 0) return { status: 'fair' as const, issueCount: fair, totalCount: clients.length };
  if (healths.every(h => h === 'good')) return { status: 'good' as const, issueCount: 0, totalCount: clients.length };
  return { status: 'unknown' as const, issueCount: 0, totalCount: clients.length };
});

function groupSyncStatus(clients: Client[]) {
  const connected = clients.filter(c => c.status === 'connected');
  if (connected.length === 0) return { status: 'unknown' as const, issueCount: 0, totalCount: 0 };
  const healths = connected.map(c => syncHealth.value[c.id]?.status ?? 'unknown');
  const poor = healths.filter(h => h === 'poor').length;
  const fair = healths.filter(h => h === 'fair').length;
  if (poor > 0) return { status: 'poor' as const, issueCount: poor + fair, totalCount: connected.length };
  if (fair > 0) return { status: 'fair' as const, issueCount: fair, totalCount: connected.length };
  if (healths.every(h => h === 'good')) return { status: 'good' as const, issueCount: 0, totalCount: connected.length };
  return { status: 'unknown' as const, issueCount: 0, totalCount: connected.length };
}

// ── Volume (debounced) ─────────────────────────────────────────────────────
const pendingVolume = ref<Record<string, { volume: number; muted: boolean }>>({});
const debounceTimers: Record<string, ReturnType<typeof setTimeout>> = {};

function setVolume(clientId: string, volume: number, muted: boolean) {
  pendingVolume.value[clientId] = { volume, muted };
  clearTimeout(debounceTimers[clientId]);
  debounceTimers[clientId] = setTimeout(async () => {
    const v = pendingVolume.value[clientId];
    if (!v) return;
    delete pendingVolume.value[clientId];
    await api.setVolume(clientId, v.volume, v.muted);
  }, 120);
}

function clientVolume(clientId: string) {
  return pendingVolume.value[clientId] ?? {
    volume: store.clientsById[clientId]?.volume ?? 100,
    muted:  store.clientsById[clientId]?.muted  ?? false,
  };
}

// ── Group master volume ────────────────────────────────────────────────────
async function setGroupVolume(clients: Client[], volume: number, muted: boolean) {
  await Promise.all(clients.map(c => api.setVolume(c.id, volume, muted)));
  await store.loadAll();
}

function groupVolume(clients: Client[]) {
  if (clients.length === 0) return { volume: 100, muted: false };
  const avg = Math.round(clients.reduce((s, c) => s + c.volume, 0) / clients.length);
  return { volume: avg, muted: clients.every(c => c.muted) };
}

// ── Stream change ──────────────────────────────────────────────────────────
async function setGroupStream(groupId: string, streamId: string) {
  await api.setGroupStream(groupId, streamId);
}

// ── Move client ────────────────────────────────────────────────────────────
async function moveClient(clientId: string, groupId: string) {
  await api.setGroup(clientId, groupId);
}

// ── Create group modal ─────────────────────────────────────────────────────
const showNewGroup   = ref(false);
const newGroupName   = ref('');
const newGroupStream = ref('');

async function createGroup() {
  if (!newGroupName.value || !newGroupStream.value) return;
  await api.createGroup(newGroupName.value, newGroupStream.value);
  showNewGroup.value  = false;
  newGroupName.value  = '';
  newGroupStream.value = '';
  await store.loadAll();
}

// ── Uptime ─────────────────────────────────────────────────────────────────
function fmtUptime(s: number) {
  if (!s) return '—';
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}
</script>

<template>
  <div class="dash-root safe-top">

    <!-- ── Top bar ──────────────────────────────────────────────────────── -->
    <header class="dash-header">
      <div class="dash-header-inner">
        <div class="flex items-center gap-3">
          <img src="/sonium-logo.png" alt="Sonium" class="h-8 w-8 object-contain" />
          <div>
            <p class="dash-brand">SONIUM <span class="version-tag">v0.1.78</span></p>
            <p class="dash-tagline">
              {{ store.connectedClients.length }} client{{ store.connectedClients.length !== 1 ? 's' : '' }} online
              <span v-if="store.uptime" class="ml-2 opacity-60">· up {{ fmtUptime(store.uptime) }}</span>
            </p>
          </div>
        </div>

        <div class="flex items-center gap-2">
          <SyncIndicator
            :status="overallSync.status"
            :issue-count="overallSync.issueCount"
            :total-count="overallSync.totalCount"
            @click="router.push('/sync')"
          />
          <ExpertToggle />
          <button
            @click="router.push('/matrix')"
            class="dash-icon-btn"
            title="Audio Matrix"
          >
            <span class="mdi mdi-grid text-lg"></span>
          </button>
          <button
            v-if="auth.isAdmin"
            @click="router.push('/admin')"
            class="dash-icon-btn"
            title="Admin panel"
          >
            <span class="mdi mdi-cog-outline text-lg"></span>
          </button>
          <button
            @click="auth.logout(); router.push('/login')"
            class="dash-icon-btn dash-icon-btn-danger"
            title="Sign out"
          >
            <span class="mdi mdi-logout text-lg"></span>
          </button>
        </div>
      </div>
    </header>

    <!-- ── Content ──────────────────────────────────────────────────────── -->
    <main class="dash-main safe-bottom">

      <!-- Skeleton -->
      <div v-if="store.loading" class="space-y-3 px-4 pt-4">
        <div v-for="i in 2" :key="i" class="card p-4 animate-pulse space-y-3">
          <div class="h-4 rounded-lg w-1/3" style="background: var(--bg-elevated);"></div>
          <div class="h-10 rounded-lg" style="background: var(--bg-elevated);"></div>
          <div class="h-4 rounded-lg w-2/3" style="background: var(--bg-elevated);"></div>
        </div>
      </div>

      <!-- Groups -->
      <template v-else>
        <div class="px-4 pt-4 space-y-3 pb-24">

          <div
            v-for="{ group, clients, stream } in groupedClients"
            :key="group.id"
            class="group-card"
          >
            <!-- Group header -->
            <div class="group-card-header">
              <div class="flex items-center gap-2.5 min-w-0">
                <div class="group-avatar">
                  <span class="mdi mdi-speaker-multiple text-xs" style="color: var(--accent);"></span>
                </div>
                <div class="min-w-0">
                  <h2 class="font-semibold truncate" style="font-size: 14px; color: var(--text-primary);">{{ group.name }}</h2>
                  <p style="font-size: 11px; color: var(--text-muted);">
                    {{ clients.filter(c => c.status === 'connected').length }}/{{ clients.length }} online
                  </p>
                </div>
              </div>

              <div class="flex items-center gap-2 shrink-0">
                <SyncIndicator
                  v-if="clients.length > 1"
                  :status="groupSyncStatus(clients).status"
                  :issue-count="groupSyncStatus(clients).issueCount"
                  :total-count="groupSyncStatus(clients).totalCount"
                  @click="router.push('/sync')"
                />
                <LevelMeter
                  v-if="stream"
                  :rms-db="store.streamLevels[stream.id] ?? -90"
                />
                <StreamBadge v-if="stream" :status="stream.status" :codec="stream.codec" />
              </div>
            </div>

            <!-- Stream selector (operator+) -->
            <div v-if="auth.isOperator" class="px-4 py-2 border-b" style="border-color: var(--border); display: flex; align-items: center; gap: 8px;">
              <span class="mdi mdi-music-box text-sm" style="color: var(--text-muted);"></span>
              <select
                :value="group.stream_id"
                @change="setGroupStream(group.id, ($event.target as HTMLSelectElement).value)"
                class="dash-select flex-1"
              >
                <option v-for="s in store.streams" :key="s.id" :value="s.id">
                  {{ s.display_name || s.id }}
                </option>
              </select>
            </div>

            <!-- Group master volume (2+ clients) -->
            <div
              v-if="auth.isOperator && clients.length > 1"
              class="px-4 py-3 border-b"
              style="border-color: var(--border);"
            >
              <p class="section-label mb-2">Group volume</p>
              <VolumeControl
                :volume="groupVolume(clients).volume"
                :muted="groupVolume(clients).muted"
                :compact="true"
                @update:volume="setGroupVolume(clients, $event, groupVolume(clients).muted)"
                @update:muted="setGroupVolume(clients, groupVolume(clients).volume, $event)"
              />
            </div>

            <!-- Clients -->
            <div v-if="clients.length === 0" class="px-4 py-4 text-center" style="font-size: 12.5px; color: var(--text-muted); font-style: italic;">
              No clients assigned to this group
            </div>

            <div
              v-for="client in clients" :key="client.id"
              class="client-item"
              :style="client.status !== 'connected' ? 'opacity: 0.4;' : ''"
            >
              <div class="flex items-center justify-between gap-2 mb-2">
                <div class="flex items-center gap-2 min-w-0">
                  <span
                    class="w-1.5 h-1.5 rounded-full shrink-0"
                    :class="client.status === 'connected' ? 'pulse-dot' : ''"
                    :style="{ background: client.status === 'connected' ? 'var(--green)' : 'var(--text-muted)' }"
                  ></span>
                  <span class="font-medium truncate" style="font-size: 13px; color: var(--text-primary);">
                    {{ client.hostname }}
                  </span>
                  <span v-if="client.latency_ms" style="font-size: 11px; color: var(--text-muted); font-family: var(--font-mono);">
                    +{{ client.latency_ms }}ms
                  </span>
                  <span v-if="client.status !== 'connected'" style="font-size: 11px; color: var(--text-muted); font-style: italic;">offline</span>
                </div>

                <div class="flex items-center gap-2">
                  <HealthStatus v-if="client.status === 'connected'" :health="client.health" />
                  
                  <!-- Expert: sync drift badge -->
                  <span
                    v-if="expertMode && client.status === 'connected' && syncHealth[client.id]"
                    class="text-xs font-mono px-1.5 py-0.5 rounded"
                    :style="{
                      background: syncHealth[client.id].status === 'good' ? 'rgba(34,197,94,0.15)' :
                                  syncHealth[client.id].status === 'fair' ? 'rgba(245,158,11,0.15)' :
                                  syncHealth[client.id].status === 'poor' ? 'rgba(239,68,68,0.15)' :
                                  'var(--bg-elevated)',
                      color: syncHealth[client.id].status === 'good' ? '#22c55e' :
                             syncHealth[client.id].status === 'fair' ? '#f59e0b' :
                             syncHealth[client.id].status === 'poor' ? '#ef4444' :
                             'var(--text-muted)',
                    }"
                  >
                    {{ syncHealth[client.id].drift_ms.toFixed(1) }}ms
                  </span>

                  <select
                    v-if="auth.isOperator && store.groups.length > 1"
                    :value="client.group_id"
                    @change="moveClient(client.id, ($event.target as HTMLSelectElement).value)"
                    class="dash-select-xs"
                  >
                    <option v-for="g in store.groups" :key="g.id" :value="g.id">{{ g.name }}</option>
                  </select>
                </div>
              </div>

              <!-- Volume slider -->
              <VolumeControl
                v-if="auth.isOperator"
                :volume="clientVolume(client.id).volume"
                :muted="clientVolume(client.id).muted"
                :compact="true"
                @update:volume="setVolume(client.id, $event, clientVolume(client.id).muted)"
                @update:muted="setVolume(client.id, clientVolume(client.id).volume, $event)"
              />
              <!-- Read-only volume bar for viewers -->
              <div v-else class="flex items-center gap-2">
                <span class="mdi text-base" style="color: var(--text-muted);"
                  :class="client.muted ? 'mdi-volume-off' : 'mdi-volume-high'"></span>
                <div class="flex-1 h-1 rounded-full" style="background: var(--bg-elevated);">
                  <div
                    class="h-full rounded-full"
                    style="background: var(--accent);"
                    :style="{ width: client.volume + '%' }"
                  ></div>
                </div>
                <span style="font-family: var(--font-mono); font-size: 11px; color: var(--text-muted); width: 28px; text-align: right;">
                  {{ client.volume }}%
                </span>
              </div>
            </div>
          </div>

          <!-- Empty state -->
          <div v-if="groupedClients.length === 0" class="text-center py-20">
            <span class="mdi mdi-speaker-off text-5xl block mb-4" style="color: var(--text-muted);"></span>
            <p style="color: var(--text-muted); font-size: 14px;">No groups configured</p>
            <router-link
              v-if="auth.isAdmin"
              to="/admin/groups"
              style="font-size: 13px; color: var(--accent); margin-top: 6px; display: inline-block;"
            >
              Go to admin panel →
            </router-link>
          </div>
        </div>
      </template>
    </main>

    <!-- ── FAB: new group (admin) ────────────────────────────────────────── -->
    <button
      v-if="auth.isAdmin"
      @click="showNewGroup = true"
      class="dash-fab"
      title="New group"
    >
      <span class="mdi mdi-plus text-2xl"></span>
    </button>

    <!-- ── New group modal ────────────────────────────────────────────────── -->
    <Teleport to="body">
      <Transition name="fade">
        <div
          v-if="showNewGroup"
          class="dialog-overlay"
          @click.self="showNewGroup = false"
        >
          <div class="card-elevated w-full max-w-sm p-5 space-y-4 anim-slide-up" style="border-color: var(--border-mid);">
            <h3 style="font-family: var(--font-display); font-size: 17px; font-weight: 700; color: var(--text-primary);">New group</h3>

            <div>
              <label class="section-label mb-2 block">Name</label>
              <input v-model="newGroupName" type="text" placeholder="Living Room" class="field" />
            </div>

            <div>
              <label class="section-label mb-2 block">Stream</label>
              <select v-model="newGroupStream" class="field">
                <option value="" disabled>Select a stream…</option>
                <option v-for="s in store.streams" :key="s.id" :value="s.id">{{ s.display_name || s.id }}</option>
              </select>
            </div>

            <div class="flex gap-2.5 pt-1">
              <button @click="showNewGroup = false" class="btn-ghost flex-1 justify-center">Cancel</button>
              <button
                @click="createGroup"
                :disabled="!newGroupName || !newGroupStream"
                class="btn-primary flex-1 justify-center"
              >
                Create
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>

  </div>
</template>

<style scoped>
.dash-root {
  min-height: 100vh;
  background: var(--bg-base);
  position: relative;
}

/* Top bar */
.dash-header {
  position: sticky;
  top: 0;
  z-index: 20;
  background: rgba(4, 8, 15, 0.88);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-bottom: 1px solid var(--border);
}
.dash-header-inner {
  max-width: 720px;
  margin: 0 auto;
  padding: 12px 16px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.dash-brand {
  font-family: var(--font-display);
  font-size: 13px;
  font-weight: 800;
  letter-spacing: 0.14em;
  color: var(--text-primary);
  line-height: 1;
}
.dash-tagline {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 1px;
}
.version-tag {
  font-size: 9px;
  opacity: 0.4;
  font-weight: 400;
  letter-spacing: 0;
  vertical-align: middle;
  margin-left: 4px;
}

/* Content */
.dash-main { max-width: 720px; margin: 0 auto; }

/* Group cards */
.group-card {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  overflow: hidden;
  transition: border-color 0.2s;
}
.group-card:hover { border-color: var(--border-mid); }

.group-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px;
  gap: 12px;
  border-bottom: 1px solid var(--border);
}

.group-avatar {
  width: 30px;
  height: 30px;
  border-radius: 8px;
  background: var(--accent-dim);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.client-item {
  padding: 11px 16px;
  border-top: 1px solid var(--border);
  transition: background 0.15s;
}
.client-item:hover { background: var(--bg-hover); }

.dash-select {
  font-size: 12.5px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  border-radius: 7px;
  padding: 5px 8px;
  color: var(--text-secondary);
  cursor: pointer;
  outline: none;
}
.dash-select:focus { border-color: var(--accent); }

.dash-select-xs {
  font-size: 11.5px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  border-radius: 6px;
  padding: 3px 7px;
  color: var(--text-secondary);
  cursor: pointer;
  outline: none;
  max-width: 110px;
  flex-shrink: 0;
}
.dash-select-xs:focus { border-color: var(--accent); }

/* Icon buttons */
.dash-icon-btn {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
  background: transparent;
  border: none;
  cursor: pointer;
  transition: all 0.15s;
}
.dash-icon-btn:hover {
  background: var(--bg-elevated);
  color: var(--text-secondary);
}
.dash-icon-btn-danger:hover {
  background: rgba(239, 68, 68, 0.1);
  color: #ef4444;
}

/* FAB */
.dash-fab {
  position: fixed;
  bottom: 24px;
  right: 24px;
  width: 52px;
  height: 52px;
  border-radius: 14px;
  background: linear-gradient(135deg, #0ea5e9, #38bdf8);
  color: #fff;
  border: none;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 0 24px var(--accent-glow-lg), 0 4px 16px rgba(0,0,0,0.4);
  transition: transform 0.15s ease, box-shadow 0.15s ease;
  z-index: 30;
}
.dash-fab:hover {
  transform: scale(1.06) translateY(-2px);
  box-shadow: 0 0 36px rgba(56, 189, 248, 0.4), 0 6px 20px rgba(0,0,0,0.4);
}
.dash-fab:active { transform: scale(0.96); }

/* Modal overlay */
.dialog-overlay {
  position: fixed;
  inset: 0;
  z-index: 50;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  padding: 16px;
  background: rgba(2, 5, 10, 0.8);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
}
@media (min-width: 640px) {
  .dialog-overlay { align-items: center; }
}

.fade-enter-active, .fade-leave-active { transition: opacity 0.2s; }
.fade-enter-from, .fade-leave-to { opacity: 0; }
</style>
