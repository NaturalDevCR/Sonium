<script setup lang="ts">
const props = defineProps<{
  status: 'playing' | 'idle' | 'error';
  codec?: string;
}>();

const config = {
  playing: { bg: 'bg-emerald-500/10', text: 'text-emerald-400', border: 'border-emerald-500/20', label: 'Playing', dot: 'bg-emerald-400' },
  idle:    { bg: 'bg-slate-500/10',   text: 'text-slate-400',   border: 'border-slate-500/20',   label: 'Idle',    dot: 'bg-slate-500' },
  error:   { bg: 'bg-rose-500/10',    text: 'text-rose-400',    border: 'border-rose-500/20',    label: 'Error',   dot: 'bg-rose-400' },
};

const c = config[props.status];
</script>

<template>
  <span
    class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-[10px] font-bold tracking-wider uppercase border"
    :class="[c.bg, c.text, c.border]"
  >
    <span class="w-1.5 h-1.5 rounded-full" :class="c.dot"></span>
    {{ c.label }}
    <span v-if="codec" class="opacity-60">· {{ codec.toUpperCase() }}</span>
  </span>
</template>
