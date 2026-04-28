<script setup lang="ts">
import { computed } from 'vue';
import type { HealthReport } from '@/lib/api';

const props = defineProps<{
  health?: HealthReport | null;
}>();

const hasIssues = computed(() => {
  if (!props.health) return false;
  return props.health.underrun_count > 0 || props.health.overrun_count > 0 || props.health.stale_drop_count > 0;
});

const statusColor = computed(() => {
  if (!props.health) return 'var(--text-muted)';
  if (props.health.underrun_count > 0) return 'var(--red)';
  if (props.health.overrun_count > 0 || props.health.stale_drop_count > 0) return 'var(--orange)';
  return 'var(--green)';
});
</script>

<template>
  <div v-if="health" class="health-chip" :class="{ 'has-issues': hasIssues }">
    <div class="flex items-center gap-1.5">
      <span class="mdi mdi-pulse health-icon" :style="{ color: statusColor }"></span>
      
      <div class="metrics-grid">
        <div class="metric" :class="{ warning: health.underrun_count > 0 }">
          <span class="mdi mdi-chevron-double-down"></span>
          {{ health.underrun_count }}
        </div>
        <div class="metric" :class="{ warning: health.overrun_count > 0 }">
          <span class="mdi mdi-chevron-double-up"></span>
          {{ health.overrun_count }}
        </div>
        <div class="metric" :class="{ warning: health.stale_drop_count > 0 }">
          <span class="mdi mdi-delete-sweep"></span>
          {{ health.stale_drop_count }}
        </div>
        <div class="metric info">
          <span class="mdi mdi-buffer"></span>
          {{ health.buffer_depth_ms }}ms
        </div>
      </div>
    </div>

    <!-- Tooltip / Detail on hover -->
    <div class="health-tooltip">
      <p class="tooltip-title">Connection Health</p>
      <div class="tooltip-grid">
        <span>Underruns (dropouts)</span> <strong>{{ health.underrun_count }}</strong>
        <span>Overruns (full buffer)</span> <strong>{{ health.overrun_count }}</strong>
        <span>Stale drops (late)</span> <strong>{{ health.stale_drop_count }}</strong>
        <span>Buffer depth</span> <strong>{{ health.buffer_depth_ms }}ms</strong>
        <span>Network jitter</span> <strong>{{ health.jitter_ms }}ms</strong>
        <span>Clock offset</span> <strong>{{ health.latency_ms }}ms</strong>
      </div>
      <p v-if="!hasIssues" class="mt-2 text-xs opacity-60 text-center">Stable stream</p>
      <p v-else class="mt-2 text-xs text-orange-400 text-center">Network instability detected</p>
    </div>
  </div>
</template>

<style scoped>
.health-chip {
  display: inline-block;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 2px 6px;
  position: relative;
  cursor: help;
  transition: all 0.2s;
}
.health-chip:hover {
  background: var(--bg-hover);
  border-color: var(--border-mid);
}
.has-issues {
  border-color: rgba(234, 88, 12, 0.3);
  background: rgba(234, 88, 12, 0.05);
}

.health-icon {
  font-size: 14px;
  filter: drop-shadow(0 0 4px currentColor);
}

.metrics-grid {
  display: flex;
  align-items: center;
  gap: 6px;
  font-family: var(--font-mono);
  font-size: 10px;
}

.metric {
  display: flex;
  align-items: center;
  gap: 2px;
  color: var(--text-muted);
}
.metric.warning {
  color: var(--orange);
  font-weight: bold;
}
.metric.info {
  color: var(--accent);
}

/* Tooltip */
.health-tooltip {
  position: absolute;
  bottom: calc(100% + 8px);
  right: 0;
  width: 200px;
  background: var(--bg-surface);
  border: 1px solid var(--border-mid);
  border-radius: 10px;
  padding: 10px;
  box-shadow: 0 8px 24px rgba(0,0,0,0.5);
  z-index: 100;
  opacity: 0;
  pointer-events: none;
  transform: translateY(4px);
  transition: all 0.15s ease;
}
.health-chip:hover .health-tooltip {
  opacity: 1;
  pointer-events: auto;
  transform: translateY(0);
}

.tooltip-title {
  font-size: 11px;
  font-weight: bold;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--text-muted);
  margin-bottom: 8px;
  border-bottom: 1px solid var(--border);
  padding-bottom: 4px;
}

.tooltip-grid {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 4px 12px;
  font-size: 11px;
}
.tooltip-grid span {
  color: var(--text-secondary);
}
.tooltip-grid strong {
  color: var(--text-primary);
  font-family: var(--font-mono);
}
</style>
