<script setup lang="ts">
import { ref, watch, computed } from 'vue';
import type { Group, Stream } from '@/lib/api';
import { api } from '@/lib/api';
import StreamBadge from './StreamBadge.vue';

const props = defineProps<{
  group:   Group;
  streams: Stream[];
  clientNames: Record<string, string>;
}>();

const emit = defineEmits<{ deleted: [id: string] }>();

const localStream = ref(props.group.stream_id);
watch(() => props.group.stream_id, (v) => { localStream.value = v; });

const currentStream = computed(() =>
  props.streams.find((s) => s.id === localStream.value),
);

function onStreamChange() {
  api.setGroupStream(props.group.id, localStream.value).catch(console.error);
}

async function deleteGroup() {
  if (!confirm(`Delete group "${props.group.name}"?`)) return;
  await api.deleteGroup(props.group.id).catch(console.error);
  emit('deleted', props.group.id);
}

// ── Drag and Drop (as drop target) ───────────────────────────────────
const isOver = ref(false);
let dragCounter = 0;  // track nested enter/leave for child elements

function onDragEnter(e: DragEvent) {
  if (!e.dataTransfer?.types.includes('application/sonium-client-id')) return;
  e.preventDefault();
  dragCounter++;
  isOver.value = true;
}

function onDragOver(e: DragEvent) {
  if (!e.dataTransfer?.types.includes('application/sonium-client-id')) return;
  e.preventDefault();
  e.dataTransfer!.dropEffect = 'move';
}

function onDragLeave() {
  dragCounter--;
  if (dragCounter <= 0) {
    dragCounter = 0;
    isOver.value = false;
  }
}

async function onDrop(e: DragEvent) {
  e.preventDefault();
  dragCounter = 0;
  isOver.value = false;

  const clientId = e.dataTransfer?.getData('application/sonium-client-id');
  if (!clientId) return;

  // Skip if client already in this group
  if (props.group.client_ids.includes(clientId)) return;

  try {
    await api.setGroup(clientId, props.group.id);
  } catch (err) {
    console.error('Failed to move client to group:', err);
  }
}

// ── Client chip drag (re-order / move out) ───────────────────────────
function onChipDragStart(e: DragEvent, clientId: string) {
  e.dataTransfer!.effectAllowed = 'move';
  e.dataTransfer!.setData('application/sonium-client-id', clientId);
  e.dataTransfer!.setData('text/plain', props.clientNames[clientId] ?? clientId);
}
</script>

<template>
  <div
    class="group-card"
    :class="{ 'drop-over': isOver }"
    @dragenter="onDragEnter"
    @dragover="onDragOver"
    @dragleave="onDragLeave"
    @drop="onDrop"
  >
    <div class="group-header">
      <span class="group-name">{{ group.name }}</span>
      <StreamBadge :stream="currentStream" />
      <button
        v-if="group.name !== 'default'"
        class="del-btn"
        title="Delete group"
        @click="deleteGroup"
      >✕</button>
    </div>

    <div class="stream-row">
      <span class="label">Stream</span>
      <select v-model="localStream" @change="onStreamChange">
        <option value="">— none —</option>
        <option v-for="s in streams" :key="s.id" :value="s.id">
          {{ s.id }} ({{ s.codec }})
        </option>
      </select>
    </div>

    <div class="clients-list" :class="{ 'drop-zone-active': isOver }">
      <span v-if="group.client_ids.length === 0 && !isOver" class="empty">No clients — drag one here</span>
      <span v-if="isOver && group.client_ids.length === 0" class="drop-prompt">Drop client here</span>
      <span
        v-for="id in group.client_ids"
        :key="id"
        class="client-chip"
        draggable="true"
        @dragstart="(e) => onChipDragStart(e, id)"
      >
        <span class="chip-grip">⠿</span>
        {{ clientNames[id] ?? id }}
      </span>
    </div>

    <!-- Drop indicator bar -->
    <div v-if="isOver" class="drop-indicator">
      <span class="drop-icon">↓</span>
      Drop to assign
    </div>
  </div>
</template>

<style scoped>
.group-card {
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  transition: border-color 0.2s, box-shadow 0.2s, background 0.2s;
}
.group-card.drop-over {
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--accent), 0 8px 32px rgba(92, 110, 248, 0.15);
  background: rgba(92, 110, 248, 0.04);
}

.group-header {
  display: flex;
  align-items: center;
  gap: 8px;
}
.group-name { font-weight: 600; font-size: 0.95rem; flex: 1; }
.del-btn {
  background: none;
  border: 1px solid var(--border);
  border-radius: 5px;
  color: var(--muted);
  cursor: pointer;
  font-size: 0.7rem;
  padding: 1px 5px;
  transition: color 0.15s, border-color 0.15s;
}
.del-btn:hover { color: #ff6b6b; border-color: #ff6b6b55; }

.stream-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 0.8rem;
}
.label { color: var(--muted); }
.stream-row select {
  background: var(--input-bg);
  border: 1px solid var(--border);
  border-radius: 5px;
  color: var(--text);
  font-size: 0.78rem;
  padding: 2px 6px;
  cursor: pointer;
}

.clients-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  min-height: 30px;
  border-radius: 8px;
  padding: 6px;
  transition: background 0.2s, border-color 0.2s;
  border: 2px dashed transparent;
}
.clients-list.drop-zone-active {
  border-color: var(--accent);
  background: rgba(92, 110, 248, 0.06);
}

.empty { font-size: 0.75rem; color: var(--muted); font-style: italic; }

.drop-prompt {
  font-size: 0.78rem;
  color: var(--accent);
  font-weight: 600;
  animation: pulse 1s ease-in-out infinite;
}
@keyframes pulse {
  0%, 100% { opacity: 0.6; }
  50%      { opacity: 1; }
}

.client-chip {
  background: var(--chip-bg);
  border-radius: 999px;
  font-size: 0.72rem;
  padding: 3px 10px;
  color: var(--muted);
  cursor: grab;
  transition: background 0.15s, color 0.15s, transform 0.15s;
  display: flex;
  align-items: center;
  gap: 4px;
  user-select: none;
}
.client-chip:hover {
  background: var(--accent);
  color: #fff;
  transform: translateY(-1px);
}
.client-chip:active { cursor: grabbing; }

.chip-grip {
  font-size: 0.6rem;
  opacity: 0.4;
  line-height: 1;
}
.client-chip:hover .chip-grip { opacity: 0.8; }

.drop-indicator {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  font-size: 0.72rem;
  font-weight: 600;
  color: var(--accent);
  padding: 4px 0;
  animation: slideIn 0.15s ease-out;
}
@keyframes slideIn {
  from { opacity: 0; transform: translateY(-4px); }
  to   { opacity: 1; transform: translateY(0); }
}
.drop-icon {
  font-size: 0.85rem;
  animation: bounce 0.6s ease-in-out infinite;
}
@keyframes bounce {
  0%, 100% { transform: translateY(0); }
  50%      { transform: translateY(2px); }
}
</style>
