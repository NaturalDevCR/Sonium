<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useServerStore } from '@/stores/server';
import { useAuthStore } from '@/stores/auth';
import { api } from '@/lib/api';
import VolumeControl from '@/components/VolumeControl.vue';

const store = useServerStore();
const auth = useAuthStore();

const containerRef = ref<HTMLElement | null>(null);
const sourceRefs = ref<Record<string, HTMLElement>>({});
const zoneRefs = ref<Record<string, HTMLElement>>({});
const zoneCardRefs = ref<Record<string, HTMLElement>>({});

const isDragging = ref(false);
const draggedStreamId = ref<string | null>(null);
const mousePos = ref({ x: 0, y: 0 });
const hoverZoneId = ref<string | null>(null);
const expandedGroups = ref<Set<string>>(new Set());
const connectionsVersion = ref(0);

const streamColors = [
  '#22d3ee', '#a78bfa', '#34d399', '#f472b6',
  '#fbbf24', '#38bdf8', '#d946ef', '#eab308',
];

const getStreamColor = (streamId: string) => {
  const index = store.streams.findIndex(s => s.id === streamId);
  return streamColors[Math.max(0, index) % streamColors.length];
};

onMounted(() => {
  store.init();
  window.addEventListener('resize', triggerRedraw);
  document.addEventListener('mousemove', handleMouseMove);
  document.addEventListener('mouseup', handleMouseUp);
  setTimeout(triggerRedraw, 200);
});

onUnmounted(() => {
  store.stopLiveUpdates();
  window.removeEventListener('resize', triggerRedraw);
  document.removeEventListener('mousemove', handleMouseMove);
  document.removeEventListener('mouseup', handleMouseUp);
});

function triggerRedraw() { connectionsVersion.value++; }

const toggleGroup = (groupId: string) => {
  if (expandedGroups.value.has(groupId)) expandedGroups.value.delete(groupId);
  else expandedGroups.value.add(groupId);
  expandedGroups.value = new Set(expandedGroups.value);
  setTimeout(triggerRedraw, 50);
  setTimeout(triggerRedraw, 150);
  setTimeout(triggerRedraw, 300);
};

const setSourceRef = (id: string) => (el: any) => { if (el) sourceRefs.value[id] = el; };
const setZoneRef = (id: string) => (el: any) => { if (el) zoneRefs.value[id] = el; };
const setZoneCardRef = (id: string) => (el: any) => { if (el) zoneCardRefs.value[id] = el; };

const getCoords = (el: HTMLElement) => {
  const rect = el.getBoundingClientRect();
  const c = containerRef.value?.getBoundingClientRect() || { left: 0, top: 0 };
  return { x: rect.left - c.left + rect.width / 2, y: rect.top - c.top + rect.height / 2 };
};

const bezier = (x1: number, y1: number, x2: number, y2: number) => {
  const dx = Math.abs(x2 - x1);
  return `M ${x1} ${y1} C ${x1 + dx * 0.4} ${y1}, ${x2 - dx * 0.4} ${y2}, ${x2} ${y2}`;
};

const activeConnections = computed(() => {
  connectionsVersion.value;
  if (!store.groups.length || !containerRef.value) return [];
  const out: any[] = [];
  store.groups.forEach(group => {
    const sid = group.stream_id;
    const sEl = sourceRefs.value[sid];
    const zEl = zoneRefs.value[group.id];
    if (sEl && zEl) {
      const s = getCoords(sEl);
      const e = getCoords(zEl);
      out.push({ groupId: group.id, streamId: sid, path: bezier(s.x, s.y, e.x, e.y), color: getStreamColor(sid), isPlaying: store.streamsById[sid]?.status === 'playing' });
    }
  });
  return out;
});

const draggingPath = computed(() => {
  if (!isDragging.value || !draggedStreamId.value || !containerRef.value) return null;
  const sEl = sourceRefs.value[draggedStreamId.value];
  if (!sEl) return null;
  const s = getCoords(sEl);
  let ex = mousePos.value.x, ey = mousePos.value.y;
  if (hoverZoneId.value && zoneRefs.value[hoverZoneId.value]) {
    const z = getCoords(zoneRefs.value[hoverZoneId.value]);
    ex = z.x; ey = z.y;
  }
  return { path: bezier(s.x, s.y, ex, ey), color: getStreamColor(draggedStreamId.value) };
});

const startDrag = (streamId: string, e: MouseEvent) => {
  isDragging.value = true;
  draggedStreamId.value = streamId;
  updateMousePos(e.clientX, e.clientY);
};

const updateMousePos = (clientX: number, clientY: number) => {
  const c = containerRef.value;
  if (!c) return;
  const r = c.getBoundingClientRect();
  mousePos.value = { x: clientX - r.left, y: clientY - r.top };
  let found = false;
  for (const [zid, el] of Object.entries(zoneCardRefs.value)) {
    if (!el) continue;
    const zr = el.getBoundingClientRect();
    if (clientX >= zr.left - 40 && clientX <= zr.right + 20 && clientY >= zr.top - 20 && clientY <= zr.bottom + 20) {
      hoverZoneId.value = zid; found = true; break;
    }
  }
  if (!found) hoverZoneId.value = null;
};

