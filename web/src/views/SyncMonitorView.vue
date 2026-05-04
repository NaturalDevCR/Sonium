<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useRouter } from 'vue-router';
import { useServerStore } from '@/stores/server';
import { useAuthStore }   from '@/stores/auth';
import SyncIndicator      from '@/components/SyncIndicator.vue';

const store  = useServerStore();
const auth   = useAuthStore();
const router = useRouter();

onMounted(async () => {
  await store.loadAll();
  store.startLiveUpdates();
});
onUnmounted(() => store.stopLiveUpdates());

// ── Sync health (mock — will be real when API provides it)
const syncHealth = computed(() => {
  const health: Record<string, { status: 'good' | 'fair' | 'poor' | 'unknown'; drift_ms: number; buffer_ms: number }> = {};
  for (const c of store.connectedClients) {
    const jitter = c.health?.jitter_ms ?? 0;
    const buffer = c.health?.buffer_depth_ms ?? 0;
    if (jitter === 0) {
      health[c.id] = { status: 'unknown', drift_ms: 0, buffer_ms: buffer };
    } else if (jitter < 10) {
      health[c.id] = { status: 'good', drift_ms: jitter, buffer_ms: buffer };
    } else if (jitter < 50) {
      health[c.id] = { status: 'fair', drift_ms: jitter, buffer_ms: buffer };
    } else {
      health[c.id] = { status: 'poor', drift_ms: jitter, buffer_ms: buffer };
    }
  }
  return health;
});

const overallSync = computed(() => {
  const clients = store.connectedClients;
  if (clients.length === 0) return { status: 'unknown' as const, issueCount: 0, totalCount: 0 };
  const healths = clients.map(c => syncHealth.value[c.id]?.status ?? 'unknown');
  const poor = healths.filter(h => h === 'poor').length;
  const fair = healths.filter(h => h === 'fair').length;
  if (poor > 0) return { status: 'poor' as const, issueCount: poor + fair, totalCount: clients.length };
  if (fair > 0) return { status: 'fair' as const, issueCount: fair, totalCount: clients.length };
  if (healths.every(h => h === 'good')) return { status: 'good' as const, issueCount: 0, totalCount: clients.length };
  return { status: 'unknown' as const, issueCount: 0, totalCount: clients.length };
});

function copyChronyCommand() {
  const cmd = 'sudo apt-get install chrony  # Debian/Ubuntu\nsudo systemctl enable --now chronyd';
  navigator.clipboard.writeText(cmd);
  copied.value = true;
  setTimeout(() => copied.value = false, 2000);
}

const copied = ref(false);

// ── Uptime ─────────────────────────────────────────────────────────────────
function fmtUptime(s: number) {
  if (!s) return '—';
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}
</script>

