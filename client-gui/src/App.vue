<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { enable, isEnabled, disable } from '@tauri-apps/plugin-autostart';

interface InstanceConfig {
  id: number;
  name: string;
  server_host: string;
  server_port: number;
  device: string | null;
  latency_ms: number;
  enabled: boolean;
}

const instances = ref<InstanceConfig[]>([]);
const audioDevices = ref<string[]>([]);
const autostart = ref(false);

const editingInstance = ref<InstanceConfig | null>(null);

async function loadInstances() {
  instances.value = await invoke('get_instances');
}

async function saveInstances() {
  await invoke('save_instances', { instances: instances.value });
  await loadInstances();
}

async function fetchAudioDevices() {
  try {
    audioDevices.value = await invoke('get_audio_devices');
  } catch (e) {
    console.error("Failed to fetch audio devices", e);
  }
}

async function checkAutostart() {
  autostart.value = await isEnabled();
}

async function toggleAutostart() {
  if (autostart.value) {
    await disable();
    autostart.value = false;
  } else {
    await enable();
    autostart.value = true;
  }
}

async function toggleInstance(instance: InstanceConfig) {
  instance.enabled = !instance.enabled;
  await saveInstances();
}

function editInstance(instance: InstanceConfig) {
  editingInstance.value = { ...instance };
}

function addInstance() {
  editingInstance.value = {
    id: Date.now() >>> 0,
    name: `Instance ${instances.value.length + 1}`,
    server_host: '127.0.0.1',
    server_port: 7331,
    device: null,
    latency_ms: 0,
    enabled: true,
  };
}

async function saveEdit() {
  if (!editingInstance.value) return;
  
  const idx = instances.value.findIndex(i => i.id === editingInstance.value!.id);
  if (idx !== -1) {
    instances.value[idx] = editingInstance.value;
  } else {
    instances.value.push(editingInstance.value);
  }
  
  await saveInstances();
  editingInstance.value = null;
}

async function deleteInstance(id: number) {
  instances.value = instances.value.filter(i => i.id !== id);
  await saveInstances();
}

function cancelEdit() {
  editingInstance.value = null;
}

onMounted(async () => {
  await fetchAudioDevices();
  await loadInstances();
  await checkAutostart();
});
</script>

