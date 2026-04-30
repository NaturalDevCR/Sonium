<script setup lang="ts">
import { useServerStore } from '@/stores/server';
import { api, type SystemLogOptions } from '@/lib/api';
import { ref, onMounted, onUnmounted, computed } from 'vue';

const server = useServerStore();
const logs = ref('');
const logWindow = ref<SystemLogOptions['since']>('2h');
const logPollTimer = ref<any>(null);
const logContainer = ref<HTMLElement | null>(null);
const toggling = ref<Record<string, boolean>>({});

onMounted(() => {
  refreshLogs();
  logPollTimer.value = setInterval(refreshLogs, 3000);
});

onUnmounted(() => {
  if (logPollTimer.value) clearInterval(logPollTimer.value);
});

async function refreshLogs() {
  try {
    const newLogs = await api.systemLogs({ since: logWindow.value, lines: 800 });
    const shouldScroll = logContainer.value &&
      (logContainer.value.scrollHeight - logContainer.value.scrollTop <= logContainer.value.clientHeight + 50);

    logs.value = newLogs;

    if (shouldScroll && logContainer.value) {
      setTimeout(() => {
        if (logContainer.value) {
          logContainer.value.scrollTop = logContainer.value.scrollHeight;
        }
      }, 50);
    }
  } catch (e) {
    console.error('Failed to fetch logs', e);
  }
}

const clients = computed(() => server.clients.filter(c => c.status === 'connected'));

const stats = computed(() => {
  let u = 0, o = 0, s = 0;
  for (const c of clients.value) {
    if (c.health) {
      u += c.health.underrun_count;
      o += c.health.overrun_count;
      s += c.health.stale_drop_count;
    }
  }
  return { underruns: u, overruns: o, stale: s };
});

function formatJitter(ms: number) {
  return `${ms}ms`;
}

function formatOffset(ms: number) {
  const sign = ms > 0 ? '+' : '';
  return `${sign}${ms}ms`;
}

async function setObservability(clientId: string, enabled: boolean) {
  toggling.value = { ...toggling.value, [clientId]: true };
  try {
    await api.setObservability(clientId, enabled);
  } finally {
    toggling.value = { ...toggling.value, [clientId]: false };
  }
}
</script>

