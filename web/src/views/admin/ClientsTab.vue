<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useServerStore } from '@/stores/server';
import { useAuthStore }   from '@/stores/auth';
import { api }            from '@/lib/api';
import type { EqBand, ScanResult } from '@/lib/api';
import VolumeControl      from '@/components/VolumeControl.vue';
import EqControl          from '@/components/EqControl.vue';

const store = useServerStore();
const auth  = useAuthStore();

onMounted(() => store.loadAll());

// ── Sorted client list ────────────────────────────────────────────────────
const sortedClients = computed(() =>
  [...store.clients].sort((a, b) => {
    if (a.status !== b.status) return a.status === 'connected' ? -1 : 1;
    return a.hostname.localeCompare(b.hostname);
  })
);

// ── Volume ────────────────────────────────────────────────────────────────
const debounceTimers: Record<string, ReturnType<typeof setTimeout>> = {};
const eqDebounceTimers: Record<string, ReturnType<typeof setTimeout>> = {};

function setVolume(clientId: string, volume: number, muted: boolean) {
  clearTimeout(debounceTimers[clientId]);
  debounceTimers[clientId] = setTimeout(() => {
    api.setVolume(clientId, volume, muted);
    store.clients = store.clients.map(c =>
      c.id === clientId ? { ...c, volume, muted } : c,
    );
  }, 120);
}

function setEq(clientId: string, bands: EqBand[], enabled: boolean) {
  store.clients = store.clients.map(c =>
    c.id === clientId ? { ...c, eq_bands: bands, eq_enabled: enabled } : c,
  );

  clearTimeout(eqDebounceTimers[clientId]);
  eqDebounceTimers[clientId] = setTimeout(() => {
    api.setEq(clientId, bands, enabled);
  }, 180);
}

// ── Latency ───────────────────────────────────────────────────────────────
const latencyEditing = ref<Record<string, number>>({});

function startLatencyEdit(clientId: string, current: number) {
  latencyEditing.value = { ...latencyEditing.value, [clientId]: current };
}

async function applyLatency(clientId: string) {
  const ms = latencyEditing.value[clientId];
  if (ms === undefined) return;
  await api.setLatency(clientId, ms);
  store.clients = store.clients.map(c =>
    c.id === clientId ? { ...c, latency_ms: ms } : c,
  );
  const next = { ...latencyEditing.value };
  delete next[clientId];
  latencyEditing.value = next;
}

function cancelLatency(clientId: string) {
  const next = { ...latencyEditing.value };
  delete next[clientId];
  latencyEditing.value = next;
}

// ── Move group ────────────────────────────────────────────────────────────
async function moveClient(clientId: string, groupId: string) {
  await api.setGroup(clientId, groupId);
  await store.loadAll();
}

// ── Subnet scan ───────────────────────────────────────────────────────────
const scanCidr    = ref('192.168.1.0/24');
const scanPort    = ref(1710);
const scanning    = ref(false);
const scanResults = ref<ScanResult[]>([]);
const scanDone    = ref(false);
const scanError   = ref('');

async function runScan() {
  scanning.value    = true;
  scanResults.value = [];
  scanDone.value    = false;
  scanError.value   = '';
  try {
    scanResults.value = await api.scanSubnet(scanCidr.value, scanPort.value);
    scanDone.value    = true;
  } catch (e) {
    scanError.value = String(e);
  } finally {
    scanning.value = false;
  }
}

// ── Helpers ───────────────────────────────────────────────────────────────
function osIcon(os: string) {
  if (os.toLowerCase().includes('linux'))   return 'mdi-linux';
  if (os.toLowerCase().includes('mac'))     return 'mdi-apple';
  if (os.toLowerCase().includes('windows')) return 'mdi-microsoft-windows';
  return 'mdi-devices';
}

function groupName(groupId: string) {
  return store.groups.find(g => g.id === groupId)?.name ?? groupId;
}
</script>

