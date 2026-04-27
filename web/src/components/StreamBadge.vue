<script setup lang="ts">
import { computed } from 'vue';

const props = defineProps<{ status: 'playing' | 'idle' | 'error' | string; codec?: string }>();

const cfg = computed(() => {
  const map: Record<string, { dot: string; text: string; bg: string; border: string; pulse: boolean; label: string }> = {
    playing: {
      dot:    '#34d399',
      text:   '#34d399',
      bg:     'rgba(52, 211, 153, 0.07)',
      border: 'rgba(52, 211, 153, 0.2)',
      pulse:  true,
      label:  'Playing',
    },
    idle: {
      dot:    '#4a5a6e',
      text:   '#4a5a6e',
      bg:     'rgba(74, 90, 110, 0.06)',
      border: 'rgba(74, 90, 110, 0.14)',
      pulse:  false,
      label:  'Idle',
    },
    error: {
      dot:    '#f87171',
      text:   '#f87171',
      bg:     'rgba(248, 113, 113, 0.07)',
      border: 'rgba(248, 113, 113, 0.2)',
      pulse:  false,
      label:  'Error',
    },
  };
  return map[props.status] ?? map.idle;
});
</script>

<template>
  <span
    class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold shrink-0"
    :style="{
      background: cfg.bg,
      border: `1px solid ${cfg.border}`,
      color: cfg.text,
      fontFamily: 'var(--font-display)',
      letterSpacing: '0.02em',
    }"
  >
    <span
      class="w-1.5 h-1.5 rounded-full shrink-0"
      :class="cfg.pulse ? 'pulse-dot' : ''"
      :style="{ background: cfg.dot }"
    ></span>
    {{ cfg.label }}
    <span v-if="codec && status === 'playing'" style="opacity: 0.45; font-family: var(--font-mono); font-size: 10px;">
      {{ codec.toUpperCase() }}
    </span>
  </span>
</template>
