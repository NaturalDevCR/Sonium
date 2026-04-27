<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useServerStore } from '@/stores/server';
import { useAuthStore }   from '@/stores/auth';
import { api }            from '@/lib/api';
import StreamBadge        from '@/components/StreamBadge.vue';
import VolumeControl      from '@/components/VolumeControl.vue';

const store = useServerStore();
const auth  = useAuthStore();

onMounted(() => store.loadAll());

// ── Create group ──────────────────────────────────────────────────────────
const showCreate  = ref(false);
const newName     = ref('');
const newStreamId = ref('');
const creating    = ref(false);

async function createGroup() {
  if (!newName.value || !newStreamId.value) return;
  creating.value = true;
  try {
    await api.createGroup(newName.value, newStreamId.value);
    await store.loadAll();
    showCreate.value = false;
    newName.value    = '';
    newStreamId.value = '';
  } finally {
    creating.value = false;
  }
}

// ── Delete group ──────────────────────────────────────────────────────────
const confirmDelete = ref<string | null>(null);

async function deleteGroup(id: string) {
  await api.deleteGroup(id);
  await store.loadAll();
  confirmDelete.value = null;
}

// ── Change stream ─────────────────────────────────────────────────────────
async function setStream(groupId: string, streamId: string) {
  await api.setGroupStream(groupId, streamId);
  store.groups = store.groups.map(g =>
    g.id === groupId ? { ...g, stream_id: streamId } : g,
  );
}

// ── Move client ───────────────────────────────────────────────────────────
async function moveClient(clientId: string, groupId: string) {
  await api.setGroup(clientId, groupId);
  await store.loadAll();
}

// ── Volume ────────────────────────────────────────────────────────────────
const debounceTimers: Record<string, ReturnType<typeof setTimeout>> = {};

function setVolume(clientId: string, volume: number, muted: boolean) {
  clearTimeout(debounceTimers[clientId]);
  debounceTimers[clientId] = setTimeout(() => {
    api.setVolume(clientId, volume, muted);
    store.clients = store.clients.map(c =>
      c.id === clientId ? { ...c, volume, muted } : c,
    );
  }, 120);
}

// ── Grouped view ──────────────────────────────────────────────────────────
const groupedClients = computed(() =>
  store.groups.map(g => ({
    group:   g,
    clients: g.client_ids.map(id => store.clientsById[id]).filter(Boolean),
    stream:  store.streamsById[g.stream_id],
  })),
);

const ungroupedClients = computed(() =>
  store.clients.filter(c => !store.groups.some(g => g.client_ids.includes(c.id))),
);
</script>

