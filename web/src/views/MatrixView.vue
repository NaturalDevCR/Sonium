<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useServerStore } from '@/stores/server';
import { useAuthStore } from '@/stores/auth';
import { api } from '@/lib/api';
import VolumeControl from '@/components/VolumeControl.vue';

const store = useServerStore();
const auth = useAuthStore();

// Container refs for coordinate calculation
const containerRef = ref<HTMLElement | null>(null);
const sourceRefs = ref<Record<string, HTMLElement>>({});
const zoneRefs = ref<Record<string, HTMLElement>>({});
const zoneCardRefs = ref<Record<string, HTMLElement>>({});

// Dragging state
const isDragging = ref(false);
const draggedStreamId = ref<string | null>(null);
const mousePos = ref({ x: 0, y: 0 });
const hoverZoneId = ref<string | null>(null);
const expandedGroups = ref<Set<string>>(new Set());

// Force reactive updates for connections on resize/scroll
const connectionsVersion = ref(0);
const triggerRedraw = () => { connectionsVersion.value++; };

const toggleGroup = (groupId: string) => {
  if (expandedGroups.value.has(groupId)) {
    expandedGroups.value.delete(groupId);
  } else {
    expandedGroups.value.add(groupId);
  }
  expandedGroups.value = new Set(expandedGroups.value);

  // Redraw connections because height changed
  setTimeout(triggerRedraw, 50);
  setTimeout(triggerRedraw, 150);
  setTimeout(triggerRedraw, 300);
};

// Colors for streams
const streamColors = [
  '#38bdf8', // sky-400 (Sonium accent)
  '#a855f7', // purple
  '#34d399', // emerald (Sonium green)
  '#f472b6', // pink
  '#fbbf24', // amber
  '#22d3ee', // cyan
  '#d946ef', // fuchsia
  '#eab308', // yellow
];

const getStreamColor = (streamId: string) => {
  const index = store.streams.findIndex(s => s.id === streamId);
  return streamColors[Math.max(0, index) % streamColors.length];
};

const getStreamLabel = (stream: any) => {
  return stream.display_name || stream.id;
};

onMounted(() => {
  store.init();
  window.addEventListener('resize', triggerRedraw);
  document.addEventListener('mousemove', handleMouseMove);
  document.addEventListener('mouseup', handleMouseUp);

  // Initial draw delay to ensure DOM is ready
  setTimeout(triggerRedraw, 200);
});

onUnmounted(() => {
  store.stopLiveUpdates();
  window.removeEventListener('resize', triggerRedraw);
  document.removeEventListener('mousemove', handleMouseMove);
  document.removeEventListener('mouseup', handleMouseUp);
});

const setSourceRef = (id: string) => (el: any) => {
  if (el) sourceRefs.value[id] = el as HTMLElement;
};

const setZoneRef = (id: string) => (el: any) => {
  if (el) zoneRefs.value[id] = el as HTMLElement;
};

const setZoneCardRef = (id: string) => (el: any) => {
  if (el) zoneCardRefs.value[id] = el as HTMLElement;
};

const getConnectorCoordinates = (el: HTMLElement) => {
  const rect = el.getBoundingClientRect();
  const containerRect = containerRef.value?.getBoundingClientRect() || { left: 0, top: 0 };
  return {
    x: rect.left - containerRect.left + rect.width / 2,
    y: rect.top - containerRect.top + rect.height / 2
  };
};

const generateBezierPath = (x1: number, y1: number, x2: number, y2: number) => {
  const dx = Math.abs(x2 - x1);
  const cx1 = x1 + dx * 0.4;
  const cx2 = x2 - dx * 0.4;
  return `M ${x1} ${y1} C ${cx1} ${y1}, ${cx2} ${y2}, ${x2} ${y2}`;
};

