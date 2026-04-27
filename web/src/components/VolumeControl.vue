<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(defineProps<{
  volume:   number;
  muted:    boolean;
  name?:    string;
  compact?: boolean;
}>(), { compact: false });

const emit = defineEmits<{
  'update:volume': [v: number];
  'update:muted':  [v: boolean];
}>();

const icon = computed(() => {
  if (props.muted || props.volume === 0) return 'mdi-volume-off';
  if (props.volume < 35)  return 'mdi-volume-low';
  if (props.volume < 70)  return 'mdi-volume-medium';
  return 'mdi-volume-high';
});

const trackStyle = computed(() => ({
  background: props.muted
    ? 'linear-gradient(to right, #1e293b 0%, #1e293b 100%)'
    : `linear-gradient(to right, #38bdf8 0%, #0ea5e9 ${props.volume}%, #1a2840 ${props.volume}%, #1a2840 100%)`,
}));
</script>

<template>
  <!-- Compact inline variant -->
  <div v-if="compact" class="flex items-center gap-2.5 w-full min-w-0">
    <button
      @click="emit('update:muted', !muted)"
      class="shrink-0 w-7 h-7 flex items-center justify-center rounded-lg transition-all"
      :style="muted
        ? 'color: #f87171; background: rgba(248,113,113,0.1);'
        : 'color: var(--text-muted);'"
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
      class="volume-slider flex-1"
    />

    <span
      class="shrink-0 w-8 text-right tabular-nums"
      style="font-family: var(--font-mono); font-size: 11px; color: var(--text-muted);"
    >
      {{ muted ? '—' : volume }}
    </span>
  </div>

  <!-- Full card variant -->
  <div v-else class="space-y-3">
    <div class="flex items-center justify-between gap-2">
      <span v-if="name" class="text-sm font-medium truncate" style="color: var(--text-secondary);">{{ name }}</span>
      <span
        class="ml-auto tabular-nums font-semibold"
        style="font-family: var(--font-mono); font-size: 14px; color: var(--text-primary);"
      >
        {{ muted ? 'MUTED' : `${volume}%` }}
      </span>
    </div>

    <div class="flex items-center gap-3">
      <button
        @click="emit('update:muted', !muted)"
        class="shrink-0 w-8 h-8 flex items-center justify-center rounded-lg transition-all"
        :style="muted
          ? 'color: #f87171; background: rgba(248,113,113,0.1);'
          : 'color: var(--text-muted);'"
      >
        <span class="mdi text-lg" :class="icon"></span>
      </button>

      <input
        type="range" min="0" max="100" step="1"
        :value="volume"
        :disabled="muted"
        :style="trackStyle"
        @input="emit('update:volume', +($event.target as HTMLInputElement).value)"
        class="volume-slider flex-1"
      />
    </div>
  </div>
</template>
