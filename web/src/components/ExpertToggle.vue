<script setup lang="ts">
import { ref, computed } from 'vue';

const EXPERT_KEY = 'sonium-expert-mode';

const enabled = ref(localStorage.getItem(EXPERT_KEY) === 'true');

function toggle() {
  enabled.value = !enabled.value;
  localStorage.setItem(EXPERT_KEY, String(enabled.value));
}

const label = computed(() => (enabled.value ? 'Expert' : 'Simple'));
</script>

<template>
  <button
    @click="toggle"
    class="expert-toggle"
    :class="{ active: enabled }"
    :title="enabled ? 'Switch to simple mode' : 'Switch to expert mode'"
  >
    <span class="w-4 h-4 flex items-center justify-center">
      <span class="mdi text-sm" :class="enabled ? 'mdi-flask' : 'mdi-flask-outline'"></span>
    </span>
    <span class="text-xs font-medium">{{ label }}</span>
  </button>
</template>

<style scoped>
.expert-toggle {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  border-radius: 6px;
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  color: var(--text-muted);
  cursor: pointer;
  transition: all 0.15s;
}
.expert-toggle:hover {
  border-color: var(--border-mid);
  color: var(--text-secondary);
}
.expert-toggle.active {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-dim);
}
</style>
