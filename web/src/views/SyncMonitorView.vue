<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue';
import { useServerStore } from '@/stores/server';
import SyncIndicator from '@/components/SyncIndicator.vue';

const store = useServerStore();

onMounted(() => store.init());
onUnmounted(() => store.stopLiveUpdates());

const syncHealth = computed(() => {
  const h: Record<string, { status: 'good' | 'fair' | 'poor' | 'unknown'; drift: number; buffer: number }> = {};
  for (const c of store.connectedClients) {
    const jitter = c.health?.jitter_ms ?? 0;
    const buffer = c.health?.buffer_depth_ms ?? 0;
    if (jitter === 0) h[c.id] = { status: 'unknown', drift: 0, buffer };
    else if (jitter < 10) h[c.id] = { status: 'good', drift: jitter, buffer };
    else if (jitter < 50) h[c.id] = { status: 'fair', drift: jitter, buffer };
    else h[c.id] = { status: 'poor', drift: jitter, buffer };
  }
  return h;
});

const overall = computed(() => {
  const cc = store.connectedClients;
  if (!cc.length) return { status: 'unknown' as const, issues: 0, total: 0 };
  const hs = cc.map(c => syncHealth.value[c.id]?.status ?? 'unknown');
  const poor = hs.filter(h => h === 'poor').length;
  const fair = hs.filter(h => h === 'fair').length;
  if (poor) return { status: 'poor' as const, issues: poor + fair, total: cc.length };
  if (fair) return { status: 'fair' as const, issues: fair, total: cc.length };
  if (hs.every(h => h === 'good')) return { status: 'good' as const, issues: 0, total: cc.length };
  return { status: 'unknown' as const, issues: 0, total: cc.length };
});
</script>

<template>
  <div class="max-w-3xl mx-auto space-y-5">
    <!-- Overall Status Card -->
    <div class="glass p-5 flex items-center justify-between">
      <div class="flex items-center gap-4">
        <div
          class="w-12 h-12 rounded-2xl flex items-center justify-center"
          :class="overall.status === 'good' ? 'bg-emerald-500/10 border border-emerald-500/20' :
                  overall.status === 'fair' ? 'bg-amber-500/10 border border-amber-500/20' :
                  overall.status === 'poor' ? 'bg-rose-500/10 border border-rose-500/20' :
                  'bg-slate-500/10 border border-slate-500/20'"
        >
          <span class="mdi text-xl"
            :class="overall.status === 'good' ? 'mdi-check-circle text-emerald-400' :
                    overall.status === 'fair' ? 'mdi-alert-circle text-amber-400' :
                    overall.status === 'poor' ? 'mdi-close-circle text-rose-400' :
                    'mdi-help-circle text-slate-500'"
          ></span>
        </div>
        <div>
          <div class="text-sm font-semibold text-white">
            {{ overall.status === 'good' ? 'Sync is healthy' :
               overall.status === 'fair' ? 'Sync could be better' :
               overall.status === 'poor' ? 'Sync issues detected' : 'Sync status unknown' }}
          </div>
          <div class="text-xs text-slate-500">
            {{ store.connectedClients.length }} clients · {{ overall.issues }} issues
          </div>
        </div>
      </div>
      <SyncIndicator :status="overall.status" :issue-count="overall.issues" :total-count="overall.total" />
    </div>

    <!-- Client List -->
    <div class="space-y-3">
      <div class="text-[10px] font-bold text-slate-600 uppercase tracking-wider px-1">Client Sync Status</div>

      <div v-if="store.connectedClients.length === 0" class="glass p-10 text-center animate-fade-up">
        <span class="mdi mdi-speaker-off text-3xl text-slate-700 block mb-3"></span>
        <p class="text-sm text-slate-500">No clients connected</p>
      </div>

      <div
        v-for="client in store.connectedClients"
        :key="client.id"
        class="glass p-4 animate-fade-up"
      >
        <div class="flex items-center justify-between gap-3 mb-3">
          <div class="flex items-center gap-2.5">
            <span
              class="w-2 h-2 rounded-full"
              :class="syncHealth[client.id]?.status === 'good' ? 'bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.5)]' :
                      syncHealth[client.id]?.status === 'fair' ? 'bg-amber-400 shadow-[0_0_8px_rgba(251,191,36,0.5)]' :
                      syncHealth[client.id]?.status === 'poor' ? 'bg-rose-400 shadow-[0_0_8px_rgba(244,63,94,0.5)]' :
                      'bg-slate-600'"
            ></span>
            <span class="text-sm font-medium text-white">{{ client.hostname }}</span>
          </div>
          <SyncIndicator :status="syncHealth[client.id]?.status ?? 'unknown'" :issue-count="0" :total-count="1" />
        </div>

        <div class="grid grid-cols-3 gap-3 pt-3 border-t border-white/[0.04]">
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Drift</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ syncHealth[client.id]?.drift.toFixed(1) ?? '—' }} <span class="text-slate-600 text-xs">ms</span></div>
          </div>
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Buffer</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ syncHealth[client.id]?.buffer.toFixed(0) ?? '—' }} <span class="text-slate-600 text-xs">ms</span></div>
          </div>
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Latency</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ client.latency_ms }} <span class="text-slate-600 text-xs">ms</span></div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
