<script setup lang="ts">
import { computed } from 'vue';
import { useServerStore } from '@/stores/server';

const store = useServerStore();

const status = computed(() => {
  if (store.connected) return { label: 'Live', color: '#22c55e', icon: 'mdi-wifi' };
  if (store.connecting) return { label: 'Connecting…', color: '#fbbf24', icon: 'mdi-wifi-sync' };
  if (!store.serverOnline) return { label: 'Server offline', color: '#ef4444', icon: 'mdi-wifi-off' };
  return { label: 'Reconnecting…', color: '#f59e0b', icon: 'mdi-wifi-strength-alert-outline' };
});
</script>

<template>
  <div class="connection-chip" :title="status.label">
    <span
      class="conn-dot"
      :class="{ pulse: store.connecting }"
      :style="{ background: status.color, boxShadow: `0 0 8px ${status.color}40` }"
    ></span>
    <span class="conn-label">{{ status.label }}</span>
  </div>
</template>

<style scoped>
.connection-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  border-radius: 20px;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  transition: all 0.15s;
}
.connection-chip:hover {
  border-color: var(--border-mid);
}
.conn-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
  transition: all 0.3s;
}
.conn-dot.pulse {
  animation: pulse-dot 1.2s ease-in-out infinite;
}
.conn-label {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  white-space: nowrap;
}
</style>
