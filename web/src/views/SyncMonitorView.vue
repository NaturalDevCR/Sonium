<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useServerStore } from '@/stores/server';
import { useAuthStore }   from '@/stores/auth';
import SyncIndicator      from '@/components/SyncIndicator.vue';

const store  = useServerStore();
const auth   = useAuthStore();

onMounted(() => store.init());
onUnmounted(() => store.stopLiveUpdates());

// ── Sync health ────────────────────────────────────────────────────────────
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
</script>

<template>
  <div class="sync-root">
    <!-- ── Header row ───────────────────────────────────────────────────── -->
    <div class="flex items-center justify-between mb-4">
      <SyncIndicator
        :status="overallSync.status"
        :issue-count="overallSync.issueCount"
        :total-count="overallSync.totalCount"
      />
    </div>

    <!-- ── Content ──────────────────────────────────────────────────────── -->
    <main>
      <!-- Chrony recommendation if sync is poor -->
      <div
        v-if="overallSync.status === 'poor' || overallSync.status === 'fair'"
        class="chrony-banner"
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
              class="chrony-btn"
            >
              <span class="mdi mr-1" :class="copied ? 'mdi-check' : 'mdi-content-copy'"></span>
              {{ copied ? 'Copied!' : 'Copy install command' }}
            </button>
          </div>
        </div>
      </div>

      <!-- Client sync table -->
      <div class="pt-2 pb-8 space-y-3">
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

          <div class="sync-grid">
            <div>
              <p class="sync-label">Drift</p>
              <p class="sync-value">
                {{ syncHealth[client.id]?.drift_ms.toFixed(1) ?? '—' }} ms
              </p>
            </div>
            <div>
              <p class="sync-label">Buffer</p>
              <p class="sync-value">
                {{ syncHealth[client.id]?.buffer_ms.toFixed(0) ?? '—' }} ms
              </p>
            </div>
            <div>
              <p class="sync-label">Latency</p>
              <p class="sync-value">
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
  max-width: 720px;
  margin: 0 auto;
}

.chrony-banner {
  margin-bottom: 16px;
  padding: 16px;
  border-radius: 12px;
  background: rgba(245, 158, 11, 0.08);
  border: 1px solid rgba(245, 158, 11, 0.3);
}
.chrony-btn {
  margin-top: 8px;
  font-size: 12px;
  font-weight: 500;
  padding: 6px 12px;
  border-radius: 8px;
  border: 1px solid rgba(245, 158, 11, 0.4);
  color: #fbbf24;
  background: transparent;
  cursor: pointer;
  transition: all 0.15s;
}
.chrony-btn:hover {
  background: rgba(245, 158, 11, 0.1);
}

.client-sync-card {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 14px 16px;
  transition: border-color 0.2s;
}
.client-sync-card:hover { border-color: var(--border-mid); }

.sync-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
}
.sync-label {
  font-size: 11px;
  color: var(--text-muted);
  margin-bottom: 2px;
}
.sync-value {
  font-size: 13px;
  font-family: var(--font-mono);
  font-weight: 500;
  color: var(--text-primary);
}
</style>
