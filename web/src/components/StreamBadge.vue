<script setup lang="ts">
import { computed } from 'vue';
import type { StreamRecovery, StreamStatus } from '@/lib/api';

const props = defineProps<{
  status: StreamStatus;
  codec?: string;
  recovery?: StreamRecovery | null;
}>();

const config: Record<StreamStatus, { bg: string; text: string; border: string; label: string; dot: string }> = {
  playing: { bg: 'bg-emerald-500/10', text: 'text-emerald-400', border: 'border-emerald-500/20', label: 'Playing', dot: 'bg-emerald-400' },
  idle:    { bg: 'bg-slate-500/10',   text: 'text-slate-400',   border: 'border-slate-500/20',   label: 'Idle',    dot: 'bg-slate-500' },
  recovering: { bg: 'bg-amber-500/10', text: 'text-amber-400', border: 'border-amber-500/20', label: 'Recovering', dot: 'bg-amber-400' },
  error:   { bg: 'bg-rose-500/10',    text: 'text-rose-400',    border: 'border-rose-500/20',    label: 'Error',   dot: 'bg-rose-400' },
};

const c = computed(() => config[props.status] ?? config.error);
const recovery = computed(() => props.status === 'recovering' ? props.recovery : null);
</script>

<template>
  <span
    class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-[10px] font-bold tracking-wider uppercase border"
    :class="[c.bg, c.text, c.border]"
  >
    <span class="w-1.5 h-1.5 rounded-full" :class="c.dot"></span>
    {{ c.label }}
    <span v-if="recovery" class="opacity-60">· retry {{ recovery.attempt }} in {{ recovery.retry_in_ms }}ms</span>
    <span v-if="codec" class="opacity-60">· {{ codec.toUpperCase() }}</span>
  </span>
</template>