<template>
  <div class="space-y-7">

    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="page-title">Clients</h1>
        <p class="page-sub">All audio endpoints, connected or recently seen</p>
      </div>
      <div class="flex items-center gap-2">
        <span class="badge" style="background: var(--green-dim); color: var(--green); border: 1px solid var(--green-border); font-family: var(--font-mono); font-size: 11px;">
          {{ store.connectedClients.length }} online
        </span>
        <span class="badge" style="background: var(--bg-elevated); color: var(--text-muted); border: 1px solid var(--border-mid); font-family: var(--font-mono); font-size: 11px;">
          {{ store.clients.length }} total
        </span>
      </div>
    </div>

    <!-- Client list -->
    <div v-if="store.clients.length === 0" class="card px-5 py-12 text-center">
      <span class="mdi mdi-speaker-off text-4xl block mb-3" style="color: var(--text-muted);"></span>
      <p style="color: var(--text-muted); font-size: 13px;">No clients have connected yet.</p>
      <p style="color: var(--text-muted); font-size: 12px; margin-top: 4px;">
        Start a Sonium or Snapcast client and point it at this server.
      </p>
    </div>

    <div v-else class="space-y-2.5">
      <div
        v-for="c in sortedClients" :key="c.id"
        class="client-card"
        :class="{ 'client-offline': c.status !== 'connected' }"
      >
        <!-- Client header row -->
        <div class="flex items-start justify-between gap-3 mb-3">
          <div class="flex items-center gap-3 min-w-0">
            <!-- Status + OS icon stack -->
            <div class="relative shrink-0">
              <div class="client-avatar">
                <span class="mdi text-base" :class="osIcon(c.os)" style="color: var(--text-secondary);"></span>
              </div>
              <span
                class="client-status-dot"
                :class="c.status === 'connected' ? 'pulse-dot' : ''"
                :style="{ background: c.status === 'connected' ? 'var(--green)' : 'var(--text-muted)' }"
              ></span>
            </div>

            <!-- Client info -->
            <div class="min-w-0">
              <p class="font-semibold truncate" style="font-size: 13.5px; color: var(--text-primary);">
                {{ c.hostname }}
              </p>
              <p class="truncate" style="font-size: 11.5px; color: var(--text-muted);">
                {{ c.client_name }} · {{ c.os }} · {{ c.arch }}
              </p>
              <p class="truncate" style="font-size: 11px; color: var(--text-muted); font-family: var(--font-mono); margin-top: 1px;">
                {{ c.remote_addr }}
              </p>
            </div>
          </div>

          <!-- Right: group + status badge -->
          <div class="flex items-center gap-2 shrink-0">
            <span
              class="status-pill"
              :style="c.status === 'connected'
                ? 'background: var(--green-dim); color: var(--green); border-color: var(--green-border);'
                : 'background: var(--bg-elevated); color: var(--text-muted); border-color: var(--border-mid);'"
            >
              {{ c.status === 'connected' ? 'Online' : 'Offline' }}
            </span>
          </div>
        </div>

        <!-- Volume + group row -->
        <div class="flex items-center gap-3 mb-2">
          <div class="flex-1 min-w-0">
            <VolumeControl
              :volume="c.volume"
              :muted="c.muted"
              :compact="true"
              @update:volume="setVolume(c.id, $event, c.muted)"
              @update:muted="setVolume(c.id, c.volume, $event)"
            />
          </div>
        </div>

        <EqControl
          :client-id="c.id"
          :model-value="c.eq_bands"
          :enabled="c.eq_enabled"
          @update:model-value="setEq(c.id, $event, c.eq_enabled ?? false)"
          @update:enabled="setEq(c.id, c.eq_bands ?? [], $event)"
        />

        <!-- Latency + group assignment -->
        <div class="flex items-center gap-3 flex-wrap mt-2">
          <!-- Latency -->
          <div class="flex items-center gap-1.5">
            <span class="mdi mdi-timer-sand text-sm" style="color: var(--text-muted);"></span>
            <div v-if="latencyEditing[c.id] !== undefined" class="flex items-center gap-1">
              <input
                v-model.number="latencyEditing[c.id]"
                type="number"
                min="-2000"
                max="5000"
                step="10"
                class="latency-input"
                @keyup.enter="applyLatency(c.id)"
                @keyup.escape="cancelLatency(c.id)"
              />
              <span style="font-size: 11px; color: var(--text-muted);">ms</span>
              <button @click="applyLatency(c.id)" class="icon-btn-sm icon-btn-ok">
                <span class="mdi mdi-check text-xs"></span>
              </button>
              <button @click="cancelLatency(c.id)" class="icon-btn-sm">
                <span class="mdi mdi-close text-xs"></span>
              </button>
            </div>
            <button
              v-else
              @click="startLatencyEdit(c.id, c.latency_ms)"
              class="latency-badge"
              :title="`Latency offset: ${c.latency_ms}ms — click to adjust`"
            >
              {{ c.latency_ms > 0 ? '+' : '' }}{{ c.latency_ms }} ms
            </button>
          </div>

          <!-- Group selector -->
          <div class="flex items-center gap-1.5 ml-auto">
            <span class="mdi mdi-speaker-multiple text-sm" style="color: var(--text-muted);"></span>
            <select
              :value="c.group_id"
              @change="moveClient(c.id, ($event.target as HTMLSelectElement).value)"
              class="group-select"
            >
              <option v-for="g in store.groups" :key="g.id" :value="g.id">{{ g.name }}</option>
            </select>
          </div>
        </div>
      </div>
    </div>

    <!-- ── Subnet scanner (admin only) ───────────────────────────────────── -->
    <section v-if="auth.isAdmin" class="space-y-4">
      <div class="flex items-center gap-3">
        <h2 class="section-label">Network Discovery</h2>
        <div class="flex-1 h-px" style="background: var(--border);"></div>
      </div>

      <div class="card-elevated p-5 space-y-4">
        <div>
          <p style="font-size: 13px; font-weight: 500; color: var(--text-primary); margin-bottom: 4px;">Scan subnet for Sonium instances</p>
          <p style="font-size: 12px; color: var(--text-muted); line-height: 1.5;">
            Probe a CIDR range for hosts with the Sonium audio port open. Useful when mDNS is blocked across subnets or VLANs.
          </p>
        </div>

        <div class="flex gap-3 flex-wrap">
          <div class="flex-1 min-w-40">
            <label class="section-label mb-2 block">CIDR range</label>
            <input v-model="scanCidr" type="text" placeholder="192.168.1.0/24" class="field field-mono" />
          </div>
          <div class="w-28">
            <label class="section-label mb-2 block">Audio port</label>
            <input v-model.number="scanPort" type="number" min="1" max="65535" class="field field-mono" />
          </div>
          <div class="flex items-end">
            <button @click="runScan" :disabled="scanning || !scanCidr" class="btn-primary">
              <span v-if="scanning" class="mdi mdi-loading spin"></span>
              <span v-else class="mdi mdi-magnify"></span>
              {{ scanning ? 'Scanning…' : 'Scan' }}
            </button>
          </div>
        </div>

        <!-- Scan progress -->
        <div v-if="scanning" class="scan-progress">
          <span class="mdi mdi-radar spin" style="color: var(--accent);"></span>
          <span style="font-size: 12.5px; color: var(--text-secondary);">
            Probing {{ scanCidr }} on port {{ scanPort }}…
          </span>
        </div>

        <!-- Scan error -->
        <div v-if="scanError" style="font-size: 12.5px; padding: 10px 14px; border-radius: 9px; background: var(--red-dim); color: var(--red); border: 1px solid var(--red-border);">
          {{ scanError }}
        </div>

        <!-- Results -->
        <div v-if="scanDone && !scanning">
          <div v-if="scanResults.length === 0" class="scan-empty">
            <span class="mdi mdi-magnify-close mr-1"></span>
            No Sonium instances found on port {{ scanPort }} in {{ scanCidr }}.
          </div>
          <div v-else>
            <p class="section-label mb-3">
              Found {{ scanResults.length }} host{{ scanResults.length !== 1 ? 's' : '' }}
            </p>
            <div class="space-y-1.5">
              <div
                v-for="r in scanResults" :key="r.addr"
                class="scan-result"
              >
                <div class="flex items-center gap-2">
                  <span class="mdi mdi-server-network text-sm" style="color: var(--accent);"></span>
                  <span class="font-mono" style="font-family: var(--font-mono); font-size: 13px; color: var(--text-primary);">
                    {{ r.addr }}
                  </span>
                </div>
                <span style="font-family: var(--font-mono); font-size: 11px; color: var(--text-muted);">
                  :{{ r.port }}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>

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