const handleMouseMove = (e: MouseEvent) => { if (isDragging.value) updateMousePos(e.clientX, e.clientY); };

const handleMouseUp = async () => {
  if (!isDragging.value) return;
  if (draggedStreamId.value && hoverZoneId.value) {
    const g = store.groups.find(gg => gg.id === hoverZoneId.value);
    if (g && g.stream_id !== draggedStreamId.value) await api.setGroupStream(g.id, draggedStreamId.value);
  }
  isDragging.value = false;
  draggedStreamId.value = null;
  hoverZoneId.value = null;
};

const pendingVolume = ref<Record<string, { volume: number; muted: boolean }>>({});
const debTimers: Record<string, any> = {};

function setVolume(cid: string, v: number, m: boolean) {
  pendingVolume.value[cid] = { volume: v, muted: m };
  clearTimeout(debTimers[cid]);
  debTimers[cid] = setTimeout(async () => {
    const pv = pendingVolume.value[cid];
    if (!pv) return;
    delete pendingVolume.value[cid];
    await api.setVolume(cid, pv.volume, pv.muted);
  }, 120);
}

function clientVolume(cid: string) {
  return pendingVolume.value[cid] ?? { volume: store.clientsById[cid]?.volume ?? 100, muted: store.clientsById[cid]?.muted ?? false };
}

const getGroupClients = (ids: string[]) => ids.map(id => store.clientsById[id]).filter(Boolean);

const renamingGroupId = ref<string | null>(null);
const newGroupName = ref('');

const startRename = (id: string, name: string) => { if (!auth.isAdmin) return; renamingGroupId.value = id; newGroupName.value = name; };
const submitRename = async (id: string) => {
  if (!newGroupName.value || newGroupName.value === store.groups.find(g => g.id === id)?.name) { renamingGroupId.value = null; return; }
  await api.renameGroup(id, newGroupName.value);
  renamingGroupId.value = null;
};
const vFocus = { mounted: (el: HTMLInputElement) => el.focus() };
</script>