const activeConnections = computed(() => {
  // Dependency on connectionsVersion to force redraw
  connectionsVersion.value;

  if (!store.groups.length || !containerRef.value) return [];

  const connections: any[] = [];
  store.groups.forEach(group => {
    const streamId = group.stream_id;
    const sourceEl = sourceRefs.value[streamId];
    const zoneEl = zoneRefs.value[group.id];

    if (sourceEl && zoneEl) {
      const start = getConnectorCoordinates(sourceEl);
      const end = getConnectorCoordinates(zoneEl);
      connections.push({
        groupId: group.id,
        streamId,
        path: generateBezierPath(start.x, start.y, end.x, end.y),
        color: getStreamColor(streamId),
        isPlaying: store.streamsById[streamId]?.status === 'playing'
      });
    }
  });
  return connections;
});

const draggingPath = computed(() => {
  if (!isDragging.value || !draggedStreamId.value || !containerRef.value) return null;
  const sourceEl = sourceRefs.value[draggedStreamId.value];
  if (!sourceEl) return null;

  const start = getConnectorCoordinates(sourceEl);
  let endX = mousePos.value.x;
  let endY = mousePos.value.y;

  if (hoverZoneId.value && zoneRefs.value[hoverZoneId.value]) {
    const zoneEl = zoneRefs.value[hoverZoneId.value];
    const snap = getConnectorCoordinates(zoneEl);
    endX = snap.x;
    endY = snap.y;
  }

  return {
    path: generateBezierPath(start.x, start.y, endX, endY),
    color: getStreamColor(draggedStreamId.value)
  };
});

const startDrag = (streamId: string, event: MouseEvent) => {
  isDragging.value = true;
  draggedStreamId.value = streamId;
  updateMousePos(event.clientX, event.clientY);
};

const updateMousePos = (clientX: number, clientY: number) => {
  const container = containerRef.value;
  if (!container) return;
  const rect = container.getBoundingClientRect();

  mousePos.value = {
    x: clientX - rect.left,
    y: clientY - rect.top
  };

  let foundHover = false;
  for (const [zoneId, el] of Object.entries(zoneCardRefs.value)) {
    if (!el) continue;
    const zoneRect = el.getBoundingClientRect();
    if (clientX >= zoneRect.left - 40 && clientX <= zoneRect.right + 20 &&
        clientY >= zoneRect.top - 20 && clientY <= zoneRect.bottom + 20) {
      hoverZoneId.value = zoneId;
      foundHover = true;
      break;
    }
  }
  if (!foundHover) hoverZoneId.value = null;
};

const handleMouseMove = (event: MouseEvent) => {
  if (!isDragging.value) return;
  updateMousePos(event.clientX, event.clientY);
};

const handleMouseUp = async () => {
  if (!isDragging.value) return;

  if (draggedStreamId.value && hoverZoneId.value) {
    const group = store.groups.find(g => g.id === hoverZoneId.value);
    if (group && group.stream_id !== draggedStreamId.value) {
      await api.setGroupStream(group.id, draggedStreamId.value);
    }
  }

  isDragging.value = false;
  draggedStreamId.value = null;
  hoverZoneId.value = null;
};

// Client volume logic (debounced)
const pendingVolume = ref<Record<string, { volume: number; muted: boolean }>>({});
const debounceTimers: Record<string, any> = {};

function setVolume(clientId: string, volume: number, muted: boolean) {
  pendingVolume.value[clientId] = { volume, muted };
  clearTimeout(debounceTimers[clientId]);
  debounceTimers[clientId] = setTimeout(async () => {
    const v = pendingVolume.value[clientId];
    if (!v) return;
    delete pendingVolume.value[clientId];
    await api.setVolume(clientId, v.volume, v.muted);
  }, 120);
}

function clientVolume(clientId: string) {
  return pendingVolume.value[clientId] ?? {
    volume: store.clientsById[clientId]?.volume ?? 100,
    muted: store.clientsById[clientId]?.muted ?? false,
  };
}

const getGroupClients = (clientIds: string[]) => {
  return clientIds.map(id => store.clientsById[id]).filter(Boolean);
};

