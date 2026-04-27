<template>
  <!-- VU meter bar — vertical, 3-zone colour (green / yellow / red) -->
  <div class="level-meter" :title="`${displayDb} dBFS`">
    <div class="level-meter__track">
      <div class="level-meter__fill" :style="fillStyle" />
    </div>
    <span class="level-meter__label">{{ displayDb }}</span>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

const props = defineProps<{
  /** RMS level in dBFS (negative). Clipped to [-60, 0]. */
  rmsDb: number;
}>();

/** Map [-60, 0] dBFS → [0, 100] % fill, bottom-up. */
const pct = computed(() => {
  const clamped = Math.max(-60, Math.min(0, props.rmsDb));
  return ((clamped + 60) / 60) * 100;
});

/** 3-zone colour: green < -18 dBFS, yellow -18…-6, red > -6. */
const fillColor = computed(() => {
  if (props.rmsDb > -6)  return '#ef4444'; // red
  if (props.rmsDb > -18) return '#facc15'; // yellow
  return '#22c55e';                          // green
});

const fillStyle = computed(() => ({
  height:     `${pct.value}%`,
  background: fillColor.value,
  transition: 'height 80ms linear, background 200ms ease',
}));

const displayDb = computed(() => {
  const clamped = Math.max(-60, Math.min(0, props.rmsDb));
  return `${clamped.toFixed(0)} dB`;
});
</script>

<style scoped>
.level-meter {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  width: 20px;
  user-select: none;
}

.level-meter__track {
  width: 8px;
  height: 60px;
  background: #374151;   /* dark grey track */
  border-radius: 4px;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  justify-content: flex-end; /* fill grows from bottom */
}

.level-meter__fill {
  width: 100%;
  border-radius: 4px;
}

.level-meter__label {
  font-size: 9px;
  color: #6b7280;
  white-space: nowrap;
}
</style>
