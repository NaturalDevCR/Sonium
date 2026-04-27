<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import { api, type DependencyActionResult, type SystemInfo } from '@/lib/api';

const info = ref<SystemInfo | null>(null);
const logs = ref('');
const loading = ref(true);
const systemError = ref('');
const logError = ref('');
const busyAction = ref('');
const actionResult = ref<DependencyActionResult | null>(null);
const actionError = ref('');

let logPollTimer: ReturnType<typeof setInterval> | null = null;

onMounted(async () => {
  loading.value = true;
  systemError.value = '';
  try {
    info.value = await api.systemInfo();
    await refreshLogs();
  } catch (e) {
    systemError.value = String(e);
  } finally {
    loading.value = false;
  }
  logPollTimer = setInterval(refreshLogs, 10_000);
});

onUnmounted(() => {
  if (logPollTimer !== null) clearInterval(logPollTimer);
});

async function refreshLogs() {
  logError.value = '';
  try {
    logs.value = await api.systemLogs();
  } catch (e) {
    logs.value = '';
    logError.value = String(e);
  }
}

async function runDependencyAction(id: string, action: 'install' | 'update' | 'remove') {
  busyAction.value = `${id}:${action}`;
  actionResult.value = null;
  actionError.value = '';
  try {
    actionResult.value = await api.dependencyAction(id, action);
    info.value = await api.systemInfo();
  } catch (e) {
    actionError.value = String(e);
  } finally {
    busyAction.value = '';
  }
}
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <h1 class="text-xl font-bold text-white">System</h1>
      <button
        @click="refreshLogs"
        class="px-3 py-1.5 rounded-lg border border-slate-700 text-slate-300 hover:bg-slate-800 text-sm transition-colors"
      >
        Refresh logs
      </button>
    </div>

    <div v-if="loading" class="card p-5 text-slate-500">Loading system details...</div>

    <div v-else-if="systemError" class="card p-5 space-y-3 border-amber-800/50 bg-amber-950/10">
      <div class="flex items-start gap-3">
        <span class="mdi mdi-alert-outline text-amber-400 text-xl shrink-0"></span>
        <div>
          <p class="font-semibold text-amber-300">System API is not available yet</p>
          <p class="text-sm text-slate-400 mt-1">
            The web UI is loaded, but the server answered the System request with a non-JSON response.
            Restart or rebuild <code class="text-slate-300">sonium-server</code> so it includes the new
            <code class="text-slate-300">/api/system/info</code> route.
          </p>
        </div>
      </div>
      <pre class="max-h-40 overflow-auto rounded-lg bg-slate-950/70 border border-slate-800 p-3 text-xs text-slate-400 whitespace-pre-wrap">{{ systemError }}</pre>
    </div>

    <template v-else-if="info">
      <div class="grid sm:grid-cols-3 gap-3">
        <div class="card p-4">
          <p class="text-xs text-slate-500 uppercase tracking-wider">Platform</p>
          <p class="mt-2 text-white font-semibold">{{ info.os }} / {{ info.arch }}</p>
        </div>
        <div class="card p-4">
          <p class="text-xs text-slate-500 uppercase tracking-wider">Audio stack</p>
          <p class="mt-2 text-white font-semibold">{{ info.audio_stack.join(', ') || 'Unknown' }}</p>
        </div>
        <div class="card p-4">
          <p class="text-xs text-slate-500 uppercase tracking-wider">Package manager</p>
          <p class="mt-2 text-white font-semibold">{{ info.package_manager || 'Manual' }}</p>
        </div>
      </div>

      <section>
        <h2 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">Prerequisites & addons</h2>
        <div class="card divide-y divide-slate-800">
          <div
            v-for="dep in info.dependencies"
            :key="dep.id"
            class="p-4 space-y-3"
          >
            <div class="flex items-start justify-between gap-4">
              <div>
                <p class="font-semibold text-white">{{ dep.label }}</p>
                <p class="text-sm text-slate-500">{{ dep.purpose }}</p>
                <p v-if="dep.version" class="text-xs text-slate-600 mt-1 font-mono">{{ dep.version }}</p>
              </div>
              <span
                class="badge border"
                :class="dep.installed
                  ? 'bg-green-500/10 text-green-400 border-green-500/30'
                  : 'bg-amber-500/10 text-amber-400 border-amber-500/30'"
              >
                {{ dep.installed ? 'installed' : 'missing' }}
              </span>
            </div>

            <div class="grid md:grid-cols-3 gap-2 text-xs">
              <button
                @click="runDependencyAction(dep.id, 'install')"
                :disabled="busyAction !== '' || !dep.install_hint"
                class="rounded-lg border border-slate-700 px-3 py-2 text-slate-300 hover:bg-slate-800 disabled:opacity-40 transition-colors text-left"
              >
                <span class="mdi mdi-download mr-1"></span>
                Install
              </button>
              <button
                @click="runDependencyAction(dep.id, 'update')"
                :disabled="busyAction !== '' || !dep.update_hint"
                class="rounded-lg border border-slate-700 px-3 py-2 text-slate-300 hover:bg-slate-800 disabled:opacity-40 transition-colors text-left"
              >
                <span class="mdi mdi-update mr-1"></span>
                Update
              </button>
              <button
                @click="runDependencyAction(dep.id, 'remove')"
                :disabled="busyAction !== '' || !dep.remove_hint"
                class="rounded-lg border border-red-900/50 px-3 py-2 text-red-300 hover:bg-red-950/30 disabled:opacity-40 transition-colors text-left"
              >
                <span class="mdi mdi-delete-outline mr-1"></span>
                Remove
              </button>
            </div>

            <div class="grid md:grid-cols-3 gap-2 text-xs">
              <code class="block rounded-lg bg-slate-950 border border-slate-800 px-3 py-2 text-slate-500 overflow-x-auto">
                {{ dep.install_hint || `Install ${dep.binary} manually` }}
              </code>
              <code class="block rounded-lg bg-slate-950 border border-slate-800 px-3 py-2 text-slate-500 overflow-x-auto">
                {{ dep.update_hint || `Update ${dep.binary} manually` }}
              </code>
              <code class="block rounded-lg bg-slate-950 border border-slate-800 px-3 py-2 text-slate-500 overflow-x-auto">
                {{ dep.remove_hint || `Remove ${dep.binary} manually` }}
              </code>
            </div>
          </div>
        </div>
        <p class="text-xs text-slate-500 mt-2">
          Package actions are admin-only and allowlisted. Linux package managers use non-interactive sudo and will fail if the Sonium service is not allowed to run them.
        </p>

        <div v-if="actionError" class="mt-3 p-3 rounded-lg bg-red-950/30 border border-red-900/50 text-red-300 text-sm">
          {{ actionError }}
        </div>

        <div v-if="actionResult" class="mt-3 card overflow-hidden">
          <div class="px-4 py-3 border-b border-slate-800 flex items-center justify-between gap-3">
            <code class="text-xs text-slate-400 overflow-x-auto">{{ actionResult.command }}</code>
            <span
              class="badge border"
              :class="actionResult.success
                ? 'bg-green-500/10 text-green-400 border-green-500/30'
                : 'bg-red-500/10 text-red-400 border-red-500/30'"
            >
              {{ actionResult.success ? 'success' : `failed ${actionResult.status ?? ''}` }}
            </span>
          </div>
          <pre class="max-h-80 overflow-auto p-4 text-xs text-slate-300 bg-slate-950/70 font-mono whitespace-pre-wrap">{{ [actionResult.stdout, actionResult.stderr].filter(Boolean).join('\n') || 'No output.' }}</pre>
        </div>
      </section>

      <section>
        <h2 class="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">Logs</h2>
        <div class="card overflow-hidden">
          <div v-if="logError" class="p-4 text-sm text-amber-400">{{ logError }}</div>
          <pre v-else class="max-h-96 overflow-auto p-4 text-xs text-slate-300 bg-slate-950/70 font-mono whitespace-pre-wrap">{{ logs || 'No logs available yet.' }}</pre>
        </div>
      </section>
    </template>
  </div>
</template>
