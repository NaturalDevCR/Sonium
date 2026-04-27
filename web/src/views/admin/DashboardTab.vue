<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue';
import { useServerStore } from '@/stores/server';
import StreamBadge from '@/components/StreamBadge.vue';

const store = useServerStore();

onMounted(async () => {
  await store.loadAll();
  store.startLiveUpdates();
});
onUnmounted(() => store.stopLiveUpdates());

const stats = computed(() => [
  {
    label: 'Online',
    value: store.connectedClients.length,
    total: store.clients.length,
    icon:  'mdi-speaker',
    color: '#38bdf8',
    bg:    'rgba(56, 189, 248, 0.07)',
    border:'rgba(56, 189, 248, 0.15)',
  },
  {
    label: 'Groups',
    value: store.groups.length,
    total: null,
    icon:  'mdi-speaker-multiple',
    color: '#a78bfa',
    bg:    'rgba(167, 139, 250, 0.07)',
    border:'rgba(167, 139, 250, 0.15)',
  },
  {
    label: 'Streams',
    value: store.streams.filter(s => s.status === 'playing').length,
    total: store.streams.length,
    icon:  'mdi-play-circle-outline',
    color: '#34d399',
    bg:    'rgba(52, 211, 153, 0.07)',
    border:'rgba(52, 211, 153, 0.15)',
  },
  {
    label: 'Uptime',
    value: fmtUptime(store.uptime),
    total: null,
    icon:  'mdi-clock-outline',
    color: '#fbbf24',
    bg:    'rgba(251, 191, 36, 0.07)',
    border:'rgba(251, 191, 36, 0.15)',
  },
]);

function fmtUptime(s: number) {
  if (!s) return '—';
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  return d > 0 ? `${d}d ${h}h` : h > 0 ? `${h}h ${m}m` : `${m}m`;
}
</script>

<template>
  <div class="space-y-7">

    <!-- Page title -->
    <div>
      <h1 class="page-title">Overview</h1>
      <p class="page-sub">System status at a glance</p>
    </div>

    <!-- Stats grid -->
    <div class="grid grid-cols-2 lg:grid-cols-4 gap-3">
      <div
        v-for="s in stats" :key="s.label"
        class="stat-card"
        :style="{ background: s.bg, border: `1px solid ${s.border}` }"
      >
        <div class="flex items-start justify-between mb-3">
          <span class="mdi text-xl" :class="s.icon" :style="{ color: s.color }"></span>
        </div>
        <p class="stat-value" :style="{ color: s.color }">{{ s.value }}</p>
        <p class="stat-label">
          {{ s.label }}
          <span v-if="s.total !== null" style="opacity: 0.5;"> / {{ s.total }}</span>
        </p>
      </div>
    </div>

    <!-- Two-column grid -->
    <div class="grid lg:grid-cols-2 gap-5">

      <!-- Streams -->
      <section>
        <div class="flex items-center justify-between mb-3">
          <h2 class="section-label">Audio Streams</h2>
          <span class="badge" style="background: var(--accent-dim); color: var(--accent); border: 1px solid var(--accent-border); font-family: var(--font-mono); font-size: 10px;">
            {{ store.streams.length }}
          </span>
        </div>
        <div class="card divide-y" style="border-color: var(--border);">
          <div v-if="store.streams.length === 0" class="px-4 py-5 text-center" style="color: var(--text-muted); font-size: 13px;">
            No streams configured
          </div>
          <div
            v-for="stream in store.streams" :key="stream.id"
            class="flex items-center justify-between px-4 py-3 gap-3"
          >
            <div class="flex items-center gap-3 min-w-0">
              <span
                class="w-7 h-7 rounded-lg flex items-center justify-center shrink-0"
                style="background: rgba(56,189,248,0.08); color: var(--accent);"
              >
                <span class="mdi mdi-music-note text-sm"></span>
              </span>
              <div class="min-w-0">
                <p class="font-medium truncate" style="font-size: 13px; color: var(--text-primary);">
                  {{ stream.display_name || stream.id }}
                </p>
                <p class="truncate" style="font-size: 11px; color: var(--text-muted); font-family: var(--font-mono);">
                  {{ stream.codec.toUpperCase() }} · {{ stream.format }}
                </p>
              </div>
            </div>
            <StreamBadge :status="stream.status" />
          </div>
        </div>
      </section>

      <!-- Clients -->
      <section>
        <div class="flex items-center justify-between mb-3">
          <h2 class="section-label">Connected Clients</h2>
          <span class="badge" style="background: var(--green-dim); color: var(--green); border: 1px solid var(--green-border); font-family: var(--font-mono); font-size: 10px;">
            {{ store.connectedClients.length }} online
          </span>
        </div>
        <div class="card divide-y" style="border-color: var(--border);">
          <div v-if="store.clients.length === 0" class="px-4 py-5 text-center" style="color: var(--text-muted); font-size: 13px;">
            No clients seen yet
          </div>
          <div
            v-for="c in store.clients" :key="c.id"
            class="flex items-center justify-between px-4 py-3 gap-3"
            :style="c.status !== 'connected' ? 'opacity: 0.45;' : ''"
          >
            <div class="flex items-center gap-3 min-w-0">
              <span
                class="w-2 h-2 rounded-full shrink-0"
                :class="c.status === 'connected' ? 'pulse-dot' : ''"
                :style="{ background: c.status === 'connected' ? 'var(--green)' : 'var(--text-muted)' }"
              ></span>
              <div class="min-w-0">
                <p class="font-medium truncate" style="font-size: 13px; color: var(--text-primary);">
                  {{ c.hostname }}
                </p>
                <p class="truncate" style="font-size: 11px; color: var(--text-muted);">
                  {{ c.os }} · {{ c.remote_addr }}
                </p>
              </div>
            </div>
            <span style="font-family: var(--font-mono); font-size: 11px; color: var(--text-muted); flex-shrink: 0;">
              {{ c.muted ? 'muted' : `${c.volume}%` }}
            </span>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.page-title {
  font-family: var(--font-display);
  font-size: 22px;
  font-weight: 700;
  color: var(--text-primary);
  letter-spacing: -0.01em;
  margin-bottom: 2px;
}
.page-sub {
  font-size: 13px;
  color: var(--text-muted);
}
.stat-card {
  border-radius: 12px;
  padding: 16px;
}
.stat-value {
  font-family: var(--font-display);
  font-size: 26px;
  font-weight: 700;
  line-height: 1;
  margin-bottom: 4px;
}
.stat-label {
  font-size: 11px;
  color: var(--text-muted);
  font-weight: 500;
  letter-spacing: 0.02em;
}
.divide-y > * + * { border-top: 1px solid var(--border); }
</style>