<template>
  <div class="space-y-6">

    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="page-title">Groups</h1>
        <p class="page-sub">Rooms and zones sharing a stream</p>
      </div>
      <button v-if="auth.isAdmin" @click="showCreate = !showCreate" class="btn-primary">
        <span class="mdi mdi-plus"></span>
        New group
      </button>
    </div>

    <!-- Create panel -->
    <Transition name="slide">
      <div v-if="showCreate && auth.isAdmin" class="card-elevated p-5 space-y-4">
        <h2 style="font-family: var(--font-display); font-size: 15px; font-weight: 700; color: var(--text-primary);">Create group</h2>
        <div class="grid sm:grid-cols-2 gap-3">
          <div>
            <label class="section-label mb-2 block">Group name</label>
            <input v-model="newName" type="text" placeholder="Living Room" class="field" />
          </div>
          <div>
            <label class="section-label mb-2 block">Stream</label>
            <select v-model="newStreamId" class="field">
              <option value="" disabled>Select a stream…</option>
              <option v-for="s in store.streams" :key="s.id" :value="s.id">
                {{ s.display_name || s.id }}
              </option>
            </select>
          </div>
        </div>
        <div class="flex gap-2.5">
          <button @click="showCreate = false" class="btn-ghost flex-1 justify-center">Cancel</button>
          <button @click="createGroup" :disabled="!newName || !newStreamId || creating" class="btn-primary flex-1 justify-center">
            <span v-if="creating" class="mdi mdi-loading spin"></span>
            {{ creating ? 'Creating…' : 'Create group' }}
          </button>
        </div>
      </div>
    </Transition>

    <!-- Empty state -->
    <div v-if="groupedClients.length === 0" class="card px-5 py-12 text-center">
      <span class="mdi mdi-speaker-off text-4xl block mb-3" style="color: var(--text-muted);"></span>
      <p style="color: var(--text-muted); font-size: 13px;">No groups configured yet.</p>
    </div>

    <!-- Group cards -->
    <div v-for="{ group, clients, stream } in groupedClients" :key="group.id" class="card overflow-hidden">

      <!-- Group header -->
      <div class="group-header">
        <div class="flex items-center gap-3 min-w-0">
          <div class="group-icon">
            <span class="mdi mdi-speaker-multiple text-sm" style="color: var(--accent);"></span>
          </div>
          <div class="min-w-0">
            <p class="font-semibold truncate" style="font-size: 14px; color: var(--text-primary);">{{ group.name }}</p>
            <p style="font-size: 11px; color: var(--text-muted);">
              {{ clients.filter(c => c.status === 'connected').length }} of {{ clients.length }} online
            </p>
          </div>
        </div>

        <div class="flex items-center gap-2 shrink-0">
          <StreamBadge v-if="stream" :status="stream.status" />

          <!-- Stream selector -->
          <select
            :value="group.stream_id"
            @change="setStream(group.id, ($event.target as HTMLSelectElement).value)"
            class="stream-select"
          >
            <option v-for="s in store.streams" :key="s.id" :value="s.id">
              {{ s.display_name || s.id }}
            </option>
          </select>

          <!-- Delete -->
          <button
            v-if="auth.isAdmin"
            @click="confirmDelete = group.id"
            class="icon-btn icon-btn-danger"
            title="Delete group"
          >
            <span class="mdi mdi-delete-outline text-base"></span>
          </button>
        </div>
      </div>

      <!-- Clients -->
      <div v-if="clients.length === 0" class="px-4 py-3" style="font-size: 12.5px; color: var(--text-muted); font-style: italic;">
        No clients in this group
      </div>

      <div
        v-for="c in clients" :key="c.id"
        class="client-row"
        :style="c.status !== 'connected' ? 'opacity: 0.4;' : ''"
      >
        <div class="flex items-center justify-between gap-3 mb-2">
          <div class="flex items-center gap-2 min-w-0">
            <span
              class="w-1.5 h-1.5 rounded-full shrink-0"
              :class="c.status === 'connected' ? 'pulse-dot' : ''"
              :style="{ background: c.status === 'connected' ? 'var(--green)' : 'var(--text-muted)' }"
            ></span>
            <p class="font-medium truncate" style="font-size: 13px; color: var(--text-primary);">
              {{ c.hostname }}
            </p>
            <span style="font-size: 11px; color: var(--text-muted);" class="hidden sm:inline">{{ c.os }}</span>
          </div>

          <select
            v-if="store.groups.length > 1"
            :value="c.group_id"
            @change="moveClient(c.id, ($event.target as HTMLSelectElement).value)"
            class="stream-select"
          >
            <option v-for="g in store.groups" :key="g.id" :value="g.id">{{ g.name }}</option>
          </select>
        </div>

        <VolumeControl
          :volume="c.volume"
          :muted="c.muted"
          :compact="true"
          @update:volume="setVolume(c.id, $event, c.muted)"
          @update:muted="setVolume(c.id, c.volume, $event)"
        />
      </div>
    </div>

    <!-- Ungrouped clients -->
    <div v-if="ungroupedClients.length > 0">
      <h2 class="section-label mb-3">Ungrouped clients</h2>
      <div class="card divide-y" style="border-color: var(--border);">
        <div
          v-for="c in ungroupedClients" :key="c.id"
          class="flex items-center justify-between px-4 py-3 gap-3"
        >
          <div class="flex items-center gap-2 min-w-0">
            <span class="w-1.5 h-1.5 rounded-full shrink-0" style="background: var(--text-muted);"></span>
            <p class="text-sm truncate" style="color: var(--text-primary);">{{ c.hostname }}</p>
          </div>
          <select
            v-if="store.groups.length > 0"
            @change="moveClient(c.id, ($event.target as HTMLSelectElement).value)"
            class="stream-select"
          >
            <option value="" disabled selected>Assign to group…</option>
            <option v-for="g in store.groups" :key="g.id" :value="g.id">{{ g.name }}</option>
          </select>
        </div>
      </div>
    </div>

    <!-- Delete confirmation modal -->
    <Teleport to="body">
      <Transition name="fade">
        <div
          v-if="confirmDelete"
          class="dialog-overlay"
          @click.self="confirmDelete = null"
        >
          <div class="card-elevated p-5 w-full max-w-sm space-y-4 anim-scale-in" style="border-color: var(--border-mid);">
            <h3 style="font-family: var(--font-display); font-size: 16px; font-weight: 700; color: var(--text-primary);">Delete group?</h3>
            <p style="font-size: 13px; color: var(--text-secondary);">Clients will be moved to the default group. This cannot be undone.</p>
            <div class="flex gap-2.5">
              <button @click="confirmDelete = null" class="btn-ghost flex-1 justify-center">Cancel</button>
              <button @click="deleteGroup(confirmDelete!)" class="btn-danger flex-1 justify-center">Delete</button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.page-title {
  font-family: var(--font-display);
  font-size: 22px;
  font-weight: 700;
  color: var(--text-primary);
  letter-spacing: -0.01em;
}
.page-sub { font-size: 13px; color: var(--text-muted); }

.group-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  gap: 12px;
  border-bottom: 1px solid var(--border);
}
.group-icon {
  width: 32px;
  height: 32px;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-dim);
  flex-shrink: 0;
}
.client-row {
  padding: 12px 18px;
  border-top: 1px solid var(--border);
}
.stream-select {
  font-size: 12px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  border-radius: 7px;
  padding: 4px 8px;
  color: var(--text-secondary);
  cursor: pointer;
  outline: none;
  flex-shrink: 0;
  max-width: 130px;
}
.stream-select:focus { border-color: var(--accent); }

.icon-btn {
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 7px;
  cursor: pointer;
  border: none;
  background: transparent;
  transition: background 0.15s, color 0.15s;
  color: var(--text-muted);
}
.icon-btn:hover { background: var(--bg-hover); color: var(--text-secondary); }
.icon-btn-danger:hover { background: var(--red-dim); color: var(--red); }

.dialog-overlay {
  position: fixed;
  inset: 0;
  z-index: 50;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  background: rgba(2, 5, 10, 0.8);
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
}
.divide-y > * + * { border-top: 1px solid var(--border); }

.slide-enter-active, .slide-leave-active { transition: all 0.2s ease; }
.slide-enter-from, .slide-leave-to { opacity: 0; transform: translateY(-8px); }
.fade-enter-active, .fade-leave-active { transition: opacity 0.2s; }
.fade-enter-from, .fade-leave-to { opacity: 0; }
</style>