// Renaming (Simplified for Sonium)
const renamingGroupId = ref<string | null>(null);
const newGroupName = ref('');

const startGroupRename = (id: string, name: string) => {
  if (!auth.isAdmin) return;
  renamingGroupId.value = id;
  newGroupName.value = name;
};

const submitGroupRename = async (id: string) => {
  if (!newGroupName.value || newGroupName.value === store.groups.find(g => g.id === id)?.name) {
    renamingGroupId.value = null;
    return;
  }
  await api.renameGroup(id, newGroupName.value);
  renamingGroupId.value = null;
};

const vFocus = {
  mounted: (el: HTMLInputElement) => el.focus()
};
</script>

<template>
  <div class="matrix-root" ref="containerRef">
    <div class="matrix-toolbar">
      <button @click="store.loadAll()" :disabled="store.loading" class="ctrl-icon-btn" title="Re-sync">
        <span class="mdi mdi-refresh text-lg" :class="{ 'spin': store.loading }"></span>
      </button>
    </div>

    <div class="relative z-20 grid grid-cols-1 lg:grid-cols-2 gap-12 lg:gap-24 mt-4">

      <!-- SOURCES COLUMN -->
      <div class="flex flex-col gap-4">
        <h2 class="section-label px-2 mb-2 flex items-center gap-2">
          <span class="material-symbols-outlined text-sm">input</span> Virtual Sources
        </h2>

        <div v-for="stream in store.streams" :key="stream.id"
             class="source-card"
             :class="[stream.status === 'playing' ? 'playing' : 'idle']"
             :style="`--stream-color: ${getStreamColor(stream.id)}`">

          <div class="flex items-center gap-4 overflow-hidden">
            <div class="source-icon">
              <span class="mdi mdi-cast text-lg" :style="{ color: getStreamColor(stream.id) }"></span>
            </div>
            <div class="truncate pr-4 flex flex-col justify-center">
              <div class="font-bold text-text-primary text-[15px] truncate items-center flex gap-2">
                {{ getStreamLabel(stream) }}
                <div class="w-1.5 h-1.5 rounded-full shrink-0 transition-all duration-500"
                     :class="stream.status === 'playing' ? 'pulse-dot' : 'opacity-40'"
                     :style="{ backgroundColor: getStreamColor(stream.id), boxShadow: stream.status === 'playing' ? `0 0 8px ${getStreamColor(stream.id)}` : 'none' }"></div>
              </div>
              <div class="text-[9px] font-bold text-text-muted mt-0.5 truncate uppercase tracking-widest">{{ stream.status }}</div>
            </div>
          </div>

          <!-- Connector Node -->
          <div :ref="setSourceRef(stream.id)"
               @mousedown.prevent="startDrag(stream.id, $event)"
               class="connector source-connector"
               :style="{ borderColor: getStreamColor(stream.id) }">
            <div class="connector-dot" :class="{'ping': isDragging && draggedStreamId === stream.id}" :style="{ backgroundColor: getStreamColor(stream.id) }"></div>
          </div>
        </div>
      </div>

      <!-- ZONES COLUMN -->
      <div class="flex flex-col gap-4">
        <h2 class="section-label px-2 mb-2 flex items-center gap-2">
          <span class="material-symbols-outlined text-sm">speaker_group</span> Output Zones
        </h2>

        <div v-for="group in store.groups" :key="group.id"
             :ref="setZoneCardRef(group.id)"
             class="zone-card"
             :class="[
                hoverZoneId === group.id ? 'hovered' : '',
                !group.stream_id ? 'empty' : ''
             ]">

          <!-- Connector Node -->
          <div :ref="setZoneRef(group.id)"
               class="connector zone-connector"
               :style="{ borderColor: group.stream_id ? getStreamColor(group.stream_id) : 'var(--text-muted)' }">
            <div class="connector-dot" :class="{'ping': hoverZoneId === group.id}" :style="{ backgroundColor: group.stream_id ? getStreamColor(group.stream_id) : 'var(--text-muted)' }"></div>
          </div>

          <div class="zone-content" @click="toggleGroup(group.id)">
            <div class="flex items-center justify-between gap-4 p-4 min-h-[72px]">
              <div class="flex items-center gap-4 ml-6 overflow-hidden">
                <div class="truncate">
                  <h3 class="text-base font-bold text-text-primary tracking-tight flex items-center gap-3 truncate">
                    <div v-if="renamingGroupId === group.id" class="flex items-center gap-2" @click.stop>
                      <input v-model="newGroupName" type="text" @keyup.enter="submitGroupRename(group.id)" @blur="submitGroupRename(group.id)" class="rename-input" v-focus />
                    </div>
                    <div v-else class="flex items-center gap-2 cursor-pointer group/name" @click.stop="startGroupRename(group.id, group.name)">
                      <span class="truncate">{{ group.name }}</span>
                      <span class="mdi mdi-pencil text-xs opacity-0 group-hover/name:opacity-100 transition-all"></span>
                    </div>
                    <span v-if="group.stream_id" class="stream-tag" :style="{ color: getStreamColor(group.stream_id), backgroundColor: `${getStreamColor(group.stream_id)}15` }">
                      {{ getStreamLabel(store.streamsById[group.stream_id] || {}) }}
                    </span>
                  </h3>
                  <p class="text-[9px] font-black text-text-muted mt-0.5 uppercase tracking-[0.1em]">{{ group.client_ids.length }} CLIENTS</p>
                </div>
              </div>

              <div class="flex items-center gap-2 shrink-0">
                <span class="mdi mdi-chevron-down text-lg text-text-muted transition-transform duration-300" :class="{'rotate-180': expandedGroups.has(group.id)}"></span>
              </div>
            </div>

            <!-- Expanded Clients -->
            <div v-if="expandedGroups.has(group.id)" class="clients-list anim-slide-up">
              <div v-for="client in getGroupClients(group.client_ids)" :key="client.id" class="client-row">
                <div class="flex items-center gap-3 min-w-0 flex-1">
                  <div class="client-icon" :class="{ 'connected': client.status === 'connected' }">
                    <span class="mdi mdi-speaker text-base"></span>
                  </div>
                  <div class="min-w-0">
                    <div class="text-xs font-semibold text-text-primary truncate">{{ client.hostname }}</div>
                    <div class="text-[9px] text-text-muted uppercase tracking-wider">{{ client.remote_addr }}</div>
                  </div>
                </div>

                <div class="flex items-center gap-3 shrink-0">
                  <VolumeControl
                    :volume="clientVolume(client.id).volume"
                    :muted="clientVolume(client.id).muted"
                    :compact="true"
                    @update:volume="setVolume(client.id, $event, clientVolume(client.id).muted)"
                    @update:muted="setVolume(client.id, clientVolume(client.id).volume, $event)"
                  />
                </div>
              </div>
              <div v-if="group.client_ids.length === 0" class="text-center py-4 text-[11px] text-text-muted italic">
                No clients assigned
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- SVG Overlay for Cables -->
    <svg class="matrix-svg" :class="{ 'dragging': isDragging }">
      <path v-for="conn in activeConnections" :key="conn.groupId"
            :d="conn.path"
            fill="none"
            :stroke="conn.color"
            :stroke-width="hoverZoneId === conn.groupId ? 6 : 4"
            stroke-linecap="round"
            class="transition-all duration-300 pointer-events-none drop-shadow-md"
            :class="[
               conn.isPlaying ? 'cable-animated opacity-90' : 'opacity-20',
               (isDragging && hoverZoneId === conn.groupId && draggedStreamId !== conn.streamId) ? 'opacity-0 scale-95' : 'opacity-100 scale-100'
            ]" />

      <path v-if="draggingPath"
            :d="draggingPath.path"
            fill="none"
            :stroke="draggingPath.color"
            stroke-width="5"
            stroke-linecap="round"
            class="cable-animated opacity-100 drop-shadow-[0_0_12px_rgba(255,255,255,0.4)] pointer-events-none" />
    </svg>
  </div>
