<template>
  <div class="eq-control">
    <div class="eq-header">
      <span class="eq-title">EQ</span>
      <button class="eq-reset" @click="reset" title="Reset EQ to flat">Reset</button>
    </div>
    <div class="eq-bands">
      <div v-for="band in bands" :key="band.freq_hz" class="eq-band">
        <span class="eq-band-label">{{ formatFreq(band.freq_hz) }}</span>
        <input
          type="range"
          class="eq-slider"
          :min="-12"
          :max="12"
          :step="0.5"
          :value="band.gain_db"
          orient="vertical"
          @input="onSlider(band.freq_hz, Number(($event.target as HTMLInputElement).value))"
        />
        <span class="eq-band-db">{{ band.gain_db > 0 ? '+' : '' }}{{ band.gain_db.toFixed(1) }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';
import type { EqBand } from '@/lib/api';

const props = defineProps<{
  clientId:  string;
  modelValue: EqBand[] | undefined;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', bands: EqBand[]): void;
}>();

const DEFAULT_BANDS: EqBand[] = [
  { freq_hz: 100,   gain_db: 0, q: 0.9 },
  { freq_hz: 1000,  gain_db: 0, q: 0.9 },
  { freq_hz: 10000, gain_db: 0, q: 0.9 },
];

const bands = ref<EqBand[]>(
  props.modelValue && props.modelValue.length > 0
    ? [...props.modelValue]
    : DEFAULT_BANDS.map(b => ({ ...b })),
);

// Keep in sync when parent updates (e.g. from WS event)
watch(() => props.modelValue, (v) => {
  if (v && v.length > 0) {
    bands.value = [...v];
  }
}, { deep: true });

function formatFreq(hz: number) {
  return hz >= 1000 ? `${hz / 1000}k` : `${hz}`;
}

function onSlider(freq_hz: number, gain_db: number) {
  bands.value = bands.value.map(b =>
    b.freq_hz === freq_hz ? { ...b, gain_db } : b,
  );
  emit('update:modelValue', bands.value.map(b => ({ ...b })));
}

function reset() {
  bands.value = DEFAULT_BANDS.map(b => ({ ...b }));
  emit('update:modelValue', bands.value.map(b => ({ ...b })));
}
</script>

<style scoped>
.eq-control {
  padding: 8px 0 4px;
}

.eq-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}

.eq-title {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.08em;
  color: var(--text-muted);
  text-transform: uppercase;
}

.eq-reset {
  font-size: 10px;
  color: var(--text-muted);
  background: none;
  border: 1px solid var(--border-mid);
  border-radius: 4px;
  padding: 1px 7px;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}
.eq-reset:hover { color: var(--accent); border-color: var(--accent); }

.eq-bands {
  display: flex;
  gap: 12px;
  justify-content: flex-start;
}

.eq-band {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
}

.eq-band-label {
  font-size: 10px;
  color: var(--text-muted);
  font-family: var(--font-mono);
}

/* Vertical range input */
.eq-slider {
  writing-mode: vertical-lr;
  direction: rtl;
  -webkit-appearance: slider-vertical;
  appearance: auto;
  width: 20px;
  height: 60px;
  cursor: pointer;
  accent-color: var(--accent);
}

.eq-band-db {
  font-size: 9.5px;
  color: var(--text-muted);
  font-family: var(--font-mono);
  min-width: 28px;
  text-align: center;
}
</style>
