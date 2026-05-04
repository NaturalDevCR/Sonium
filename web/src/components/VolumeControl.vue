<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(defineProps<{
  volume: number;
  muted: boolean;
  name?: string;
  compact?: boolean;
}>(), { compact: false });

const emit = defineEmits<{
  'update:volume': [v: number];
  'update:muted': [v: boolean];
}>();

const icon = computed(() => {
  if (props.muted || props.volume === 0) return 'mdi-volume-off';
  if (props.volume < 35) return 'mdi-volume-low';
  if (props.volume < 70) return 'mdi-volume-medium';
  return 'mdi-volume-high';
});

const trackStyle = computed(() => ({
  background: props.muted
    ? 'linear-gradient(to right, rgba(255,255,255,0.04) 0%, rgba(255,255,255,0.04) 100%)'
    : `linear-gradient(to right, #22d3ee 0%, #8b5cf6 ${props.volume}%, rgba(255,255,255,0.05) ${props.volume}%, rgba(255,255,255,0.05) 100%)`,
}));
</script>

<template>
  <div v-if="compact" class="flex items-center gap-3 w-full min-w-0">
    <button
      @click="emit('update:muted', !muted)"
      class="shrink-0 w-8 h-8 flex items-center justify-center rounded-lg transition-all"
      :class="muted ? 'bg-rose-500/10 text-rose-400' : 'text-slate-500 hover:text-slate-300 hover:bg-white/[0.05]'"
      :title="muted ? 'Unmute' : 'Mute'"
    >
      <span class="mdi text-base" :class="icon"></span>
    </button>
    <input
      type="range" min="0" max="100" step="1"
      :value="volume"
      :disabled="muted"
      :style="trackStyle"
      @input="emit('update:volume', +($event.target as HTMLInputElement).value)"
      class="slider-glass flex-1"
    />
    <span class="shrink-0 w-8 text-right text-[11px] font-mono text-slate-500">
      {{ muted ? '—' : volume }}
    </span>
  </div>

  <div v-else class="space-y-3">
    <div class="flex items-center justify-between gap-2">
      <span v-if="name" class="text-sm font-medium text-slate-300">{{ name }}</span>
      <span class="ml-auto text-sm font-mono font-semibold text-white">
        {{ muted ? 'MUTED' : `${volume}%` }}
      </span>
    </div>
    <div class="flex items-center gap-3">
      <button
        @click="emit('update:muted', !muted)"
        class="shrink-0 w-9 h-9 flex items-center justify-center rounded-lg transition-all"
        :class="muted ? 'bg-rose-500/10 text-rose-400' : 'text-slate-500 hover:text-slate-300'"
      >
        <span class="mdi text-lg" :class="icon"></span>
      </button>
      <input
        type="range" min="0" max="100" step="1"
        :value="volume"
        :disabled="muted"
        :style="trackStyle"
        @input="emit('update:volume', +($event.target as HTMLInputElement).value)"
        class="slider-glass flex-1"
      />
    </div>
  </div>
</template>
