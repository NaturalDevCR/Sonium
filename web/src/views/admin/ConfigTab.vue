<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { useAuthStore } from '@/stores/auth';
import { api } from '@/lib/api';

const auth = useAuthStore();

const value   = ref('');
const saved   = ref('');
const loading = ref(true);
const saving  = ref(false);
const status  = ref<{ type: 'success' | 'error'; msg: string } | null>(null);

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

async function save() {
  saving.value = true;
  status.value = null;
  try {
    await api.saveConfigRaw(value.value);
    saved.value  = value.value;
    status.value = { type: 'success', msg: 'Config saved. Restart the server for changes to take effect.' };
  } catch (e) {
    status.value = { type: 'error', msg: String(e) };
  } finally {
    saving.value = false;
  }
}

function reset() {
  value.value  = saved.value;
  status.value = null;
}

// Auto-grow textarea height to content
function autoGrow(el: HTMLTextAreaElement) {
  el.style.height = 'auto';
  el.style.height = el.scrollHeight + 'px';
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
          :disabled="!isDirty || saving || !auth.isAdmin"
          class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-sm
                 font-medium disabled:opacity-50 transition-colors"
        >
          <span class="mdi" :class="saving ? 'mdi-loading animate-spin' : 'mdi-content-save'"></span>
          {{ saving ? 'Saving…' : 'Save' }}
        </button>
      </div>
    </div>

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
    <div v-else class="rounded-xl overflow-hidden border border-slate-700 bg-slate-950">
      <div class="flex items-center justify-between px-4 py-2 border-b border-slate-800 bg-slate-900/60">
        <span class="text-xs text-slate-500 font-mono">sonium.toml</span>
        <span v-if="isDirty" class="text-xs text-amber-400 flex items-center gap-1">
          <span class="mdi mdi-circle-medium"></span> unsaved
        </span>
      </div>
      <textarea
        v-model="value"
        spellcheck="false"
        autocomplete="off"
        autocorrect="off"
        @input="autoGrow($event.target as HTMLTextAreaElement)"
        class="w-full bg-transparent text-slate-200 font-mono text-sm px-4 py-4 resize-none outline-none
               leading-relaxed min-h-[400px]"
        style="tab-size: 2;"
      ></textarea>
    </div>

    <p class="text-xs text-slate-600">
      Changes require a server restart to take effect.
      Sonium validates the TOML before saving — invalid config will be rejected.
    </p>
  </div>
</template>

<style scoped>
.slide-enter-active, .slide-leave-active { transition: all .2s ease; }
.slide-enter-from, .slide-leave-to { opacity: 0; transform: translateY(-6px); }
</style>
