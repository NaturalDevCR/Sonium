<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue';
import { useServerStore } from '@/stores/server';
import { syncStateFromHealth, type SyncState } from '@/lib/api';
import SyncIndicator from '@/components/SyncIndicator.vue';

const store = useServerStore();

onMounted(() => store.init());
onUnmounted(() => store.stopLiveUpdates());

type Status = 'good' | 'fair' | 'poor' | 'unknown';

function syncStateToStatus(s: SyncState): Status {
  if (s === 'sync_ok') return 'good';
  if (s === 'sync_degraded') return 'fair';
  return 'poor';
}

interface PerClient {
  status: Status;
  state: SyncState;
  jitter_ms: number;
  buffer_ms: number;
  clock_offset_us: number;
  group_offset_us: number;
  total_offset_us: number;
  sync_error_us: number;
  playout_p50_us: number;
  playout_p95_us: number;
  playout_p99_us: number;
  callback_p99_us: number;
  output_latency_us: number;
  has_data: boolean;
}

const syncHealth = computed(() => {
  const h: Record<string, PerClient> = {};
  for (const c of store.connectedClients) {
    const r = c.health ?? null;
    const state = syncStateFromHealth(r);
    h[c.id] = {
      status: r ? syncStateToStatus(state) : 'unknown',
      state,
      jitter_ms: r?.jitter_ms ?? 0,
      buffer_ms: r?.buffer_depth_ms ?? 0,
      clock_offset_us: r?.clock_offset_us ?? 0,
      group_offset_us: r?.group_offset_us ?? 0,
      total_offset_us: r?.total_offset_us ?? 0,
      sync_error_us: r?.sync_error_to_group_us ?? 0,
      playout_p50_us: r?.playout_error_us_p50 ?? 0,
      playout_p95_us: r?.playout_error_us_p95 ?? 0,
      playout_p99_us: r?.playout_error_us_p99 ?? 0,
      callback_p99_us: r?.callback_xrun_us_p99 ?? 0,
      output_latency_us: r?.output_latency_us ?? 0,
      has_data: !!r,
    };
  }
  return h;
});

const groupSkewMaxUs = computed(() => {
  let max = 0;
  for (const c of store.connectedClients) {
    const e = Math.abs(syncHealth.value[c.id]?.sync_error_us ?? 0);
    if (e > max) max = e;
  }
  return max;
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

function fmtUs(us: number): string {
  if (!Number.isFinite(us)) return '—';
  const abs = Math.abs(us);
  if (abs >= 1000) return (us / 1000).toFixed(2) + ' ms';
  return us.toFixed(0) + ' µs';
}
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
            {{ store.connectedClients.length }} clients · {{ overall.issues }} issues · max group skew {{ fmtUs(groupSkewMaxUs) }}
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
            <span class="text-[10px] text-slate-600 uppercase tracking-wider">{{ syncHealth[client.id]?.state ?? 'unknown' }}</span>
          </div>
          <SyncIndicator :status="syncHealth[client.id]?.status ?? 'unknown'" :issue-count="0" :total-count="1" />
        </div>

        <div class="grid grid-cols-3 gap-3 pt-3 border-t border-white/[0.04]">
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Jitter</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ syncHealth[client.id]?.jitter_ms.toFixed(1) ?? '—' }} <span class="text-slate-600 text-xs">ms</span></div>
          </div>
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Buffer</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ syncHealth[client.id]?.buffer_ms.toFixed(0) ?? '—' }} <span class="text-slate-600 text-xs">ms</span></div>
          </div>
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Sync error</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ fmtUs(syncHealth[client.id]?.sync_error_us ?? 0) }}</div>
          </div>
        </div>

        <div class="grid grid-cols-3 gap-3 pt-3 mt-3 border-t border-white/[0.04]">
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Playout p50</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ fmtUs(syncHealth[client.id]?.playout_p50_us ?? 0) }}</div>
          </div>
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Playout p95</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ fmtUs(syncHealth[client.id]?.playout_p95_us ?? 0) }}</div>
          </div>
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Playout p99</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ fmtUs(syncHealth[client.id]?.playout_p99_us ?? 0) }}</div>
          </div>
        </div>

        <div class="grid grid-cols-3 gap-3 pt-3 mt-3 border-t border-white/[0.04]">
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Clock offset</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ fmtUs(syncHealth[client.id]?.clock_offset_us ?? 0) }}</div>
          </div>
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Callback p99</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ fmtUs(syncHealth[client.id]?.callback_p99_us ?? 0) }}</div>
          </div>
          <div>
            <div class="text-[10px] text-slate-600 uppercase tracking-wider mb-1">Output lat</div>
            <div class="text-sm font-mono font-medium text-slate-200">{{ fmtUs(syncHealth[client.id]?.output_latency_us ?? 0) }}</div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
