<script setup lang="ts">
import { computed } from 'vue';

interface Props {
  /** Overall sync status */
  status: 'good' | 'fair' | 'poor' | 'unknown';
  /** Number of clients with issues */
  issueCount?: number;
  /** Total client count */
  totalCount?: number;
}

const props = withDefaults(defineProps<Props>(), {
  issueCount: 0,
  totalCount: 0,
});

const statusConfig = computed(() => {
  switch (props.status) {
    case 'good':
      return {
        color: '#22c55e',
        glow: 'rgba(34, 197, 94, 0.4)',
        label: 'Sync OK',
        icon: 'mdi-check-circle',
      };
    case 'fair':
      return {
        color: '#f59e0b',
        glow: 'rgba(245, 158, 11, 0.4)',
        label: `Sync Fair (${props.issueCount}/${props.totalCount})`,
        icon: 'mdi-alert-circle',
      };
    case 'poor':
      return {
        color: '#ef4444',
        glow: 'rgba(239, 68, 68, 0.4)',
        label: `Sync Poor (${props.issueCount}/${props.totalCount})`,
        icon: 'mdi-close-circle',
      };
    default:
      return {
        color: 'var(--text-muted)',
        glow: 'transparent',
        label: 'Sync Unknown',
        icon: 'mdi-help-circle',
      };
  }
});
</script>

<template>
  <div class="sync-indicator" :title="statusConfig.label">
    <span
      class="sync-dot"
      :style="{
        background: statusConfig.color,
        boxShadow: `0 0 8px ${statusConfig.glow}`,
      }"
    ></span>
    <span class="sync-label">{{ statusConfig.label }}</span>
  </div>
</template>

<style scoped>
.sync-indicator {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  border-radius: 20px;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  cursor: pointer;
  transition: all 0.15s;
}
.sync-indicator:hover {
  border-color: var(--border-mid);
}
.sync-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
  transition: all 0.3s;
}
.sync-label {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  white-space: nowrap;
}
</style>
