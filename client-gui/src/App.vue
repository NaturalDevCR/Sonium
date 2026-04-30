<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
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
const localIp = ref('');
const scanning = ref(false);

const healthState = ref<Record<number, { lastSeen: number, status: string, report?: any, logs: string[] }>>({});
const APP_VERSION = 'v0.1.42';

function instanceStatus(instance: InstanceConfig) {
  if (!instance.enabled) return { label: 'Stopped', tone: 'text-slate-500/80', dot: 'bg-slate-600' };
  const state = healthState.value[instance.id];
  if (state?.status === 'ready') return { label: 'Ready', tone: 'text-emerald-500/80', dot: 'bg-emerald-500 animate-pulse' };
  if (state?.status === 'connected') return { label: 'Connected', tone: 'text-emerald-500/80', dot: 'bg-emerald-500 animate-pulse' };
  if (state?.status === 'connecting') return { label: 'Connecting', tone: 'text-amber-400/80', dot: 'bg-amber-400 animate-pulse' };
  if (state?.status === 'error') return { label: 'Error', tone: 'text-red-500/80', dot: 'bg-red-500' };
  if (!state || Date.now() - state.lastSeen < 10000) return { label: 'Starting', tone: 'text-amber-400/80', dot: 'bg-amber-400 animate-pulse' };
  return { label: 'Offline', tone: 'text-red-500/80', dot: 'bg-red-500' };
}

const localSubnet = computed(() => {
  if (!localIp.value) return '';
  return localIp.value.split('.').slice(0, 3).join('.');
});

async function fetchLocalIp() {
  try {
    localIp.value = await invoke('get_local_ip');
  } catch (e) {
    console.error("Failed to fetch local IP", e);
  }
}

async function startScan() {
  scanning.value = true;
  try {
    const found = await invoke<string[]>('scan_subnet');
    if (found.length > 0 && editingInstance.value) {
      editingInstance.value.server_host = found[0];
    }
  } catch (e) {
    console.error("Scan failed", e);
  } finally {
    scanning.value = false;
  }
}

async function loadInstances() {
  instances.value = await invoke('get_instances');
}

async function saveInstances() {
  await invoke('save_instances', { instances: instances.value });
  const now = Date.now();
  for (const instance of instances.value) {
    if (instance.enabled && !healthState.value[instance.id]) {
      healthState.value[instance.id] = {
        lastSeen: now,
        status: 'starting',
        logs: [`${new Date().toLocaleTimeString()} - Starting client instance`],
      };
    }
  }
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
    server_host: localIp.value || '127.0.0.1',
    server_port: 1710,
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
  await fetchLocalIp();
  await fetchAudioDevices();
  await loadInstances();
  await checkAutostart();

  // Listen for health updates from all instances
  await listen('client-health', (event: { payload: { id: number, report: any } }) => {
    const { id, report } = event.payload;
    const currentState = healthState.value[id] || { status: 'ready', logs: [] };
    
    // Detect significant events for the log
    const newLogs = [...currentState.logs];
    if (report.underrun_count > 0 && (!currentState.report || report.underrun_count > currentState.report.underrun_count)) {
      newLogs.unshift(`${new Date().toLocaleTimeString()} - Buffer underrun detected (+${report.underrun_count - (currentState.report?.underrun_count || 0)})`);
    }
    if (report.stale_drop_count > 0 && (!currentState.report || report.stale_drop_count > currentState.report.stale_drop_count)) {
      newLogs.unshift(`${new Date().toLocaleTimeString()} - Stale chunks dropped (+${report.stale_drop_count - (currentState.report?.stale_drop_count || 0)})`);
    }
    
    healthState.value[id] = {
      lastSeen: Date.now(),
      status: currentState.status === 'starting' || currentState.status === 'connecting'
        ? 'ready'
        : currentState.status,
      report,
      logs: newLogs.slice(0, 10) // Keep last 10 logs
    };
  });

  await listen('client-status', (event: { payload: { id: number, status: string, message?: string | null } }) => {
    const { id, status, message } = event.payload;
    const currentState = healthState.value[id] || { logs: [] };
    const newLogs = [...currentState.logs];
    const lastLog = newLogs[0] || '';
    const label = message ? `${status}: ${message}` : status;

    if (!lastLog.endsWith(label)) {
      newLogs.unshift(`${new Date().toLocaleTimeString()} - ${label}`);
    }

    healthState.value[id] = {
      lastSeen: Date.now(),
      status,
      report: currentState.report,
      logs: newLogs.slice(0, 10),
    };
  });

  // Periodically check for stale connections
  setInterval(() => {
    const now = Date.now();
    for (const id in healthState.value) {
      if (now - healthState.value[id].lastSeen > 10000) {
        if (['connected', 'ready'].includes(healthState.value[id].status)) {
          healthState.value[id].logs.unshift(`[${new Date().toLocaleTimeString()}] Connection timed out`);
        }
        healthState.value[id].status = 'disconnected';
      }
    }
  }, 2000);
});
</script>