<template>
  <div class="sync-root safe-top">

    <!-- ── Top bar ──────────────────────────────────────────────────────── -->
    <header class="sync-header">
      <div class="sync-header-inner">
        <div class="flex items-center gap-3">
          <button @click="router.push('/')" class="sync-back">
            <span class="mdi mdi-arrow-left text-lg"></span>
          </button>
          <div>
            <p class="sync-brand">Sync Monitor</p>
            <p class="sync-tagline">
              {{ store.connectedClients.length }} client{{ store.connectedClients.length !== 1 ? 's' : '' }} online
              <span v-if="store.uptime" class="ml-2 opacity-60">· up {{ fmtUptime(store.uptime) }}</span>
            </p>
          </div>
        </div>

        <div class="flex items-center gap-2">
          <SyncIndicator
            :status="overallSync.status"
            :issue-count="overallSync.issueCount"
            :total-count="overallSync.totalCount"
          />
        </div>
      </div>
    </header>

    <!-- ── Content ──────────────────────────────────────────────────────── -->
    <main class="sync-main safe-bottom">

      <!-- Chrony recommendation if sync is poor -->
      <div
        v-if="overallSync.status === 'poor' || overallSync.status === 'fair'"
        class="mx-4 mt-4 p-4 rounded-xl border"
        style="background: rgba(245, 158, 11, 0.08); border-color: rgba(245, 158, 11, 0.3);"
      >
        <div class="flex items-start gap-3">
          <span class="mdi mdi-information-outline text-xl shrink-0" style="color: #f59e0b;"></span>
          <div>
            <p class="font-semibold text-sm" style="color: #fbbf24;">Sync could be better</p>
            <p class="text-xs mt-1" style="color: var(--text-muted);">
              For sample-accurate multi-room sync, install chrony on all devices.
            </p>
            <button
              @click="copyChronyCommand"
              class="mt-2 text-xs font-medium px-3 py-1.5 rounded-lg border"
              style="border-color: rgba(245, 158, 11, 0.4); color: #fbbf24;"
            >
              <span class="mdi mr-1" :class="copied ? 'mdi-check' : 'mdi-content-copy'"></span>
              {{ copied ? 'Copied!' : 'Copy install command' }}
            </button>
          </div>
        </div>
      </div>

      <!-- Client sync table -->
      <div class="px-4 pt-4 pb-24 space-y-3">
        <h2 class="section-label">Client Sync Status</h2>

        <div v-if="store.connectedClients.length === 0" class="text-center py-12">
          <span class="mdi mdi-speaker-off text-4xl block mb-3" style="color: var(--text-muted);"></span>
          <p style="color: var(--text-muted); font-size: 13px;">No clients connected</p>
        </div>

        <div
          v-for="client in store.connectedClients"
          :key="client.id"
          class="client-sync-card"
        >
          <div class="flex items-center justify-between gap-3">
            <div class="flex items-center gap-2.5 min-w-0">
              <span
                class="w-2 h-2 rounded-full shrink-0"
                :style="{ background: syncHealth[client.id]?.status === 'good' ? '#22c55e' :
                                  syncHealth[client.id]?.status === 'fair' ? '#f59e0b' :
                                  syncHealth[client.id]?.status === 'poor' ? '#ef4444' :
                                  'var(--text-muted)' }"
              ></span>
              <span class="font-medium truncate" style="font-size: 13px; color: var(--text-primary);">
                {{ client.hostname }}
              </span>
            </div>
            <SyncIndicator
              :status="syncHealth[client.id]?.status ?? 'unknown'"
              :issue-count="0"
              :total-count="1"
            />
          </div>

          <div class="grid grid-cols-3 gap-3 mt-3 pt-3 border-t" style="border-color: var(--border);">
            <div>
              <p class="text-xs" style="color: var(--text-muted);">Drift</p>
              <p class="text-sm font-mono font-medium" style="color: var(--text-primary);">
                {{ syncHealth[client.id]?.drift_ms.toFixed(1) ?? '—' }} ms
              </p>
            </div>
            <div>
              <p class="text-xs" style="color: var(--text-muted);">Buffer</p>
              <p class="text-sm font-mono font-medium" style="color: var(--text-primary);">
                {{ syncHealth[client.id]?.buffer_ms.toFixed(0) ?? '—' }} ms
              </p>
            </div>
            <div>
              <p class="text-xs" style="color: var(--text-muted);">Latency</p>
              <p class="text-sm font-mono font-medium" style="color: var(--text-primary);">
                {{ client.latency_ms }} ms
              </p>
            </div>
          </div>
        </div>
      </div>
    </main>

  </div>
</template>

<style scoped>
.sync-root {
  min-height: 100vh;
  background: var(--bg-base);
  position: relative;
}

.sync-header {
  position: sticky;
  top: 0;
  z-index: 20;
  background: rgba(4, 8, 15, 0.88);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-bottom: 1px solid var(--border);
}
.sync-header-inner {
  max-width: 720px;
  margin: 0 auto;
  padding: 12px 16px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.sync-back {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
  background: transparent;
  border: none;
  cursor: pointer;
  transition: all 0.15s;
}
.sync-back:hover {
  background: var(--bg-elevated);
  color: var(--text-secondary);
}

.sync-brand {
  font-family: var(--font-display);
  font-size: 14px;
  font-weight: 700;
  color: var(--text-primary);
  line-height: 1;
}
.sync-tagline {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 1px;
}

.sync-main { max-width: 720px; margin: 0 auto; }

.client-sync-card {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 14px 16px;
  transition: border-color 0.2s;
}
.client-sync-card:hover { border-color: var(--border-mid); }
</style>
