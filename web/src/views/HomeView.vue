<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useServerStore } from '@/stores/server';
import { useAuthStore } from '@/stores/auth';
import { api } from '@/lib/api';
import StreamBadge from '@/components/StreamBadge.vue';
import VolumeControl from '@/components/VolumeControl.vue';
import LevelMeter from '@/components/LevelMeter.vue';
import HealthStatus from '@/components/HealthStatus.vue';
import type { Client } from '@/lib/api';

const store = useServerStore();
const auth = useAuthStore();

onMounted(() => store.init());
onUnmounted(() => store.stopLiveUpdates());

// ── Stats ────────────────────────────────────────────────────────────────
const stats = computed(() => [
  { label: 'Clients', value: store.connectedClients.length, total: store.clients.length, icon: 'mdi-speaker', color: 'text-emerald-400', bg: 'from-emerald-500/10 to-emerald-500/5', border: 'border-emerald-500/20' },
  { label: 'Groups', value: store.groups.length, icon: 'mdi-speaker-multiple', color: 'text-violet-400', bg: 'from-violet-500/10 to-violet-500/5', border: 'border-violet-500/20' },
  { label: 'Playing', value: store.streams.filter(s => s.status === 'playing').length, total: store.streams.length, icon: 'mdi-play-circle', color: 'text-cyan-400', bg: 'from-cyan-500/10 to-cyan-500/5', border: 'border-cyan-500/20' },
  { label: 'Uptime', value: fmtUptime(store.uptime), icon: 'mdi-clock-outline', color: 'text-amber-400', bg: 'from-amber-500/10 to-amber-500/5', border: 'border-amber-500/20' },
]);

function fmtUptime(s: number) {
  if (!s) return '—';
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  return d > 0 ? `${d}d ${h}h` : h > 0 ? `${h}h ${m}m` : `${m}m`;
}

// ── Grouped view ─────────────────────────────────────────────────────────
const groupedClients = computed(() =>
  store.groups.map(g => ({
    group: g,
    clients: g.client_ids.map(id => store.clientsById[id]).filter(Boolean),
    stream: store.streamsById[g.stream_id],
  })),
);

// ── Volume debounce ──────────────────────────────────────────────────────
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
    muted: store.clientsById[clientId]?.muted ?? false,
  };
}

async function setGroupVolume(clients: Client[], volume: number, muted: boolean) {
  await Promise.all(clients.map(c => api.setVolume(c.id, volume, muted)));
  await store.loadAll();
}

function groupVolume(clients: Client[]) {
  if (clients.length === 0) return { volume: 100, muted: false };
  const avg = Math.round(clients.reduce((s, c) => s + c.volume, 0) / clients.length);
  return { volume: avg, muted: clients.every(c => c.muted) };
}

async function setGroupStream(groupId: string, streamId: string) {
  await api.setGroupStream(groupId, streamId);
}

async function moveClient(clientId: string, groupId: string) {
  await api.setGroup(clientId, groupId);
}

// ── New group modal ──────────────────────────────────────────────────────
const showNewGroup = ref(false);
const newGroupName = ref('');
const newGroupStream = ref('');

async function createGroup() {
  if (!newGroupName.value || !newGroupStream.value) return;
  await api.createGroup(newGroupName.value, newGroupStream.value);
  showNewGroup.value = false;
  newGroupName.value = '';
  newGroupStream.value = '';
  await store.loadAll();
}
</script>