.client-card {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 14px 16px;
  transition: border-color 0.15s, background 0.15s;
}
.client-card:hover { border-color: var(--border-mid); }
.client-offline { opacity: 0.5; }

.client-avatar {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  display: flex;
  align-items: center;
  justify-content: center;
}
.client-status-dot {
  position: absolute;
  bottom: -2px;
  right: -2px;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  border: 2px solid var(--bg-surface);
}

.status-pill {
  display: inline-flex;
  align-items: center;
  padding: 3px 9px;
  border-radius: 20px;
  font-size: 11px;
  font-weight: 600;
  font-family: var(--font-display);
  letter-spacing: 0.04em;
  border: 1px solid;
}

.latency-badge {
  display: inline-flex;
  align-items: center;
  padding: 3px 9px;
  border-radius: 7px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-muted);
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  cursor: pointer;
  transition: border-color 0.15s, color 0.15s;
}
.latency-badge:hover { border-color: var(--accent-border); color: var(--accent); }

.latency-input {
  width: 64px;
  background: var(--bg-elevated);
  border: 1px solid var(--accent-border);
  border-radius: 6px;
  padding: 3px 7px;
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 11px;
  outline: none;
  text-align: center;
}

.icon-btn-sm {
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 5px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  color: var(--text-muted);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.icon-btn-sm:hover { background: var(--bg-hover); color: var(--text-secondary); }
.icon-btn-ok { border-color: var(--green-border); color: var(--green); }
.icon-btn-ok:hover { background: var(--green-dim); }

.group-select {
  font-size: 12px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  border-radius: 7px;
  padding: 4px 8px;
  color: var(--text-secondary);
  cursor: pointer;
  outline: none;
  max-width: 130px;
}
.group-select:focus { border-color: var(--accent); }

/* Scan section */
.scan-progress {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-radius: 9px;
  background: var(--accent-dim);
  border: 1px solid var(--accent-border);
}
.scan-empty {
  font-size: 12.5px;
  color: var(--text-muted);
  padding: 10px 0;
  display: flex;
  align-items: center;
}
.scan-result {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 12px;
  border-radius: 9px;
  background: var(--accent-dim);
  border: 1px solid var(--accent-border);
  transition: background 0.15s;
}
.scan-result:hover { background: rgba(56, 189, 248, 0.12); }
</style>
