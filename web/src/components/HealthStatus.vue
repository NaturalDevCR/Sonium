<script setup lang="ts">
import { computed } from 'vue';
import type { HealthReport } from '@/lib/api';
const props = defineProps<{ health?: HealthReport | null }>();

const hasIssues = computed(() => {
  if (!props.health) return false;
  return props.health.underrun_count > 0 || props.health.overrun_count > 0 || props.health.stale_drop_count > 0;
});

const color = computed(() => {
  if (!props.health) return '#475569';
  if (props.health.underrun_count > 0) return '#f43f5e';
  if (props.health.overrun_count > 0 || props.health.stale_drop_count > 0) return '#f59e0b';
  return '#10b981';
});
</script>

<template>
  <div v-if="health" class="relative group inline-block">
    <div
      class="inline-flex items-center gap-1.5 px-2 py-1 rounded-lg text-[10px] font-mono border cursor-help transition-all"
      :class="hasIssues ? 'bg-amber-500/5 border-amber-500/20' : 'bg-white/[0.03] border-white/[0.06] hover:border-white/[0.1]'"
    >
      <span class="mdi mdi-pulse" :style="`color: ${color}; filter: drop-shadow(0 0 4px ${color});`"></span>
      <span class="text-slate-400">{{ health.underrun_count }}</span>
      <span class="text-slate-500">/</span>
      <span class="text-slate-400">{{ health.overrun_count }}</span>
      <span class="text-slate-500">/</span>
      <span class="text-slate-400">{{ health.buffer_depth_ms }}ms</span>
    </div>

    <!-- Tooltip -->
    <div class="absolute bottom-full right-0 mb-2 w-52 glass p-3 rounded-xl opacity-0 group-hover:opacity-100 pointer-events-none group-hover:pointer-events-auto transition-all duration-200 z-50 translate-y-1 group-hover:translate-y-0">
      <div class="text-[10px] font-bold text-slate-500 uppercase tracking-wider mb-2 pb-1 border-b border-white/[0.06]">Connection Health</div>
      <div class="grid grid-cols-[1fr_auto] gap-x-3 gap-y-1 text-[11px]">
        <span class="text-slate-400">Underruns</span> <span class="text-white font-mono">{{ health.underrun_count }}</span>
        <span class="text-slate-400">Overruns</span> <span class="text-white font-mono">{{ health.overrun_count }}</span>
        <span class="text-slate-400">Stale drops</span> <span class="text-white font-mono">{{ health.stale_drop_count }}</span>
        <span class="text-slate-400">Buffer</span> <span class="text-white font-mono">{{ health.buffer_depth_ms }}ms</span>
        <span class="text-slate-400">Jitter</span> <span class="text-white font-mono">{{ health.jitter_ms }}ms</span>
        <span class="text-slate-400">Latency</span> <span class="text-white font-mono">{{ health.latency_ms }}ms</span>
      </div>
      <p v-if="!hasIssues" class="mt-2 text-[10px] text-emerald-400/80 text-center">Stable stream</p>
      <p v-else class="mt-2 text-[10px] text-amber-400/80 text-center">Network instability detected</p>
    </div>
  </div>
</template>
