<script setup lang="ts">
import { ref, shallowRef, onMounted, computed, watch } from 'vue';
import { useAuthStore } from '@/stores/auth';
import { api } from '@/lib/api';
import { Codemirror } from 'vue-codemirror';
import { oneDark } from '@codemirror/theme-one-dark';
import { StreamLanguage } from '@codemirror/language';
import { toml } from '@codemirror/legacy-modes/mode/toml';
import { parse } from 'smol-toml';

const extensions = shallowRef([StreamLanguage.define(toml), oneDark]);

const auth = useAuthStore();

const value   = ref('');
const saved   = ref('');
const loading = ref(true);
const saving  = ref(false);
const restarting = ref(false);
const status  = ref<{ type: 'success' | 'error'; msg: string } | null>(null);
const validationError = ref<string | null>(null);

onMounted(async () => {
  try {
    value.value = saved.value = await api.configRaw();
  } catch (e) {
    status.value = { type: 'error', msg: String(e) };
  } finally {
    loading.value = false;
  }
});

const isDirty = computed(() => value.value !== saved.value);

watch(value, (newVal) => {
  try {
    parse(newVal);
    validationError.value = null;
  } catch (e: any) {
    validationError.value = e.message || String(e);
  }
});

async function save() {
  if (validationError.value) return;
  saving.value = true;
  status.value = null;
  try {
    await api.saveConfigRaw(value.value);
    saved.value  = value.value;
    status.value = { type: 'success', msg: 'Config saved. Restart Sonium server to apply these changes.' };
  } catch (e) {
    status.value = { type: 'error', msg: String(e) };
  } finally {
    saving.value = false;
  }
}

function reset() {
  value.value  = saved.value;
  status.value = null;
  validationError.value = null;
}

async function restartServer() {
  const ok = window.confirm(
    'Restart Sonium server now? Audio will stop briefly and the web UI may disconnect while the process comes back.',
  );
  if (!ok) return;

  restarting.value = true;
  status.value = null;
  try {
    await api.restartServer();
    status.value = {
      type: 'success',
      msg: 'Restart requested. If Sonium is managed by systemd, launchd, Docker, or another supervisor, it should come back automatically.',
    };
  } catch (e) {
    status.value = { type: 'error', msg: String(e) };
    restarting.value = false;
  }
}

</script>

<template>
  <div class="space-y-5">
    <div class="flex items-center justify-between gap-3">
      <div>
        <h1 class="text-xl font-bold text-white">Config</h1>
        <p class="text-xs text-slate-500 mt-0.5">Editing <code class="text-slate-400">sonium.toml</code> directly</p>
      </div>
      <div class="flex items-center gap-2 shrink-0">
        <button
          @click="reset"
          :disabled="!isDirty || saving"
          class="px-3 py-1.5 rounded-lg border border-slate-700 text-slate-300 hover:bg-slate-800 text-sm
                 disabled:opacity-40 transition-colors"
        >
          Reset
        </button>
        <button
          @click="save"
          :disabled="!isDirty || saving || restarting || !auth.isAdmin || !!validationError"
          class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-sm
                 font-medium disabled:opacity-50 transition-colors"
        >
          <span class="mdi" :class="saving ? 'mdi-loading animate-spin' : 'mdi-content-save'"></span>
          {{ saving ? 'Saving…' : 'Save' }}
        </button>
        <button
          @click="restartServer"
          :disabled="saving || restarting || !auth.isAdmin"
          class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-amber-600/90 hover:bg-amber-500 text-white text-sm
                 font-medium disabled:opacity-50 transition-colors"
        >
          <span class="mdi" :class="restarting ? 'mdi-loading animate-spin' : 'mdi-restart'"></span>
          {{ restarting ? 'Restarting…' : 'Restart server' }}
        </button>
      </div>
    </div>

    <!-- Validation Error Banner -->
    <Transition name="slide">
      <div
        v-if="validationError"
        class="flex items-start gap-2 p-3 rounded-lg text-sm border bg-red-900/20 border-red-800/40 text-red-400"
      >
        <span class="mdi mdi-alert shrink-0 mt-0.5"></span>
        <div class="flex flex-col">
          <span class="font-bold">Invalid TOML</span>
          <span class="font-mono text-xs mt-1">{{ validationError }}</span>
        </div>
      </div>
    </Transition>

    <!-- Status banner -->
    <Transition name="slide">
      <div
        v-if="status"
        class="flex items-start gap-2 p-3 rounded-lg text-sm border"
        :class="status.type === 'success'
          ? 'bg-green-900/20 border-green-800/40 text-green-400'
          : 'bg-red-900/20 border-red-800/40 text-red-400'"
      >
        <span class="mdi shrink-0 mt-0.5"
          :class="status.type === 'success' ? 'mdi-check-circle-outline' : 'mdi-alert-circle-outline'"></span>
        {{ status.msg }}
      </div>
    </Transition>

    <!-- Loading skeleton -->
    <div v-if="loading" class="card p-4 animate-pulse space-y-2">
      <div v-for="i in 8" :key="i" class="h-3.5 bg-slate-800 rounded" :style="{ width: (40 + i * 6) % 85 + '%' }"></div>
    </div>

    <!-- Editor -->
    <div v-else class="rounded-xl overflow-hidden border border-slate-700 bg-[#282c34] shadow-lg">
      <div class="flex items-center justify-between px-4 py-2 border-b border-slate-800 bg-slate-900/60">
        <span class="text-xs text-slate-500 font-mono">sonium.toml</span>
        <span v-if="isDirty" class="text-xs text-amber-400 flex items-center gap-1">
          <span class="mdi mdi-circle-medium"></span> unsaved
        </span>
      </div>
      <Codemirror
        v-model="value"
        :extensions="extensions"
        :autofocus="true"
        :indent-with-tab="false"
        :tab-size="2"
        style="height: auto; min-height: 400px; font-family: monospace; font-size: 14px;"
      />
    </div>

    <p class="text-xs text-slate-600">
      Sonium validates the TOML before saving. Changes are applied on the next restart; use restart server when Sonium is supervised and should come back automatically.
    </p>
  </div>
</template>

<style scoped>
.slide-enter-active, .slide-leave-active { transition: all .2s ease; }
.slide-enter-from, .slide-leave-to { opacity: 0; transform: translateY(-6px); }

/* Make CodeMirror fill the container properly and hide ugly outlines */
:deep(.cm-editor) {
  min-height: 400px;
  outline: none !important;
}
:deep(.cm-scroller) {
  font-family: inherit;
  padding: 1rem 0;
}
</style>
