<script setup lang="ts">
import { ref, watch } from 'vue';
import type { Client, Group } from '@/lib/api';
import { api } from '@/lib/api';

const props = defineProps<{ client: Client; groups: Group[] }>();

const localVolume = ref(props.client.volume);
const localMuted  = ref(props.client.muted);

// Keep local state in sync when server pushes updates
watch(() => props.client.volume, (v) => { localVolume.value = v; });
watch(() => props.client.muted,  (v) => { localMuted.value  = v; });

let volumeTimer: ReturnType<typeof setTimeout> | null = null;

function onVolumeInput() {
  if (volumeTimer) clearTimeout(volumeTimer);
  volumeTimer = setTimeout(() => {
    api.setVolume(props.client.id, localVolume.value, localMuted.value).catch(console.error);
  }, 150);
}

function toggleMute() {
  localMuted.value = !localMuted.value;
  api.setVolume(props.client.id, localVolume.value, localMuted.value).catch(console.error);
}

// ── Drag and Drop ──────────────────────────────────────────────────────
const isDragging = ref(false);

function onDragStart(e: DragEvent) {
  isDragging.value = true;
  e.dataTransfer!.effectAllowed = 'move';
  e.dataTransfer!.setData('application/sonium-client-id', props.client.id);
  e.dataTransfer!.setData('text/plain', props.client.client_name || props.client.hostname);
}

function onDragEnd() {
  isDragging.value = false;
}

// Get the current group name for the pill display
const currentGroupName = ref('');
watch(
  [() => props.client.group_id, () => props.groups],
  () => {
    const g = props.groups.find((g) => g.id === props.client.group_id);
    currentGroupName.value = g?.name ?? '—';
  },
  { immediate: true },
);
</script>

<template>
  <div
    class="card"
    :class="{ disconnected: client.status === 'disconnected', dragging: isDragging }"
    draggable="true"
    @dragstart="onDragStart"
    @dragend="onDragEnd"
  >
    <div class="card-header">
      <div class="info">
        <span class="name">{{ client.client_name || client.hostname }}</span>
        <span class="meta">{{ client.os }} · {{ client.arch }} · {{ client.remote_addr }}</span>
      </div>
      <div class="header-right">
        <span class="group-pill" :title="`Group: ${currentGroupName}`">{{ currentGroupName }}</span>
        <span class="dot" :class="client.status" :title="client.status" />
      </div>
    </div>

    <div class="controls">
      <button class="mute-btn" :class="{ muted: localMuted }" @click="toggleMute" title="Toggle mute">
        {{ localMuted ? '🔇' : '🔊' }}
      </button>
      <input
        type="range" min="0" max="100" step="1"
        v-model.number="localVolume"
        @input="onVolumeInput"
        :disabled="localMuted"
        class="volume-slider"
      />
      <span class="vol-label">{{ localVolume }}%</span>
    </div>

    <div class="footer">
      <span class="drag-hint">⠿ Drag to assign group</span>
      <label class="field latency">
        <span>Latency</span>
        <span class="value">{{ client.latency_ms }} ms</span>
      </label>
    </div>
  </div>
</template>

<style scoped>
.card {
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  transition: opacity 0.2s, transform 0.15s, box-shadow 0.15s;
  cursor: grab;
  user-select: none;
}
.card:hover {
  border-color: var(--accent);
  box-shadow: 0 0 0 1px var(--accent), 0 4px 20px rgba(92, 110, 248, 0.08);
}
.card:active { cursor: grabbing; }
.card.disconnected { opacity: 0.45; cursor: default; }
.card.dragging {
  opacity: 0.5;
  transform: scale(0.97);
  box-shadow: 0 8px 32px rgba(0,0,0,0.3);
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
}
.info { display: flex; flex-direction: column; gap: 3px; }
.name { font-weight: 600; font-size: 0.95rem; }
.meta { font-size: 0.72rem; color: var(--muted); }

.header-right {
  display: flex;
  align-items: center;
  gap: 8px;
}
.group-pill {
  background: var(--chip-bg);
  border-radius: 999px;
  font-size: 0.68rem;
  padding: 2px 10px;
  color: var(--accent);
  font-weight: 600;
  letter-spacing: 0.01em;
}

.dot {
  width: 10px; height: 10px;
  border-radius: 50%;
  flex-shrink: 0;
}
.dot.connected    { background: #4caf7d; box-shadow: 0 0 6px #4caf7d88; }
.dot.disconnected { background: #555; }

.controls {
  display: flex;
  align-items: center;
  gap: 8px;
}
.mute-btn {
  background: none;
  border: 1px solid var(--border);
  border-radius: 6px;
  cursor: pointer;
  font-size: 1rem;
  padding: 2px 6px;
  line-height: 1.5;
  transition: background 0.15s;
}
.mute-btn:hover    { background: var(--hover); }
.mute-btn.muted    { border-color: #ff6b6b44; background: #ff6b6b11; }

.volume-slider {
  flex: 1;
  accent-color: var(--accent);
}
.volume-slider:disabled { opacity: 0.3; }
.vol-label { font-size: 0.75rem; color: var(--muted); width: 36px; text-align: right; }

.footer {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
}
.drag-hint {
  font-size: 0.7rem;
  color: var(--muted);
  opacity: 0.5;
  letter-spacing: 0.02em;
}
.card:hover .drag-hint { opacity: 1; color: var(--accent); }

.field {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.78rem;
  color: var(--muted);
}
.field.latency .value {
  color: var(--text);
  font-variant-numeric: tabular-nums;
}
</style>
