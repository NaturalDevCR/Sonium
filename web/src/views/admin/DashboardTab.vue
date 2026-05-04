<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { useServerStore } from '@/stores/server';
import StreamBadge from '@/components/StreamBadge.vue';

const store = useServerStore();
onMounted(() => store.loadAll());

const stats = computed(() => [
  { label: 'Clients Online', value: store.connectedClients.length, total: store.clients.length, icon: 'mdi-speaker', color: 'from-emerald-500/20 to-emerald-500/5', border: 'border-emerald-500/20', text: 'text-emerald-400' },
  { label: 'Groups', value: store.groups.length, icon: 'mdi-speaker-multiple', color: 'from-violet-500/20 to-violet-500/5', border: 'border-violet-500/20', text: 'text-violet-400' },
  { label: 'Playing', value: store.streams.filter(s => s.status === 'playing').length, total: store.streams.length, icon: 'mdi-play-circle', color: 'from-cyan-500/20 to-cyan-500/5', border: 'border-cyan-500/20', text: 'text-cyan-400' },
  { label: 'Uptime', value: fmtUptime(store.uptime), icon: 'mdi-clock-outline', color: 'from-amber-500/20 to-amber-500/5', border: 'border-amber-500/20', text: 'text-amber-400' },
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
  <div class="space-y-6">
    <!-- Stats -->
    <div class="grid grid-cols-2 lg:grid-cols-4 gap-3">
      <div
        v-for="(s, i) in stats" :key="s.label"
        class="glass glass-hover p-4 animate-fade-up"
        :style="`animation-delay: ${i * 0.05}s; opacity: 0;`"
      >
        <div class="flex items-center justify-between mb-3">
          <div class="w-9 h-9 rounded-xl bg-gradient-to-br flex items-center justify-center border" :class="[s.color, s.border]">
            <span class="mdi text-lg" :class="[s.icon, s.text]"></span>
          </div>
        </div>
        <div class="text-2xl font-display font-bold text-white">{{ s.value }}</div>
        <div class="text-[11px] text-slate-500 mt-0.5">{{ s.label }}</div>
      </div>
    </div>

    <!-- Two columns -->
    <div class="grid lg:grid-cols-2 gap-5">
      <!-- Streams -->
      <div class="glass animate-fade-up delay-200">
        <div class="flex items-center justify-between px-5 py-4 border-b border-white/[0.06]">
          <span class="text-xs font-bold text-slate-500 uppercase tracking-wider">Audio Streams</span>
          <span class="px-2 py-0.5 rounded-md bg-cyan-500/10 text-cyan-400 text-[10px] font-bold font-mono border border-cyan-500/20">{{ store.streams.length }}</span>
        </div>
        <div class="divide-y divide-white/[0.04]">
          <div v-if="store.streams.length === 0" class="px-5 py-6 text-center text-sm text-slate-500">No streams configured</div>
          <div
            v-for="stream in store.streams" :key="stream.id"
            class="flex items-center justify-between px-5 py-3 gap-3 hover:bg-white/[0.02] transition-colors"
          >
            <div class="flex items-center gap-3 min-w-0">
              <div class="w-8 h-8 rounded-lg bg-cyan-500/10 border border-cyan-500/20 flex items-center justify-center shrink-0">
                <span class="mdi mdi-music-note text-cyan-400 text-sm"></span>
              </div>
              <div class="min-w-0">
                <p class="text-sm font-medium text-white truncate">{{ stream.display_name || stream.id }}</p>
                <p class="text-[11px] text-slate-500 font-mono">{{ stream.codec.toUpperCase() }} · {{ stream.format }}</p>
              </div>
            </div>
            <StreamBadge :status="stream.status" />
          </div>
        </div>
      </div>

      <!-- Clients -->
      <div class="glass animate-fade-up delay-300">
        <div class="flex items-center justify-between px-5 py-4 border-b border-white/[0.06]">
          <span class="text-xs font-bold text-slate-500 uppercase tracking-wider">Clients</span>
          <span class="px-2 py-0.5 rounded-md bg-emerald-500/10 text-emerald-400 text-[10px] font-bold font-mono border border-emerald-500/20">{{ store.connectedClients.length }} online</span>
        </div>
        <div class="divide-y divide-white/[0.04]">
          <div v-if="store.clients.length === 0" class="px-5 py-6 text-center text-sm text-slate-500">No clients yet</div>
          <div
            v-for="c in store.clients" :key="c.id"
            class="flex items-center justify-between px-5 py-3 gap-3 hover:bg-white/[0.02] transition-colors"
            :class="c.status !== 'connected' ? 'opacity-40' : ''"
          >
            <div class="flex items-center gap-3 min-w-0">
              <span class="status-dot shrink-0" :class="c.status === 'connected' ? 'online' : 'offline'"></span>
              <div class="min-w-0">
                <p class="text-sm font-medium text-white truncate">{{ c.hostname }}</p>
                <p class="text-[11px] text-slate-500">{{ c.os }} · {{ c.remote_addr }}</p>
              </div>
            </div>
            <span class="text-[11px] text-slate-500 font-mono shrink-0">{{ c.muted ? 'muted' : `${c.volume}%` }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