<template>
  <div class="space-y-6 max-w-6xl mx-auto">
    <!-- ── Stats Row ──────────────────────────────────────────────────── -->
    <div class="grid grid-cols-2 lg:grid-cols-4 gap-3">
      <div
        v-for="(s, i) in stats" :key="s.label"
        class="glass glass-hover p-4 animate-fade-up"
        :style="`animation-delay: ${i * 0.05}s; opacity: 0;`"
        :class="`gradient-border bg-gradient-to-br ${s.bg}`"
      >
        <div class="flex items-center justify-between mb-2">
          <span class="mdi text-xl" :class="[s.icon, s.color]"></span>
        </div>
        <div class="text-2xl font-display font-bold text-white tracking-tight">
          {{ s.value }}
          <span v-if="s.total !== undefined && s.total !== null" class="text-sm text-slate-500 font-normal">/ {{ s.total }}</span>
        </div>
        <div class="text-[11px] font-medium text-slate-500 tracking-wide uppercase mt-0.5">{{ s.label }}</div>
      </div>
    </div>

    <!-- ── Loading Skeleton ───────────────────────────────────────────── -->
    <div v-if="store.loading" class="space-y-3">
      <div v-for="i in 2" :key="i" class="glass p-5 space-y-4 animate-pulse">
        <div class="flex items-center gap-3">
          <div class="w-10 h-10 rounded-xl bg-white/[0.04]"></div>
          <div class="flex-1 space-y-2">
            <div class="h-3.5 rounded-lg bg-white/[0.04] w-1/3"></div>
            <div class="h-2.5 rounded-lg bg-white/[0.03] w-1/4"></div>
          </div>
        </div>
        <div class="h-12 rounded-xl bg-white/[0.03]"></div>
      </div>
    </div>

    <!-- ── Groups Grid ────────────────────────────────────────────────── -->
    <div v-else class="grid grid-cols-1 xl:grid-cols-2 gap-4">
      <div
        v-for="({ group, clients, stream }, idx) in groupedClients"
        :key="group.id"
        class="glass glass-hover animate-fade-up"
        :style="`animation-delay: ${idx * 0.08}s; opacity: 0;`"
      >
        <!-- Group Header -->
        <div class="p-5 border-b border-white/[0.06]">
          <div class="flex items-center justify-between gap-3">
            <div class="flex items-center gap-3 min-w-0">
              <div class="w-10 h-10 rounded-xl bg-gradient-to-br from-cyan-500/20 to-violet-500/20 border border-white/[0.08] flex items-center justify-center shrink-0">
                <span class="mdi mdi-speaker-multiple text-cyan-300 text-base"></span>
              </div>
              <div class="min-w-0">
                <h3 class="font-semibold text-white text-[15px] truncate">{{ group.name }}</h3>
                <p class="text-[11px] text-slate-500">
                  {{ clients.filter(c => c.status === 'connected').length }}/{{ clients.length }} online
                </p>
              </div>
            </div>
            <div class="flex items-center gap-2 shrink-0">
              <LevelMeter v-if="stream" :rms-db="store.streamLevels[stream.id] ?? -90" />
              <StreamBadge v-if="stream" :status="stream.status" :codec="stream.codec" />
            </div>
          </div>

          <!-- Stream selector -->
          <div v-if="auth.isOperator" class="mt-3 flex items-center gap-2">
            <span class="mdi mdi-music-box text-slate-500 text-sm"></span>
            <select
              :value="group.stream_id"
              @change="setGroupStream(group.id, ($event.target as HTMLSelectElement).value)"
              class="input-glass text-xs py-1.5 flex-1"
            >
              <option v-for="s in store.streams" :key="s.id" :value="s.id">{{ s.display_name || s.id }}</option>
            </select>
          </div>
        </div>

        <!-- Group Volume -->
        <div v-if="auth.isOperator && clients.length > 1" class="px-5 py-3 border-b border-white/[0.04]">
          <div class="text-[10px] font-bold text-slate-500 uppercase tracking-wider mb-2">Group Volume</div>
          <VolumeControl
            :volume="groupVolume(clients).volume"
            :muted="groupVolume(clients).muted"
            :compact="true"
            @update:volume="setGroupVolume(clients, $event, groupVolume(clients).muted)"
            @update:muted="setGroupVolume(clients, groupVolume(clients).volume, $event)"
          />
        </div>

        <!-- Clients -->
        <div class="p-2">
          <div v-if="clients.length === 0" class="text-center py-6 text-sm text-slate-500 italic">
            No clients in this group
          </div>
          <div
            v-for="client in clients"
            :key="client.id"
            class="flex flex-col gap-2 p-3 rounded-xl transition-all"
            :class="client.status !== 'connected' ? 'opacity-40' : 'hover:bg-white/[0.03]'"
          >
            <div class="flex items-center justify-between gap-2">
              <div class="flex items-center gap-2.5 min-w-0">
                <span
                  class="status-dot shrink-0"
                  :class="client.status === 'connected' ? 'online' : 'offline'"
                ></span>
                <span class="text-sm font-medium text-slate-200 truncate">{{ client.hostname }}</span>
                <span v-if="client.latency_ms" class="text-[10px] text-slate-500 font-mono">+{{ client.latency_ms }}ms</span>
                <span v-if="client.status !== 'connected'" class="text-[10px] text-slate-600 italic">offline</span>
              </div>
              <div class="flex items-center gap-2 shrink-0">
                <HealthStatus v-if="client.status === 'connected'" :health="client.health" />
                <select
                  v-if="auth.isOperator && store.groups.length > 1"
                  :value="client.group_id"
                  @change="moveClient(client.id, ($event.target as HTMLSelectElement).value)"
                  class="input-glass text-[11px] py-1 px-2 w-24"
                >
                  <option v-for="g in store.groups" :key="g.id" :value="g.id">{{ g.name }}</option>
                </select>
              </div>
            </div>
            <VolumeControl
              v-if="auth.isOperator"
              :volume="clientVolume(client.id).volume"
              :muted="clientVolume(client.id).muted"
              :compact="true"
              @update:volume="setVolume(client.id, $event, clientVolume(client.id).muted)"
              @update:muted="setVolume(client.id, clientVolume(client.id).volume, $event)"
            />
            <div v-else class="flex items-center gap-2">
              <span class="mdi text-slate-500 text-sm" :class="client.muted ? 'mdi-volume-off' : 'mdi-volume-high'"></span>
              <div class="flex-1 h-1 rounded-full bg-white/[0.06]">
                <div class="h-full rounded-full bg-gradient-to-r from-cyan-400 to-violet-400" :style="{ width: client.volume + '%' }"></div>
              </div>
              <span class="text-[11px] text-slate-500 font-mono w-7 text-right">{{ client.volume }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- ── Empty State ────────────────────────────────────────────────── -->
    <div v-if="!store.loading && groupedClients.length === 0" class="text-center py-20 animate-fade-up">
      <div class="relative inline-block mb-6">
        <div class="w-20 h-20 rounded-2xl bg-gradient-to-br from-cyan-500/10 to-violet-500/10 border border-white/[0.08] flex items-center justify-center">
          <span class="mdi mdi-speaker-off text-3xl text-slate-600"></span>
        </div>
        <div class="absolute -inset-2 bg-cyan-500/5 blur-xl rounded-full"></div>
      </div>
      <h3 class="text-lg font-semibold text-slate-300 mb-1">No groups yet</h3>
      <p class="text-sm text-slate-500 mb-5">Create your first audio group to get started</p>
      <router-link
        v-if="auth.isAdmin"
        to="/admin/groups"
        class="btn-gradient"
      >
        <span class="mdi mdi-plus"></span>
        Create Group
      </router-link>
    </div>

    <!-- ── FAB ────────────────────────────────────────────────────────── -->
    <button
      v-if="auth.isAdmin"
      @click="showNewGroup = true"
      class="fixed bottom-6 right-6 lg:bottom-8 lg:right-8 w-14 h-14 rounded-2xl bg-gradient-to-br from-cyan-400 to-violet-500 text-white shadow-lg shadow-cyan-500/25 flex items-center justify-center transition-all hover:scale-110 hover:shadow-cyan-500/40 z-30"
      style="bottom: calc(24px + env(safe-area-inset-bottom));"
    >
      <span class="mdi mdi-plus text-2xl"></span>
    </button>

    <!-- ── New Group Modal ────────────────────────────────────────────── -->
    <Teleport to="body">
      <Transition name="modal">
        <div v-if="showNewGroup" class="fixed inset-0 z-50 flex items-end sm:items-center justify-center p-4 bg-black/60 backdrop-blur-sm" @click.self="showNewGroup = false">
          <div class="glass w-full max-w-sm p-6 space-y-5 animate-scale-in">
            <h3 class="text-lg font-display font-bold text-white">New Group</h3>
            <div class="space-y-4">
              <div>
                <label class="text-[10px] font-bold text-slate-500 uppercase tracking-wider mb-1.5 block">Name</label>
                <input v-model="newGroupName" type="text" placeholder="e.g. Living Room" class="input-glass" />
              </div>
              <div>
                <label class="text-[10px] font-bold text-slate-500 uppercase tracking-wider mb-1.5 block">Stream</label>
                <select v-model="newGroupStream" class="input-glass">
                  <option value="" disabled>Select a stream</option>
                  <option v-for="s in store.streams" :key="s.id" :value="s.id">{{ s.display_name || s.id }}</option>
                </select>
              </div>
            </div>
            <div class="flex gap-3 pt-1">
              <button @click="showNewGroup = false" class="btn-glass flex-1 justify-center">Cancel</button>
              <button
                @click="createGroup"
                :disabled="!newGroupName || !newGroupStream"
                class="btn-gradient flex-1 justify-center disabled:opacity-40"
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
.modal-enter-active, .modal-leave-active { transition: opacity 0.2s; }
.modal-enter-from, .modal-leave-to { opacity: 0; }
</style>