<template>
  <div class="h-screen flex flex-col bg-slate-900/60 backdrop-blur-xl text-slate-100 overflow-hidden border border-white/10 rounded-xl">
    <!-- Header with drag region -->
    <div 
      data-tauri-drag-region
      class="h-12 flex items-center bg-white/10 border-b border-white/5 shrink-0 relative z-50 select-none"
    >
      <!-- Traffic lights spacer (macOS) - pointer-events-none ensures clicks pass to native controls -->
      <div class="w-20 h-full pointer-events-none"></div>
      
      <!-- Drag Handle Label -->
      <div data-tauri-drag-region class="flex-1 h-full flex items-center justify-center pointer-events-none">
        <div class="flex items-center space-x-2">
          <span class="text-[10px] font-bold tracking-[0.2em] text-slate-400 uppercase">Sonium Agent</span>
          <span class="text-[10px] text-slate-600 font-medium">{{ APP_VERSION }}</span>
        </div>
      </div>
      
      <div data-tauri-drag-region class="w-20 h-full pointer-events-none"></div>
    </div>

    <div class="flex-1 overflow-y-auto p-6 space-y-6 custom-scrollbar">
      <!-- Top Section -->
      <div class="flex justify-between items-start">
        <div>
          <h1 class="text-4xl font-extrabold tracking-tight bg-clip-text text-transparent bg-gradient-to-br from-white via-blue-400 to-indigo-600">
            Sonium
          </h1>
          <p class="text-slate-400 text-xs font-medium mt-1">Multiroom Audio Desktop Client</p>
        </div>
        
        <div class="bg-white/5 p-2 rounded-xl border border-white/5 flex items-center space-x-3">
          <span class="text-[10px] font-bold text-slate-500 uppercase tracking-widest ml-1">Autostart</span>
          <button 
            @click="toggleAutostart"
            class="relative inline-flex h-5 w-9 items-center rounded-full transition-all duration-300 focus:outline-none"
            :class="autostart ? 'bg-indigo-500' : 'bg-slate-700'"
          >
            <span 
              class="inline-block h-3 w-3 transform rounded-full bg-white transition-transform duration-300 shadow-sm"
              :class="autostart ? 'translate-x-5' : 'translate-x-1'"
            />
          </button>
        </div>
      </div>

      <!-- Main Content -->
      <div v-if="!editingInstance" class="space-y-3">
        <div class="flex items-center justify-between mb-2">
          <h2 class="text-xs font-bold text-slate-500 uppercase tracking-[0.2em]">Active Instances</h2>
          <span class="text-[10px] text-slate-600 bg-white/5 px-2 py-0.5 rounded-full">{{ instances.length }} total</span>
        </div>

        <div v-if="instances.length === 0" 
             class="group flex flex-col items-center justify-center py-12 px-6 text-center border-2 border-dashed border-white/5 rounded-2xl hover:border-indigo-500/30 transition-all duration-500 bg-white/[0.02]">
          <div class="mb-6 group-hover:scale-105 transition-transform duration-500">
            <img src="/sonium-logo.png" class="w-20 h-20 mx-auto drop-shadow-2xl" alt="Sonium Logo" />
            <h2 class="text-2xl font-black tracking-[0.3em] uppercase mt-4 bg-clip-text text-transparent bg-gradient-to-r from-white to-slate-500">Sonium</h2>
          </div>
          <p class="text-slate-400 font-medium mb-1">Welcome to Sonium</p>
          <p class="text-slate-500 text-xs mb-8">No instances configured. Add a client to start streaming.</p>
          <button @click="addInstance" class="px-8 py-3 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-black uppercase tracking-widest rounded-xl transition-all shadow-2xl shadow-indigo-500/40 active:scale-95">
            Get Started
          </button>
        </div>

        <div v-for="instance in instances" :key="instance.id" class="space-y-2">
          <div 
               class="group flex items-center justify-between p-4 bg-white/[0.03] hover:bg-white/[0.06] rounded-2xl border border-white/5 hover:border-indigo-500/30 transition-all duration-300">
            <div class="flex items-center space-x-4">
              <button 
                @click="toggleInstance(instance)"
                class="relative inline-flex h-6 w-11 items-center rounded-full transition-all duration-300 focus:outline-none shrink-0"
                :class="instance.enabled ? 'bg-emerald-500 shadow-lg shadow-emerald-500/20' : 'bg-slate-700'"
              >
                <span 
                  class="inline-block h-4 w-4 transform rounded-full bg-white transition-transform duration-300 shadow-sm"
                  :class="instance.enabled ? 'translate-x-6' : 'translate-x-1'"
                />
              </button>
              <div>
                <div class="flex items-center space-x-2">
                  <h3 class="font-bold text-slate-200 group-hover:text-white transition-colors">{{ instance.name }}</h3>
                  <div v-if="instance.enabled" class="flex items-center">
                    <span 
                      class="w-1.5 h-1.5 rounded-full mr-1.5"
                      :class="instanceStatus(instance).dot"
                    ></span>
                    <span class="text-[9px] font-bold uppercase tracking-widest" :class="instanceStatus(instance).tone">
                      {{ instanceStatus(instance).label }}
                    </span>
                  </div>
                </div>
                <div class="flex items-center space-x-2 mt-0.5">
                  <span class="text-[10px] font-mono text-slate-500 bg-white/5 px-1.5 py-0.5 rounded">{{ instance.server_host }}:{{ instance.server_port }}</span>
                  <span class="text-[10px] text-slate-600">•</span>
                  <span class="text-[10px] text-slate-500 truncate max-w-[120px]">{{ instance.device || 'Default Device' }}</span>
                </div>
              </div>
            </div>
            
            <div class="flex items-center space-x-1">
              <button @click="editInstance(instance)" class="p-2.5 text-slate-500 hover:text-white bg-white/0 hover:bg-white/5 rounded-xl transition-all active:scale-90">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
                  <path d="M13.586 3.586a2 2 0 112.828 2.828l-.793.793-2.828-2.828.793-.793zM11.379 5.793L3 14.172V17h2.828l8.38-8.379-2.83-2.828z" />
                </svg>
              </button>
            </div>
          </div>

          <!-- Diagnostics & Logs Panel -->
          <div v-if="instance.enabled"
               class="mx-2 overflow-hidden bg-slate-950/40 rounded-xl border border-white/5 animate-in fade-in slide-in-from-top-2 duration-300">
            <!-- Stats Grid -->
            <div class="p-4 grid grid-cols-3 gap-4 border-b border-white/5">
              <div class="space-y-1">
                <span class="text-[9px] font-black text-slate-500 uppercase tracking-widest block">Buffer Depth</span>
                <div class="flex items-baseline space-x-1">
                  <span class="text-lg font-mono font-bold text-blue-400">{{ healthState[instance.id]?.report?.buffer_depth_ms || 0 }}</span>
                  <span class="text-[9px] font-bold text-slate-600 uppercase">ms</span>
                </div>
              </div>
              <div class="space-y-1">
                <span class="text-[9px] font-black text-slate-500 uppercase tracking-widest block">Jitter / Latency</span>
                <div class="flex items-center space-x-2">
                  <span class="text-xs font-mono font-bold text-slate-300">{{ healthState[instance.id]?.report?.jitter_ms || 0 }}ms</span>
                  <span class="text-[10px] text-slate-600">/</span>
                  <span class="text-xs font-mono font-bold text-slate-300">{{ healthState[instance.id]?.report?.latency_ms || 0 }}ms</span>
                </div>
              </div>
              <div class="space-y-1">
                <span class="text-[9px] font-black text-slate-500 uppercase tracking-widest block">Errors</span>
                <div class="flex items-center space-x-3">
                  <div class="flex items-center space-x-1" :class="(healthState[instance.id]?.report?.underrun_count || 0) > 0 ? 'text-amber-500' : 'text-slate-600'">
                    <span class="text-[10px] font-bold">{{ healthState[instance.id]?.report?.underrun_count || 0 }}</span>
                    <span class="text-[8px] font-black uppercase">Drops</span>
                  </div>
                  <div class="flex items-center space-x-1" :class="(healthState[instance.id]?.report?.stale_drop_count || 0) > 0 ? 'text-red-500' : 'text-slate-600'">
                    <span class="text-[10px] font-bold">{{ healthState[instance.id]?.report?.stale_drop_count || 0 }}</span>
                    <span class="text-[8px] font-black uppercase">Stale</span>
                  </div>
                </div>
              </div>
            </div>
            <!-- Recent Activity Log -->
            <div class="px-4 py-2 bg-black/20">
              <span class="text-[8px] font-black text-slate-600 uppercase tracking-[0.2em] mb-1 block">Activity Log</span>
              <div class="space-y-1 min-h-[40px]">
                <div v-for="(log, idx) in healthState[instance.id]?.logs" :key="idx" 
                     class="text-[10px] font-mono text-slate-400 border-l border-white/10 pl-2 leading-tight">
                  {{ log }}
                </div>
                <div v-if="!healthState[instance.id]?.logs?.length" class="text-[10px] font-mono text-slate-700 italic">
                  Waiting for events...
                </div>
              </div>
            </div>
          </div>
        </div>

        <button v-if="instances.length > 0" @click="addInstance" 
                class="w-full py-4 border-2 border-dashed border-white/5 rounded-2xl text-slate-500 hover:text-white hover:border-indigo-500/40 hover:bg-white/[0.02] transition-all duration-300 font-bold text-xs uppercase tracking-widest flex items-center justify-center space-x-2 active:scale-[0.98]">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" clip-rule="evenodd" />
          </svg>
          <span>Add New Instance</span>
        </button>
      </div>

      <!-- Edit Form -->
      <div v-else class="bg-white/[0.03] p-6 rounded-2xl border border-white/10 space-y-5 animate-in fade-in slide-in-from-bottom-4 duration-500">
        <div class="flex justify-between items-center mb-2">
          <h2 class="text-xl font-black tracking-tight">{{ editingInstance.id ? 'Configure' : 'Create' }} Instance</h2>
          <button v-if="editingInstance.id && instances.find(i => i.id === editingInstance!.id)" 
                  @click="deleteInstance(editingInstance.id); editingInstance = null" 
                  class="text-red-500/70 hover:text-red-400 text-[10px] font-bold uppercase tracking-widest flex items-center space-x-1 px-3 py-1.5 bg-red-500/5 hover:bg-red-500/10 rounded-lg transition-all active:scale-95">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
            </svg>
            <span>Remove</span>
          </button>
        </div>

        <div class="space-y-4">
          <div>
            <label class="block text-[10px] font-black text-slate-500 uppercase tracking-[0.2em] mb-2 ml-1">Instance Name</label>
            <input v-model="editingInstance.name" type="text" placeholder="e.g. Living Room" class="w-full bg-white/5 border border-white/5 rounded-xl p-3 text-sm text-white focus:border-indigo-500 focus:ring-2 focus:ring-indigo-500/20 outline-none transition-all placeholder:text-slate-600" />
          </div>

          <div class="grid grid-cols-3 gap-4">
            <div class="col-span-2">
              <label class="block text-xs font-semibold text-slate-400 uppercase tracking-wider mb-1">Server Host</label>
              <div class="flex gap-2">
                <input 
                  v-model="editingInstance.server_host" 
                  type="text" 
                  class="w-full bg-slate-900/50 border border-slate-700 rounded-lg px-4 py-2.5 text-white focus:outline-none focus:border-indigo-500 transition-colors"
                  placeholder="e.g. 192.168.1.50"
                />
                <button 
                  @click="startScan"
                  :disabled="scanning"
                  class="px-3 bg-indigo-600 hover:bg-indigo-500 disabled:bg-slate-700 text-white rounded-lg transition-all flex items-center justify-center min-w-[80px]"
                >
                  <span v-if="scanning" class="animate-spin mr-1">◌</span>
                  {{ scanning ? '...' : 'Scan' }}
                </button>
              </div>
              <p v-if="localIp" class="text-[10px] text-slate-500 mt-1">
                Your IP: {{ localIp }} • Suggested subnet: {{ localSubnet }}.0/24
              </p>
            </div>
            <div>
              <label class="block text-[10px] font-black text-slate-500 uppercase tracking-[0.2em] mb-2 ml-1">Port</label>
              <input v-model.number="editingInstance.server_port" type="number" class="w-full bg-white/5 border border-white/5 rounded-xl p-3 text-sm text-white focus:border-indigo-500 outline-none transition-all" />
            </div>
          </div>

          <div>
            <label class="block text-[10px] font-black text-slate-500 uppercase tracking-[0.2em] mb-2 ml-1">Audio Device</label>
            <div class="relative">
              <select v-model="editingInstance.device" class="w-full bg-white/5 border border-white/5 rounded-xl p-3 text-sm text-white focus:border-indigo-500 outline-none transition-all appearance-none cursor-pointer">
                <option :value="null" class="bg-slate-900">System Default Output</option>
                <option v-for="dev in audioDevices" :key="dev" :value="dev" class="bg-slate-900">{{ dev }}</option>
              </select>
              <div class="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-slate-500">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
                </svg>
              </div>
            </div>
          </div>

          <div>
            <div class="flex justify-between items-center mb-2 ml-1">
              <label class="block text-[10px] font-black text-slate-500 uppercase tracking-[0.2em]">Buffer Latency</label>
              <span class="text-[10px] font-bold text-indigo-400">{{ editingInstance.latency_ms }}ms</span>
            </div>
            <input v-model.number="editingInstance.latency_ms" type="range" min="0" max="1000" step="10" class="w-full h-1.5 bg-white/5 rounded-lg appearance-none cursor-pointer accent-indigo-500" />
            <p class="text-[9px] text-slate-600 mt-2 italic px-1">Tip: Use 0ms for automatic minimum latency based on device.</p>
          </div>
        </div>

        <div class="flex justify-end space-x-3 pt-6 border-t border-white/5 mt-4">
          <button @click="cancelEdit" class="px-5 py-2.5 rounded-xl text-xs font-bold text-slate-400 hover:text-white hover:bg-white/5 transition-all">Discard</button>
          <button @click="saveEdit" class="px-8 py-2.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-black shadow-xl shadow-indigo-600/30 transition-all active:scale-95">Save Changes</button>
        </div>
      </div>
      
    </div>
    
    <div class="h-8 bg-white/5 flex items-center justify-center px-4 shrink-0">
      <span class="text-[9px] font-bold text-slate-600 uppercase tracking-[0.3em]">Ready to Stream • {{ APP_VERSION }}</span>
    </div>
  </div>
</template>

<style>
.custom-scrollbar::-webkit-scrollbar {
  width: 4px;
}
.custom-scrollbar::-webkit-scrollbar-track {
  background: transparent;
}
.custom-scrollbar::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.1);
  border-radius: 10px;
}
.custom-scrollbar::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.2);
}

@keyframes fade-in { from { opacity: 0; } to { opacity: 1; } }
@keyframes slide-in-from-bottom-4 { from { transform: translateY(1rem); } to { transform: translateY(0); } }
.animate-in { animation: fade-in 0.5s ease-out forwards, slide-in-from-bottom-4 0.5s ease-out forwards; }
</style>