<template>
  <div class="relative" ref="containerRef">
    <!-- Toolbar -->
    <div class="flex items-center justify-end mb-4 gap-2">
      <button @click="store.loadAll()" :disabled="store.loading" class="btn-glass" title="Refresh">
        <span class="mdi mdi-refresh" :class="{ 'animate-spin': store.loading }"></span>
      </button>
    </div>

    <!-- Two column grid -->
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-8 lg:gap-16 relative z-10">
      <!-- Sources -->
      <div class="space-y-3">
        <div class="flex items-center gap-2 mb-3">
          <div class="w-8 h-8 rounded-lg bg-cyan-500/10 border border-cyan-500/20 flex items-center justify-center">
            <span class="mdi mdi-cast text-cyan-400"></span>
          </div>
          <span class="text-xs font-bold text-slate-500 uppercase tracking-wider">Sources</span>
        </div>

        <div
          v-for="stream in store.streams"
          :key="stream.id"
          class="glass p-4 flex items-center justify-between relative"
          :class="stream.status === 'playing' ? 'border-cyan-500/20' : ''"
        >
          <div class="flex items-center gap-3 min-w-0">
            <div class="w-10 h-10 rounded-xl flex items-center justify-center shrink-0" :style="`background: ${getStreamColor(stream.id)}15; border: 1px solid ${getStreamColor(stream.id)}30;`">
              <span class="mdi mdi-music-note text-lg" :style="`color: ${getStreamColor(stream.id)}`"></span>
            </div>
            <div class="min-w-0">
              <div class="text-sm font-semibold text-white truncate flex items-center gap-2">
                {{ stream.display_name || stream.id }}
                <span class="w-1.5 h-1.5 rounded-full" :class="stream.status === 'playing' ? 'animate-pulse-glow bg-cyan-400' : 'bg-slate-600'"></span>
              </div>
              <div class="text-[10px] text-slate-500 uppercase tracking-wider">{{ stream.status }}</div>
            </div>
          </div>

          <div
            :ref="setSourceRef(stream.id)"
            @mousedown.prevent="startDrag(stream.id, $event)"
            class="w-5 h-5 rounded-full border-[3px] flex items-center justify-center cursor-grab active:cursor-grabbing relative z-20"
            :style="`border-color: ${getStreamColor(stream.id)}; background: #0f172a;`"
          >
            <div class="w-2 h-2 rounded-full" :class="isDragging && draggedStreamId === stream.id ? 'animate-ping' : ''" :style="`background: ${getStreamColor(stream.id)}`"></div>
          </div>
        </div>
      </div>

      <!-- Zones -->
      <div class="space-y-3">
        <div class="flex items-center gap-2 mb-3">
          <div class="w-8 h-8 rounded-lg bg-violet-500/10 border border-violet-500/20 flex items-center justify-center">
            <span class="mdi mdi-speaker-multiple text-violet-400"></span>
          </div>
          <span class="text-xs font-bold text-slate-500 uppercase tracking-wider">Zones</span>
        </div>

        <div
          v-for="group in store.groups"
          :key="group.id"
          :ref="setZoneCardRef(group.id)"
          class="glass relative"
          :class="[hoverZoneId === group.id ? 'border-cyan-400/40 shadow-[0_0_30px_rgba(34,211,238,0.1)] scale-[1.01]' : '', !group.stream_id ? 'border-rose-500/20' : '']"
        >
          <div
            :ref="setZoneRef(group.id)"
            class="absolute left-[-10px] top-5 w-5 h-5 rounded-full border-[3px] flex items-center justify-center z-20"
            :style="`border-color: ${group.stream_id ? getStreamColor(group.stream_id) : '#475569'}; background: #0f172a;`"
          >
            <div class="w-2 h-2 rounded-full" :class="hoverZoneId === group.id ? 'animate-ping' : ''" :style="`background: ${group.stream_id ? getStreamColor(group.stream_id) : '#475569'}`"></div>
          </div>

          <div class="p-4 cursor-pointer" @click="toggleGroup(group.id)">
            <div class="flex items-center justify-between gap-3">
              <div class="flex items-center gap-3 min-w-0 ml-4">
                <div class="min-w-0">
                  <div class="text-sm font-semibold text-white truncate flex items-center gap-2">
                    <div v-if="renamingGroupId === group.id" @click.stop>
                      <input v-model="newGroupName" type="text" @keyup.enter="submitRename(group.id)" @blur="submitRename(group.id)" class="input-glass text-sm py-1 w-40" v-focus />
                    </div>
                    <div v-else class="flex items-center gap-2 group" @click.stop="startRename(group.id, group.name)">
                      <span class="truncate">{{ group.name }}</span>
                      <span class="mdi mdi-pencil text-[10px] text-slate-600 opacity-0 group-hover:opacity-100 transition-opacity"></span>
                    </div>
                    <span v-if="group.stream_id" class="px-2 py-0.5 rounded-md text-[9px] font-bold uppercase tracking-wider" :style="`color: ${getStreamColor(group.stream_id)}; background: ${getStreamColor(group.stream_id)}15;`">
                      {{ store.streamsById[group.stream_id]?.display_name || group.stream_id }}
                    </span>
                  </div>
                  <div class="text-[10px] text-slate-500 uppercase tracking-wider mt-0.5">{{ group.client_ids.length }} clients</div>
                </div>
              </div>
              <span class="mdi mdi-chevron-down text-slate-500 transition-transform" :class="expandedGroups.has(group.id) ? 'rotate-180' : ''"></span>
            </div>
          </div>

          <div v-if="expandedGroups.has(group.id)" class="px-4 pb-4 space-y-2 animate-fade-up">
            <div
              v-for="client in getGroupClients(group.client_ids)"
              :key="client.id"
              class="flex items-center justify-between gap-3 p-3 rounded-xl bg-white/[0.02] border border-white/[0.04]"
            >
              <div class="flex items-center gap-2.5 min-w-0">
                <div class="w-8 h-8 rounded-lg bg-white/[0.03] flex items-center justify-center text-slate-500">
                  <span class="mdi mdi-speaker text-sm"></span>
                </div>
                <div class="min-w-0">
                  <div class="text-xs font-semibold text-slate-200 truncate">{{ client.hostname }}</div>
                  <div class="text-[9px] text-slate-600">{{ client.remote_addr }}</div>
                </div>
              </div>
              <VolumeControl
                :volume="clientVolume(client.id).volume"
                :muted="clientVolume(client.id).muted"
                :compact="true"
                @update:volume="setVolume(client.id, $event, clientVolume(client.id).muted)"
                @update:muted="setVolume(client.id, clientVolume(client.id).volume, $event)"
              />
            </div>
            <div v-if="group.client_ids.length === 0" class="text-center py-3 text-xs text-slate-600 italic">No clients assigned</div>
          </div>
        </div>
      </div>
    </div>

    <!-- SVG Cables -->
    <svg class="absolute inset-0 w-full h-full pointer-events-none z-[5] overflow-visible" :class="{ 'z-[15]': isDragging }">
      <path
        v-for="conn in activeConnections"
        :key="conn.groupId"
        :d="conn.path"
        fill="none"
        :stroke="conn.color"
        :stroke-width="hoverZoneId === conn.groupId ? 5 : 3"
        stroke-linecap="round"
        class="transition-all duration-300"
        :class="conn.isPlaying ? 'opacity-80' : 'opacity-15'"
      />
      <path
        v-if="draggingPath"
        :d="draggingPath.path"
        fill="none"
        :stroke="draggingPath.color"
        stroke-width="4"
        stroke-linecap="round"
        class="opacity-90"
        stroke-dasharray="6 4"
      />
    </svg>
  </div>
</template>