<template>
  <div class="space-y-6 max-w-5xl mx-auto">
    <!-- Header -->
    <div class="flex flex-col md:flex-row md:items-center justify-between gap-4">
      <div>
        <h1 class="text-2xl font-bold text-white flex items-center gap-3">
          <span class="mdi mdi-pulse text-accent"></span>
          Observability
        </h1>
        <p class="text-slate-500 text-sm mt-1">Real-time network health and system diagnostics.</p>
      </div>
      <div class="flex gap-2">
        <button @click="refreshLogs" class="btn-secondary py-1.5 px-3 text-sm">
          <span class="mdi mdi-refresh mr-1"></span>
          Refresh
        </button>
      </div>
    </div>

    <!-- Summary Stats -->
    <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
      <div class="stat-card">
        <div class="flex items-center justify-between">
          <span class="text-slate-400 text-xs font-semibold uppercase tracking-wider">Audio Underruns</span>
          <span class="mdi mdi-waveform text-red-500/50 text-xl"></span>
        </div>
        <div class="mt-2 flex items-baseline gap-2">
          <span class="text-3xl font-bold" :class="stats.underruns > 0 ? 'text-red-400' : 'text-white'">
            {{ stats.underruns }}
          </span>
          <span class="text-slate-500 text-xs">Total dropouts</span>
        </div>
      </div>

      <div class="stat-card">
        <div class="flex items-center justify-between">
          <span class="text-slate-400 text-xs font-semibold uppercase tracking-wider">Stale Drops</span>
          <span class="mdi mdi-clock-alert-outline text-amber-500/50 text-xl"></span>
        </div>
        <div class="mt-2 flex items-baseline gap-2">
          <span class="text-3xl font-bold" :class="stats.stale > 0 ? 'text-amber-400' : 'text-white'">
            {{ stats.stale }}
          </span>
          <span class="text-slate-500 text-xs">Late packets</span>
        </div>
      </div>

      <div class="stat-card">
        <div class="flex items-center justify-between">
          <span class="text-slate-400 text-xs font-semibold uppercase tracking-wider">Active Nodes</span>
          <span class="mdi mdi-lan-connect text-emerald-500/50 text-xl"></span>
        </div>
        <div class="mt-2 flex items-baseline gap-2">
          <span class="text-3xl font-bold text-white">{{ clients.length }}</span>
          <span class="text-slate-500 text-xs">Streaming now</span>
        </div>
      </div>
    </div>

    <!-- Clients Detail -->
    <section>
      <h2 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-4 px-1">Endpoint Diagnostics</h2>
      <div class="card overflow-hidden">
        <div class="overflow-x-auto">
          <table class="w-full text-left border-collapse">
            <thead>
              <tr class="bg-slate-900/50 border-b border-slate-800">
                <th class="px-4 py-3 text-xs font-semibold text-slate-500 uppercase tracking-wider">Client</th>
                <th class="px-4 py-3 text-xs font-semibold text-slate-500 uppercase tracking-wider">Diagnostics</th>
                <th class="px-4 py-3 text-xs font-semibold text-slate-500 uppercase tracking-wider">Health</th>
                <th class="px-4 py-3 text-xs font-semibold text-slate-500 uppercase tracking-wider">Buffer</th>
                <th class="px-4 py-3 text-xs font-semibold text-slate-500 uppercase tracking-wider text-right">Jitter</th>
                <th class="px-4 py-3 text-xs font-semibold text-slate-500 uppercase tracking-wider text-right">Offset</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-slate-800">
              <tr v-for="client in clients" :key="client.id" class="hover:bg-slate-800/30 transition-colors">
                <td class="px-4 py-4">
                  <div class="flex items-center gap-3">
                    <div class="w-8 h-8 rounded-lg bg-slate-800 flex items-center justify-center text-accent">
                      <span class="mdi mdi-speaker"></span>
                    </div>
                    <div>
                      <p class="text-sm font-semibold text-white">{{ client.display_name || client.hostname }}</p>
                      <p class="text-xs text-slate-500 font-mono">{{ client.id }}</p>
                    </div>
                  </div>
                </td>
                <td class="px-4 py-4">
                  <button
                    class="diag-toggle"
                    :class="{ active: client.observability_enabled }"
                    :disabled="toggling[client.id]"
                    @click="setObservability(client.id, !client.observability_enabled)"
                  >
                    <span
                      class="mdi"
                      :class="toggling[client.id]
                        ? 'mdi-loading animate-spin'
                        : client.observability_enabled ? 'mdi-pulse' : 'mdi-pulse-off'"
                    ></span>
                    {{ client.observability_enabled ? 'On' : 'Off' }}
                  </button>
                </td>
                <td class="px-4 py-4">
                  <div class="flex gap-4">
                    <div class="flex flex-col">
                      <span class="text-[10px] text-slate-500 uppercase">Underrun</span>
                      <span class="text-sm" :class="client.health?.underrun_count ? 'text-red-400 font-bold' : 'text-slate-300'">
                        {{ client.health?.underrun_count ?? 0 }}
                      </span>
                    </div>
                    <div class="flex flex-col">
                      <span class="text-[10px] text-slate-500 uppercase">Stale</span>
                      <span class="text-sm" :class="client.health?.stale_drop_count ? 'text-amber-400 font-bold' : 'text-slate-300'">
                        {{ client.health?.stale_drop_count ?? 0 }}
                      </span>
                    </div>
                  </div>
                </td>
                <td class="px-4 py-4">
                  <div class="flex items-center gap-2">
                    <div class="flex-1 h-1.5 bg-slate-800 rounded-full overflow-hidden max-w-[60px]">
                      <div 
                        class="h-full bg-accent transition-all duration-500" 
                        :style="{ width: Math.min((client.health?.buffer_depth_ms ?? 0) / 2, 100) + '%' }"
                      ></div>
                    </div>
                    <span class="text-sm text-slate-300 font-mono">{{ client.health?.buffer_depth_ms ?? 0 }}ms</span>
                  </div>
                </td>
                <td class="px-4 py-4 text-right">
                  <span class="text-sm font-mono" :class="(client.health?.jitter_ms ?? 0) > 30 ? 'text-amber-400' : 'text-slate-300'">
                    {{ formatJitter(client.health?.jitter_ms ?? 0) }}
                  </span>
                </td>
                <td class="px-4 py-4 text-right">
                  <span class="text-sm font-mono" :class="Math.abs(client.health?.latency_ms ?? 0) > 100 ? 'text-red-400' : 'text-slate-300'">
                    {{ formatOffset(client.health?.latency_ms ?? 0) }}
                  </span>
                </td>
              </tr>
              <tr v-if="clients.length === 0">
                <td colspan="6" class="px-4 py-8 text-center text-slate-500 text-sm italic">
                  No online clients available for diagnostics.
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </section>

    <!-- Logs -->
    <section class="flex-1 flex flex-col min-h-0">
      <div class="flex items-center justify-between mb-4 px-1">
        <h2 class="text-sm font-semibold text-slate-400 uppercase tracking-wider">System Logs</h2>
        <div class="flex items-center gap-2 text-[10px] text-slate-500">
          <select
            v-model="logWindow"
            @change="refreshLogs"
            class="rounded-md border border-slate-800 bg-slate-950 px-2 py-1 text-xs text-slate-300 outline-none"
          >
            <option value="1h">Last hour</option>
            <option value="2h">Last 2 hours</option>
            <option value="6h">Last 6 hours</option>
            <option value="12h">Last 12 hours</option>
            <option value="24h">Last 24 hours</option>
            <option value="all">All</option>
          </select>
          <span class="w-1.5 h-1.5 bg-accent rounded-full animate-pulse"></span>
          Live Monitoring
        </div>
      </div>
      <div class="card bg-slate-950 border-slate-800 flex flex-col overflow-hidden" style="height: 400px;">
        <div 
          ref="logContainer"
          class="flex-1 p-4 overflow-auto font-mono text-[11px] leading-relaxed"
        >
          <div v-for="(line, i) in logs.split('\n')" :key="i" class="log-line">
            <template v-if="line.trim()">
              <span class="text-slate-600 mr-2 opacity-50">{{ i + 1 }}</span>
              <span :class="getLogLevelClass(line)">{{ line }}</span>
            </template>
          </div>
          <div v-if="!logs" class="text-slate-600 italic">Waiting for logs...</div>
        </div>
      </div>
    </section>
  </div>