<template>
  <div class="min-h-screen bg-slate-900 text-slate-100 p-6 flex flex-col items-center">
    <div class="w-full max-w-2xl bg-slate-800 rounded-xl shadow-2xl p-6 border border-slate-700">
      
      <div class="flex justify-between items-center mb-6">
        <div>
          <h1 class="text-3xl font-bold bg-clip-text text-transparent bg-gradient-to-r from-blue-400 to-indigo-500">Sonium</h1>
          <p class="text-slate-400 text-sm">Desktop Agent</p>
        </div>
        <div class="flex items-center space-x-2">
          <label class="text-sm text-slate-300">Run on Startup</label>
          <button 
            @click="toggleAutostart"
            class="relative inline-flex h-6 w-11 items-center rounded-full transition-colors focus:outline-none"
            :class="autostart ? 'bg-indigo-500' : 'bg-slate-600'"
          >
            <span 
              class="inline-block h-4 w-4 transform rounded-full bg-white transition-transform"
              :class="autostart ? 'translate-x-6' : 'translate-x-1'"
            />
          </button>
        </div>
      </div>

      <div v-if="!editingInstance" class="space-y-4">
        <div v-if="instances.length === 0" class="text-center py-10 text-slate-500 border-2 border-dashed border-slate-700 rounded-lg">
          No instances configured. Add one to get started!
        </div>

        <div v-for="instance in instances" :key="instance.id" 
             class="flex items-center justify-between p-4 bg-slate-700/50 rounded-lg border border-slate-600 hover:border-indigo-500/50 transition-colors">
          <div class="flex items-center space-x-4">
            <button 
              @click="toggleInstance(instance)"
              class="relative inline-flex h-6 w-11 items-center rounded-full transition-colors focus:outline-none shrink-0"
              :class="instance.enabled ? 'bg-emerald-500' : 'bg-slate-500'"
            >
              <span 
                class="inline-block h-4 w-4 transform rounded-full bg-white transition-transform"
                :class="instance.enabled ? 'translate-x-6' : 'translate-x-1'"
              />
            </button>
            <div>
              <h3 class="font-semibold text-lg">{{ instance.name }}</h3>
              <p class="text-xs text-slate-400 font-mono">{{ instance.server_host }}:{{ instance.server_port }}</p>
            </div>
          </div>
          <div class="flex items-center space-x-2">
            <span class="text-xs px-2 py-1 bg-slate-800 rounded text-slate-300 truncate max-w-[150px]" :title="instance.device || 'Default'">
              {{ instance.device || 'Default Device' }}
            </span>
            <button @click="editInstance(instance)" class="p-2 text-slate-400 hover:text-white transition-colors">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
                <path d="M13.586 3.586a2 2 0 112.828 2.828l-.793.793-2.828-2.828.793-.793zM11.379 5.793L3 14.172V17h2.828l8.38-8.379-2.83-2.828z" />
              </svg>
            </button>
          </div>
        </div>

        <button @click="addInstance" 
                class="w-full py-3 mt-4 border-2 border-dashed border-slate-600 rounded-lg text-slate-400 hover:text-white hover:border-indigo-500 transition-colors font-medium flex items-center justify-center space-x-2">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" clip-rule="evenodd" />
          </svg>
          <span>Add Instance</span>
        </button>
      </div>

      <div v-else class="bg-slate-700/30 p-5 rounded-lg border border-slate-600 space-y-4">
        <div class="flex justify-between items-center mb-4">
          <h2 class="text-xl font-bold">{{ editingInstance.id ? 'Edit' : 'Add' }} Instance</h2>
          <button v-if="editingInstance.id && instances.find(i => i.id === editingInstance!.id)" 
                  @click="deleteInstance(editingInstance.id); editingInstance = null" 
                  class="text-red-400 hover:text-red-300 text-sm flex items-center space-x-1">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
            </svg>
            <span>Delete</span>
          </button>
        </div>

        <div>
          <label class="block text-xs text-slate-400 uppercase tracking-wider mb-1">Name</label>
          <input v-model="editingInstance.name" type="text" class="w-full bg-slate-800 border border-slate-600 rounded p-2 text-white focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500 outline-none" />
        </div>

        <div class="grid grid-cols-3 gap-4">
          <div class="col-span-2">
            <label class="block text-xs text-slate-400 uppercase tracking-wider mb-1">Server Host</label>
            <input v-model="editingInstance.server_host" type="text" class="w-full bg-slate-800 border border-slate-600 rounded p-2 text-white focus:border-indigo-500 outline-none" placeholder="127.0.0.1" />
          </div>
          <div>
            <label class="block text-xs text-slate-400 uppercase tracking-wider mb-1">Port</label>
            <input v-model.number="editingInstance.server_port" type="number" class="w-full bg-slate-800 border border-slate-600 rounded p-2 text-white focus:border-indigo-500 outline-none" />
          </div>
        </div>

        <div>
          <label class="block text-xs text-slate-400 uppercase tracking-wider mb-1">Audio Device</label>
          <select v-model="editingInstance.device" class="w-full bg-slate-800 border border-slate-600 rounded p-2 text-white focus:border-indigo-500 outline-none">
            <option :value="null">System Default</option>
            <option v-for="dev in audioDevices" :key="dev" :value="dev">{{ dev }}</option>
          </select>
        </div>

        <div>
          <label class="block text-xs text-slate-400 uppercase tracking-wider mb-1">Buffer Latency (ms)</label>
          <input v-model.number="editingInstance.latency_ms" type="number" class="w-full bg-slate-800 border border-slate-600 rounded p-2 text-white focus:border-indigo-500 outline-none" />
          <p class="text-xs text-slate-500 mt-1">Set to 0 for automatic minimum latency</p>
        </div>

        <div class="flex justify-end space-x-3 pt-4 border-t border-slate-600 mt-6">
          <button @click="cancelEdit" class="px-4 py-2 rounded text-slate-300 hover:bg-slate-700 transition-colors">Cancel</button>
          <button @click="saveEdit" class="px-6 py-2 rounded bg-indigo-600 hover:bg-indigo-500 text-white font-medium shadow-lg shadow-indigo-500/30 transition-all">Save</button>
        </div>
      </div>
      
    </div>
  </div>
</template>

<style>
/* Remove the existing global styles since we use Tailwind */
</style>