</template>

<style scoped>
.matrix-root {
  position: relative;
  overflow-x: hidden;
  min-height: 60vh;
}

.matrix-toolbar {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 8px;
}

.matrix-svg {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  z-index: 10;
  overflow: visible;
}
.matrix-svg.dragging {
  z-index: 35;
}

/* Source Cards */
.source-card {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 1.25rem;
  padding: 1rem;
  display: flex;
  align-items: center;
  justify-content: space-between;
  position: relative;
  transition: all 0.3s ease;
  box-shadow: 0 4px 20px rgba(0,0,0,0.2);
}
.source-card.playing {
  border-color: var(--accent-border);
  background: rgba(56, 189, 248, 0.03);
}

.source-icon {
  width: 40px;
  height: 40px;
  border-radius: 12px;
  background: rgba(255,255,255,0.03);
  border: 1px solid var(--border);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

/* Zone Cards */
.zone-card {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 1.5rem;
  position: relative;
  transition: all 0.4s cubic-bezier(0.16, 1, 0.3, 1);
  box-shadow: 0 10px 30px rgba(0,0,0,0.25);
  display: flex;
  flex-direction: column;
}
.zone-card.hovered {
  border-color: var(--accent);
  background: var(--accent-dim);
  transform: scale(1.02);
  z-index: 30;
  box-shadow: 0 20px 50px rgba(0,0,0,0.4), 0 0 0 1px var(--accent);
}
.zone-card.empty { border-color: rgba(248, 113, 113, 0.2); }

.zone-content { cursor: pointer; }

.stream-tag {
  padding: 2px 8px;
  border-radius: 6px;
  font-size: 9px;
  font-weight: 800;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  background: rgba(255,255,255,0.05);
}

.rename-input {
  background: var(--bg-base);
  border: 1px solid var(--accent);
  border-radius: 6px;
  padding: 2px 8px;
  font-size: 14px;
  color: var(--text-primary);
  outline: none;
  width: 150px;
}

/* Connectors */
.connector {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: var(--bg-surface);
  border: 3px solid;
  display: flex;
  align-items: center;
  justify-content: center;
  position: absolute;
  z-index: 30;
  box-shadow: 0 0 15px rgba(0,0,0,0.5);
  transition: transform 0.2s;
}
.connector:hover { transform: scale(1.2); }

.source-connector {
  right: -10px;
  cursor: grab;
}
.source-connector:active { cursor: grabbing; }

.zone-connector {
  left: -10px;
  top: 26px;
}

.connector-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.ping { animation: ping 1.5s cubic-bezier(0, 0, 0.2, 1) infinite; }
@keyframes ping {
  75%, 100% { transform: scale(2); opacity: 0; }
}

/* Clients List */
.clients-list {
  background: rgba(0,0,0,0.2);
  padding: 1rem;
  border-top: 1px solid var(--border);
  border-bottom-left-radius: 1.5rem;
  border-bottom-right-radius: 1.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.client-row {
  background: rgba(255,255,255,0.02);
  border: 1px solid var(--border);
  border-radius: 1rem;
  padding: 0.75rem 1rem;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
}

.client-icon {
  width: 32px;
  height: 32px;
  border-radius: 8px;
  background: rgba(255,255,255,0.03);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
}
.client-icon.connected { color: var(--text-secondary); background: var(--accent-dim); }

/* SVG Cables */
.cable-animated {
  stroke-dasharray: 10 10;
  animation: flow 1s linear infinite;
}
@keyframes flow {
  from { stroke-dashoffset: 20; }
  to { stroke-dashoffset: 0; }
}

@media (max-width: 1024px) {
  .source-connector { right: -8px; }
  .zone-connector { left: -8px; }
}
</style>