</template>

<script lang="ts">
function getLogLevelClass(line: string) {
  if (line.includes('ERROR') || line.includes('error')) return 'text-red-400';
  if (line.includes('WARN') || line.includes('warn')) return 'text-amber-400';
  if (line.includes('INFO') || line.includes('info')) return 'text-blue-400';
  if (line.includes('DEBUG') || line.includes('debug')) return 'text-slate-500';
  return 'text-slate-300';
}
</script>

<style scoped>
.stat-card {
  background: linear-gradient(135deg, var(--bg-surface) 0%, rgba(30, 41, 59, 0.5) 100%);
  border: 1px solid var(--border);
  border-radius: 1rem;
  padding: 1.25rem;
  position: relative;
  overflow: hidden;
}

.stat-card::after {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 1px;
  background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.05), transparent);
}

.btn-secondary {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  color: var(--text-primary);
  border-radius: 0.5rem;
  transition: all 0.2s;
}

.btn-secondary:hover {
  background: var(--border);
  border-color: var(--text-muted);
}

.log-line {
  white-space: pre-wrap;
  word-break: break-all;
  border-left: 1px solid transparent;
  padding-left: 4px;
}

.log-line:hover {
  background: rgba(255, 255, 255, 0.03);
  border-left: 1px solid var(--accent);
}

.diag-toggle {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  min-width: 4.75rem;
  padding: 0.35rem 0.65rem;
  border-radius: 0.5rem;
  border: 1px solid var(--border);
  background: rgba(15, 23, 42, 0.8);
  color: var(--text-muted);
  font-size: 0.75rem;
  font-weight: 700;
}

.diag-toggle.active {
  border-color: rgba(34, 211, 238, 0.45);
  background: rgba(8, 145, 178, 0.12);
  color: var(--accent);
}

.diag-toggle:disabled {
  opacity: 0.55;
}

/* Custom scrollbar for logs */
::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}
::-webkit-scrollbar-track {
  background: transparent;
}
::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.1);
  border-radius: 3px;
}
::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.2);
}
</style>
