<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue';
import { useServerStore } from '@/stores/server';
import { useAuthStore }   from '@/stores/auth';
import { api }            from '@/lib/api';
import StreamBadge        from '@/components/StreamBadge.vue';
import EqControl          from '@/components/EqControl.vue';
import { parse, stringify } from 'smol-toml';

const store = useServerStore();
const auth  = useAuthStore();

onMounted(() => store.loadAll());

// ── Param system ──────────────────────────────────────────────────────────

interface Param {
  key:          string;
  label:        string;
  description?: string;
  type:         'text' | 'number' | 'boolean' | 'select';
  default:      string;
  required?:    boolean;
  options?:     { value: string; label: string }[];
  min?:         number;
  max?:         number;
  placeholder?: string;
  mono?:        boolean;
  hint?:        string;
}

interface SourceType {
  id:     string;
  label:  string;
  icon:   string;
  group:  string;
  hint:   string;
  params: Param[];
}

const sourceTypes: SourceType[] = [
  // ── Direct ──────────────────────────────────────────────────────────────
  {
    id: 'stdin', label: 'Standard input', icon: 'mdi-console-line', group: 'Direct',
    hint: 'Pipe raw PCM 16-bit/48 kHz/stereo from any process via stdin.',
    params: [],
  },
  {
    id: 'fifo', label: 'Named pipe (FIFO)', icon: 'mdi-pipe', group: 'Direct',
    hint: 'Any app that writes raw PCM to a named pipe on disk.',
    params: [
      { key: 'path', label: 'FIFO path', type: 'text', default: '/tmp/sonium.fifo',
        required: true, mono: true, placeholder: '/tmp/sonium.fifo' },
    ],
  },
  // ── ffmpeg ───────────────────────────────────────────────────────────────
  {
    id: 'ffmpeg-file', label: 'File / Playlist', icon: 'mdi-file-music', group: 'ffmpeg',
    hint: 'Play an audio file or playlist via ffmpeg (MP3, FLAC, AAC, M4A…).',
    params: [
      { key: 'path', label: 'File path', type: 'text', default: '', required: true,
        mono: true, placeholder: '/music/album.mp3' },
      { key: 'realtime', label: 'Real-time rate', description: 'Read at playback speed (-re flag)',
        type: 'boolean', default: 'true' },
      { key: 'loop', label: 'Loop', description: 'Restart file when it ends (-stream_loop -1)',
        type: 'boolean', default: 'false' },
    ],
  },
  {
    id: 'ffmpeg-radio', label: 'Internet radio / HTTP', icon: 'mdi-radio-tower', group: 'ffmpeg',
    hint: 'Stream any HTTP/HTTPS audio URL — Icecast, SHOUTcast, HLS, etc.',
    params: [
      { key: 'url', label: 'Stream URL', type: 'text', default: '', required: true,
        placeholder: 'http://stream.example.com/radio' },
      { key: 'reconnect', label: 'Auto-reconnect', description: 'Re-connect if the stream drops',
        type: 'boolean', default: 'true' },
      { key: 'format', label: 'Format hint', description: 'Force input format (auto = let ffmpeg decide)',
        type: 'select', default: 'auto',
        options: [
          { value: 'auto',  label: 'Auto-detect' },
          { value: 'mp3',   label: 'MP3' },
          { value: 'aac',   label: 'AAC' },
          { value: 'ogg',   label: 'Ogg / Vorbis' },
          { value: 'flac',  label: 'FLAC' },
        ],
      },
    ],
  },
  {
    id: 'ffmpeg-mac', label: 'System audio (macOS)', icon: 'mdi-apple', group: 'ffmpeg',
    hint: 'Capture Mac system audio via a virtual audio device like BlackHole.',
    params: [
      { key: 'device', label: 'Virtual audio device', type: 'text',
        default: 'BlackHole 2ch', mono: true,
        hint: 'Install: brew install blackhole-2ch' },
    ],
  },
  {
    id: 'ffmpeg-alsa', label: 'ALSA capture (Linux)', icon: 'mdi-linux', group: 'ffmpeg',
    hint: 'Capture from an ALSA device — sound card, loopback, USB audio.',
    params: [
      { key: 'device', label: 'ALSA device', type: 'text', default: 'default', mono: true,
        hint: 'List devices: arecord -l' },
    ],
  },
  {
    id: 'ffmpeg-pulse', label: 'PulseAudio / PipeWire', icon: 'mdi-sine-wave', group: 'ffmpeg',
    hint: 'Capture from a PulseAudio or PipeWire monitor source.',
    params: [
      { key: 'source', label: 'Monitor source name', type: 'text', default: 'default', mono: true,
        hint: 'List sources: pactl list short sources' },
    ],
  },
  {
    id: 'ffmpeg-tcp', label: 'TCP stream (ffmpeg)', icon: 'mdi-lan-connect', group: 'ffmpeg',
    hint: 'ffmpeg listens for a TCP PCM sender and decodes it.',
    params: [
      { key: 'host', label: 'Listen address', type: 'text', default: '0.0.0.0', mono: true },
      { key: 'port', label: 'Port', type: 'number', default: '4953',
        min: 1, max: 65535, mono: true },
    ],
  },
  // ── TCP ──────────────────────────────────────────────────────────────────
  {
    id: 'tcp-listen', label: 'TCP input (server)', icon: 'mdi-access-point', group: 'TCP',
    hint: 'Sonium opens a port and waits for a sender to connect and push PCM.',
    params: [
      { key: 'bind', label: 'Bind address', type: 'text', default: '0.0.0.0', mono: true },
      { key: 'port', label: 'Port', type: 'number', default: '4953',
        min: 1, max: 65535, mono: true },
    ],
  },
  {
    id: 'tcp-connect', label: 'TCP input (client)', icon: 'mdi-lan-pending', group: 'TCP',
    hint: 'Sonium connects outbound to an app that is serving raw PCM.',
    params: [
      { key: 'host', label: 'Remote host / IP', type: 'text', default: '', required: true,
        mono: true, placeholder: '192.168.1.100' },
      { key: 'port', label: 'Port', type: 'number', default: '4953',
        min: 1, max: 65535, mono: true },
    ],
  },
  // ── Virtual / Integrations ───────────────────────────────────────────────
  {
    id: 'airplay', label: 'AirPlay (shairport-sync)', icon: 'mdi-airplay', group: 'Virtual',
    hint: 'Appear as an AirPlay receiver — stream from any Apple device.',
    params: [
      { key: 'binary', label: 'shairport-sync path', type: 'text',
        default: '/usr/bin/shairport-sync', mono: true },
      { key: 'name', label: 'AirPlay name', type: 'text', default: 'Sonium',
        description: 'Device name shown on AirPlay sender devices' },
    ],
  },
  {
    id: 'spotify', label: 'Spotify Connect (librespot)', icon: 'mdi-spotify', group: 'Virtual',
    hint: 'Appear as a Spotify Connect speaker using librespot.',
    params: [
      { key: 'binary', label: 'librespot path', type: 'text',
        default: '/usr/bin/librespot', mono: true },
      { key: 'name', label: 'Device name', type: 'text', default: 'Sonium',
        description: 'Name shown in the Spotify app' },
      { key: 'bitrate', label: 'Bitrate', type: 'select', default: '320',
        options: [
          { value: '96',  label: '96 kbps' },
          { value: '160', label: '160 kbps' },
          { value: '320', label: '320 kbps (highest)' },
        ],
      },
    ],
  },
  {
    id: 'mpd', label: 'MPD (Music Player Daemon)', icon: 'mdi-music-circle', group: 'Virtual',
    hint: 'Pipe MPD output via a FIFO. Configure MPD to write to it first.',
    params: [
      { key: 'path', label: 'MPD FIFO path', type: 'text', default: '/tmp/mpd.fifo', mono: true,
        hint: 'mpd.conf: audio_output { type "fifo"  path "/tmp/mpd.fifo"  format "48000:16:2" }' },
    ],
  },
  {
    id: 'process', label: 'Custom process', icon: 'mdi-console', group: 'Virtual',
    hint: 'Launch any process that outputs raw PCM 48 kHz/16-bit/stereo to stdout.',
    params: [
      { key: 'binary', label: 'Binary path', type: 'text', default: '', required: true,
        mono: true, placeholder: '/usr/bin/my-app' },
      { key: 'args', label: 'Arguments', type: 'text', default: '',
        description: 'Space-separated arguments', mono: true },
    ],
  },
  // ── Meta ─────────────────────────────────────────────────────────────────
  {
    id: 'meta', label: 'Meta (priority chain)', icon: 'mdi-layers-triple', group: 'Meta',
    hint: 'Combine multiple streams with automatic priority-based failover. The first active stream in the chain plays; others are standby.',
    params: [],
  },
];

const groups = ['Direct', 'ffmpeg', 'TCP', 'Virtual', 'Meta'];
const groupLabel: Record<string, string> = { ffmpeg: 'Via ffmpeg', Meta: 'Virtual / Combined' };

// ── Source URI builder ────────────────────────────────────────────────────

function fieldVal(f: Record<string, string>, key: string, fallback = '') {
  return f[key] ?? fallback;
}

function buildSource(typeId: string, f: Record<string, string>): string {
  const v = (k: string, fb = '') => fieldVal(f, k, fb);

  switch (typeId) {
    case 'stdin':  return '-';
    case 'fifo':   return v('path', '/tmp/sonium.fifo');
    case 'mpd':    return v('path', '/tmp/mpd.fifo');

    case 'ffmpeg-file': {
      const args: string[] = [];
      if (v('realtime', 'true') === 'true') args.push('-re');
      if (v('loop', 'false') === 'true') args.push('-stream_loop', '-1');
      args.push('-i', v('path'), '-f', 's16le', '-ar', '48000', '-ac', '2', '-');
      return `pipe:///usr/bin/ffmpeg?${args.join('&')}`;
    }

    case 'ffmpeg-radio': {
      const args: string[] = [];
      if (v('reconnect', 'true') === 'true')
        args.push('-reconnect', '1', '-reconnect_streamed', '1');
      const fmt = v('format', 'auto');
      if (fmt !== 'auto') args.push('-f', fmt);
      args.push('-i', v('url'), '-f', 's16le', '-ar', '48000', '-ac', '2', '-');
      return `pipe:///usr/bin/ffmpeg?${args.join('&')}`;
    }

    case 'ffmpeg-mac':
      return `pipe:///usr/bin/ffmpeg?-f&avfoundation&-i&:${v('device', 'BlackHole 2ch')}&-f&s16le&-ar&48000&-ac&2&-`;

    case 'ffmpeg-alsa':
      return `pipe:///usr/bin/ffmpeg?-f&alsa&-i&${v('device', 'default')}&-f&s16le&-ar&48000&-ac&2&-`;

    case 'ffmpeg-pulse':
      return `pipe:///usr/bin/ffmpeg?-f&pulse&-i&${v('source', 'default')}&-f&s16le&-ar&48000&-ac&2&-`;

    case 'ffmpeg-tcp':
      return `pipe:///usr/bin/ffmpeg?-f&s16le&-ar&48000&-ac&2&-i&tcp://${v('host','0.0.0.0')}:${v('port','4953')}?listen&-f&s16le&-ar&48000&-ac&2&-`;

    case 'tcp-listen':
      return `tcp-listen://${v('bind', '0.0.0.0')}:${v('port', '4953')}`;

    case 'tcp-connect':
      return `tcp://${v('host')}:${v('port', '4953')}`;

    case 'airplay': {
      const args = ['-o', 'stdout', '--', '-d'];
      const name = v('name', 'Sonium');
      if (name) args.push('--name', name);
      return `pipe://${v('binary', '/usr/bin/shairport-sync')}?${args.join('&')}`;
    }

    case 'spotify': {
      const args = ['--backend', 'pipe', '--name', v('name', 'Sonium'), '--bitrate', v('bitrate', '320')];
      return `pipe://${v('binary', '/usr/bin/librespot')}?${args.join('&')}`;
    }

    case 'process': {
      const args = v('args').trim().split(/\s+/).filter(Boolean).join('&');
      return `pipe://${v('binary')}${args ? '?' + args : ''}`;
    }

    default: return '';
  }
}

function isSourceValid(typeId: string, f: Record<string, string>): boolean {
  switch (typeId) {
    case 'ffmpeg-file':  return !!f.path?.trim();
    case 'ffmpeg-radio': return !!f.url?.trim();
    case 'ffmpeg-tcp':   return !!f.host?.trim() && !!f.port;
    case 'tcp-connect':  return !!f.host?.trim() && !!f.port;
    case 'tcp-listen':   return !!f.port;
    case 'process':      return !!f.binary?.trim();
    case 'meta':         return metaSources.value.length >= 1;
    default: return true;
  }
}

// ── Meta stream state ─────────────────────────────────────────────────────

const metaSources = ref<string[]>([]);

const availableForMeta = computed(() =>
  store.streams.filter(s => !metaSources.value.includes(s.id))
);

function streamLabel(id: string) {
  const s = store.streams.find(s => s.id === id);
  return s ? (s.display_name || s.id) : id;
}

function addToMeta(id: string) {
  if (!metaSources.value.includes(id)) metaSources.value.push(id);
}
function removeFromMeta(idx: number) { metaSources.value.splice(idx, 1); }
function moveMetaUp(idx: number) {
  if (idx > 0) {
    const a = metaSources.value[idx - 1];
    metaSources.value[idx - 1] = metaSources.value[idx];
    metaSources.value[idx] = a;
  }
}
function moveMetaDown(idx: number) {
  if (idx < metaSources.value.length - 1) {
    const a = metaSources.value[idx + 1];
    metaSources.value[idx + 1] = metaSources.value[idx];
    metaSources.value[idx] = a;
  }
}

// ── Add stream form state ─────────────────────────────────────────────────

const showAdd    = ref(false);
const addType    = ref(sourceTypes[0]);
const addFields  = ref<Record<string, string>>(initFields(sourceTypes[0]));
const addId      = ref('');
const addName    = ref('');
const addCodec   = ref('opus');
const addBufMs   = ref(1000);
const addIdleEnabled = ref(false);
const addIdleMs      = ref(3000);
const addSilence     = ref(false);
const showToml   = ref(true);
const addInfo    = ref('');
const saving     = ref(false);
const isEditMode = ref(false);
const editIdOriginal = ref('');
const editingEqStreamId = ref<string | null>(null);

// Raw URI — stays in sync with computed, can be manually overridden.
const rawUri         = ref('');
const uriOverridden  = ref(false);

function initFields(t: SourceType): Record<string, string> {
  return Object.fromEntries(t.params.map(p => [p.key, p.default]));
}

const computedSource = computed(() => {
  if (addType.value.id === 'meta') {
    return metaSources.value.length
      ? `meta://${metaSources.value.join('/')}`
      : '';
  }
  return buildSource(addType.value.id, addFields.value);
});

// Keep rawUri in sync unless user has overridden it.
watch(computedSource, v => {
  if (!uriOverridden.value) rawUri.value = v;
}, { immediate: true });

function onRawUriEdit() { uriOverridden.value = true; }
function resetRawUri()  { uriOverridden.value = false; rawUri.value = computedSource.value; }

const effectiveSource = computed(() =>
  uriOverridden.value ? rawUri.value : computedSource.value
);

const canSave = computed(() =>
  !!addId.value.trim()
  && isSourceValid(addType.value.id, addFields.value)
  && !!effectiveSource.value
  && !saving.value
);

function esc(v: string) {
  return v.replaceAll('\\', '\\\\').replaceAll('"', '\\"');
}

const tomlSnippet = computed(() => {
  const id = addId.value.trim();
  if (!id) return '# Fill in Stream ID to preview';
  const lines = ['[[streams]]', `id        = "${esc(id)}"`];
  if (addName.value.trim()) lines.push(`display_name = "${esc(addName.value.trim())}"`);
  lines.push(`source    = "${esc(effectiveSource.value)}"`);
  lines.push(`codec     = "${esc(addCodec.value)}"`);
  lines.push(`buffer_ms = ${addBufMs.value}`);
  if (addIdleEnabled.value) {
    lines.push(`idle_timeout_ms = ${addIdleMs.value}`);
    if (addSilence.value) lines.push(`silence_on_idle = true`);
  }
  return lines.join('\n');
});

function selectType(t: SourceType) {
  addType.value   = t;
  addFields.value = initFields(t);
  addInfo.value   = '';
  if (t.id !== 'meta') metaSources.value = [];
  uriOverridden.value = false;
}

function closeDialog() {
  showAdd.value = false;
  addId.value   = '';
  addName.value = '';
  addInfo.value = '';
  addIdleEnabled.value = false;
  addSilence.value = false;
  metaSources.value = [];
  uriOverridden.value = false;
  isEditMode.value = false;
  editIdOriginal.value = '';
  selectType(sourceTypes[0]);
}

function editStream(s: any) {
  isEditMode.value = true;
  editIdOriginal.value = s.id;
  addId.value = s.id;
  addName.value = s.display_name || '';
  addCodec.value = s.codec || 'opus';
  addBufMs.value = s.buffer_ms || 1000;
  addIdleEnabled.value = !!s.idle_timeout_ms;
  addIdleMs.value = s.idle_timeout_ms || 3000;
  addSilence.value = !!s.silence_on_idle;
  
  // Meta handling
  if (s.source?.startsWith('meta://')) {
    const list = s.source.replace('meta://', '').split(',');
    metaSources.value = list.filter((x: string) => x);
    selectType(sourceTypes.find(t => t.id === 'meta')!);
    uriOverridden.value = false;
  } else {
    // Try to find matching source type for others if they are simple
    // For now, if it's not meta, we use raw URI mode to be safe
    rawUri.value = s.source || '';
    uriOverridden.value = true;
  }
  
  showAdd.value = true;
}

async function submitAdd() {
  if (!canSave.value) return;
  saving.value  = true;
  addInfo.value = '';
  try {
    const raw = await api.configRaw().catch(() => '');
    let config: any = parse(raw);
    
    if (!config.streams) config.streams = [];
    
    const streamData: any = {
      id: addId.value.trim(),
      source: effectiveSource.value,
      codec: addCodec.value,
      buffer_ms: addBufMs.value,
    };
    if (addName.value.trim()) streamData.display_name = addName.value.trim();
    if (addIdleEnabled.value) {
      streamData.idle_timeout_ms = addIdleMs.value;
      if (addSilence.value) streamData.silence_on_idle = true;
    }

    if (isEditMode.value) {
      const idx = config.streams.findIndex((st: any) => st.id === editIdOriginal.value);
      if (idx !== -1) {
        config.streams[idx] = streamData;
      } else {
        config.streams.push(streamData);
      }
    } else {
      config.streams.push(streamData);
    }

    await api.saveConfigRaw(stringify(config));
    addInfo.value = isEditMode.value ? '✓ Stream updated successfully!' : '✓ Stream added successfully!';
    
    // Refresh the store and close after a delay
    setTimeout(() => {
      store.loadAll();
      closeDialog();
    }, 1200);
  } catch (e) {
    addInfo.value = `Could not save: ${String(e)}`;
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="space-y-6">

    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="page-title">Streams</h1>
        <p class="page-sub">Audio sources feeding your groups</p>
      </div>
      <button v-if="auth.isAdmin" @click="showAdd = true" class="btn-primary">
        <span class="mdi mdi-plus"></span>
        Add stream
      </button>
    </div>

    <!-- Stream list -->
    <div class="card">
      <div v-if="store.streams.length === 0"
           class="px-5 py-12 text-center" style="color:var(--text-muted);">
        <span class="mdi mdi-music-off text-4xl block mb-3"></span>
        No streams configured — add one or edit sonium.toml directly.
      </div>
      <div v-for="(s, i) in store.streams" :key="s.id"
           class="stream-row" :class="{ 'border-t': i > 0 }">
        <div class="flex items-center gap-3 min-w-0">
          <div class="stream-icon">
            <span class="mdi mdi-music-note text-sm" style="color:var(--accent);"></span>
          </div>
          <div class="min-w-0">
            <p class="font-semibold truncate" style="font-size:13.5px;color:var(--text-primary);">
              {{ s.display_name || s.id }}
            </p>
            <p v-if="s.display_name" class="truncate"
               style="font-size:11px;color:var(--text-muted);font-family:var(--font-mono);">
              {{ s.id }}
            </p>
            <p class="truncate" style="font-size:11px;color:var(--text-muted);">
              {{ s.codec.toUpperCase() }} · {{ s.format }}
            </p>
          </div>
        </div>
        <div class="flex items-center gap-4">
          <StreamBadge :status="s.status" :codec="s.codec" />
          <button v-if="auth.isAdmin" @click="editingEqStreamId = s.id" 
                  class="p-2 rounded-lg text-slate-500 hover:text-white hover:bg-white/5 transition-colors"
                  title="Equalizer">
            <span class="mdi mdi-tune"></span>
          </button>
          <button v-if="auth.isAdmin" @click="editStream(s)" 
                  class="p-2 rounded-lg text-slate-500 hover:text-white hover:bg-white/5 transition-colors"
                  title="Edit stream">
            <span class="mdi mdi-pencil-outline"></span>
          </button>
        </div>
      </div>
    </div>

    <!-- ── Add stream dialog ─────────────────────────────────────────────── -->
    <Teleport to="body">
      <div v-if="showAdd && auth.isAdmin"
           class="dialog-overlay" @click.self="closeDialog">
        <div class="dialog-panel anim-scale-in">

          <!-- Dialog header -->
          <div class="dialog-header">
            <div>
              <h2 class="dialog-title">{{ isEditMode ? 'Edit' : 'Add' }} stream</h2>
              <p class="dialog-sub">{{ isEditMode ? 'Modify stream configuration' : 'Choose a source type and configure it' }}</p>
            </div>
            <button @click="closeDialog" class="dialog-close">
              <span class="mdi mdi-close text-lg"></span>
            </button>
          </div>

          <!-- Dialog body: left type panel + right config panel -->
          <div class="dialog-body">
            <div class="dialog-grid">

              <!-- ── Left: type selector ─────────────────────────────────── -->
              <div class="type-panel">
                <div v-for="g in groups" :key="g" class="mb-4">
                  <p class="group-label">{{ groupLabel[g] || g }}</p>
                  <div class="grid grid-cols-2 gap-1">
                    <button
                      v-for="t in sourceTypes.filter(s => s.group === g)" :key="t.id"
                      @click="selectType(t)"
                      class="type-btn"
                      :class="{ 'type-btn-active': addType.id === t.id }"
                    >
                      <span class="mdi shrink-0" :class="t.icon" style="font-size:14px;"></span>
                      <span class="truncate text-left" style="font-size:11px;">{{ t.label }}</span>
                    </button>
                  </div>
                </div>

                <!-- Type hint -->
                <div class="type-hint-box">
                  <p style="font-size:11.5px;font-weight:600;color:var(--accent);margin-bottom:4px;">
                    {{ addType.label }}
                  </p>
                  <p style="font-size:11.5px;color:var(--text-secondary);line-height:1.6;">
                    {{ addType.hint }}
                  </p>
                </div>
              </div>

              <!-- ── Right: config ────────────────────────────────────────── -->
              <div class="config-panel">

                <!-- Identity -->
                <section class="config-section">
                  <p class="section-heading">Identity</p>
                  <div class="grid grid-cols-2 gap-3">
                    <div>
                      <label class="param-label block mb-1.5">Stream ID <span class="req">*</span></label>
                      <input v-model="addId" type="text" placeholder="living-room"
                             class="field field-mono" />
                    </div>
                    <div>
                      <label class="param-label block mb-1.5">Display name</label>
                      <input v-model="addName" type="text" placeholder="Living Room" class="field" />
                    </div>
                  </div>
                </section>

                <!-- Meta dual-pane OR per-type params -->
                <section class="config-section" v-if="addType.id === 'meta'">
                  <p class="section-heading">Priority chain</p>
                  <p style="font-size:11.5px;color:var(--text-muted);margin-bottom:10px;">
                    Click a stream to add it. First in the list = highest priority. Falls back to the next when the active one goes idle.
                  </p>
                  <div class="meta-grid">
                    <!-- Available -->
                    <div>
                      <p class="param-label mb-2">Available streams</p>
                      <div class="meta-list">
                        <div v-if="availableForMeta.length === 0"
                             style="padding:12px;text-align:center;font-size:11.5px;color:var(--text-muted);">
                          All streams added
                        </div>
                        <button
                          v-for="s in availableForMeta" :key="s.id"
                          @click="addToMeta(s.id)"
                          class="meta-stream-btn"
                        >
                          <StreamBadge :status="s.status" :codec="s.codec" />
                          <span class="flex-1 text-left truncate" style="font-size:12px;">
                            {{ s.display_name || s.id }}
                          </span>
                          <span class="mdi mdi-plus" style="color:var(--accent);font-size:14px;"></span>
                        </button>
                      </div>
                    </div>

                    <!-- Chain -->
                    <div>
                      <p class="param-label mb-2">Priority chain</p>
                      <div class="meta-list">
                        <div v-if="metaSources.length === 0"
                             style="padding:12px;text-align:center;font-size:11.5px;color:var(--text-muted);">
                          ← Click a stream to add
                        </div>
                        <div v-for="(sid, idx) in metaSources" :key="sid"
                             class="meta-chain-item">
                          <span class="meta-priority">{{ idx + 1 }}</span>
                          <span class="flex-1 truncate" style="font-size:12px;">
                            {{ streamLabel(sid) }}
                          </span>
                          <div class="flex gap-0.5">
                            <button @click="moveMetaUp(idx)" :disabled="idx === 0"
                                    class="chain-btn" title="Move up">
                              <span class="mdi mdi-chevron-up"></span>
                            </button>
                            <button @click="moveMetaDown(idx)"
                                    :disabled="idx === metaSources.length - 1"
                                    class="chain-btn" title="Move down">
                              <span class="mdi mdi-chevron-down"></span>
                            </button>
                            <button @click="removeFromMeta(idx)"
                                    class="chain-btn chain-btn-remove" title="Remove">
                              <span class="mdi mdi-close"></span>
                            </button>
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                </section>

                <section class="config-section" v-else-if="addType.params.length > 0">
                  <p class="section-heading">Source settings</p>
                  <div class="space-y-3">
                    <div v-for="p in addType.params" :key="p.key" class="param-row">

                      <!-- Boolean params: full-width toggle row -->
                      <template v-if="p.type === 'boolean'">
                        <label class="boolean-row">
                          <span class="toggle-wrap">
                            <input
                              v-model="addFields[p.key]"
                              true-value="true" false-value="false"
                              type="checkbox" class="sr-only"
                            />
                            <span class="toggle" :class="addFields[p.key] === 'true' ? 'toggle-on' : ''"></span>
                          </span>
                          <span>
                            <span class="param-label">{{ p.label }}</span>
                            <span v-if="p.description" class="param-desc block">{{ p.description }}</span>
                          </span>
                        </label>
                      </template>

                      <!-- Text / Number / Select: two-col row -->
                      <template v-else>
                        <div class="param-two-col">
                          <div>
                            <span class="param-label">
                              {{ p.label }}
                              <span v-if="p.required" class="req">*</span>
                            </span>
                            <span v-if="p.description" class="param-desc block">{{ p.description }}</span>
                          </div>
                          <div>
                            <input
                              v-if="p.type === 'text'"
                              v-model="addFields[p.key]"
                              type="text"
                              :placeholder="p.placeholder ?? String(p.default)"
                              class="field"
                              :class="{ 'field-mono': p.mono }"
                            />
                            <input
                              v-else-if="p.type === 'number'"
                              v-model="addFields[p.key]"
                              type="number"
                              :min="p.min"
                              :max="p.max"
                              class="field field-mono"
                            />
                            <select
                              v-else-if="p.type === 'select'"
                              v-model="addFields[p.key]"
                              class="field"
                            >
                              <option v-for="opt in p.options" :key="opt.value" :value="opt.value">
                                {{ opt.label }}
                              </option>
                            </select>
                          </div>
                        </div>
                        <p v-if="p.hint"
                           style="font-size:10.5px;color:var(--text-muted);font-family:var(--font-mono);margin-top:4px;">
                          {{ p.hint }}
                        </p>
                      </template>
                    </div>
                  </div>
                </section>

                <section v-else-if="addType.id !== 'meta'" class="config-section">
                  <div class="no-params-note">
                    <span class="mdi mdi-check-circle-outline mr-1.5" style="color:var(--green);"></span>
                    No source configuration needed — source is fixed for this type.
                  </div>
                </section>

                <!-- Stream settings -->
                <section class="config-section">
                  <p class="section-heading">Stream settings</p>
                  <div class="space-y-3">

                    <!-- Codec + Buffer -->
                    <div class="grid grid-cols-2 gap-3">
                      <div>
                        <label class="param-label block mb-1.5">Codec</label>
                        <select v-model="addCodec" class="field">
                          <option value="opus">Opus (recommended)</option>
                          <option value="flac">FLAC (lossless)</option>
                          <option value="pcm">PCM (raw, no compression)</option>
                        </select>
                      </div>
                      <div>
                        <label class="param-label block mb-1.5">Buffer (ms)</label>
                        <input v-model.number="addBufMs" type="number"
                               min="100" max="10000" step="100"
                               class="field field-mono" />
                      </div>
                    </div>

                    <!-- Idle detection -->
                    <div class="idle-section">
                      <label class="boolean-row">
                        <span class="toggle-wrap">
                          <input v-model="addIdleEnabled" type="checkbox" class="sr-only" />
                          <span class="toggle" :class="addIdleEnabled ? 'toggle-on' : ''"></span>
                        </span>
                        <span>
                          <span class="param-label">Idle detection</span>
                          <span class="param-desc block">Mark stream idle after this many ms with no input data</span>
                        </span>
                      </label>

                      <div v-if="addIdleEnabled" class="idle-extra">
                        <div class="grid grid-cols-2 gap-3">
                          <div>
                            <label class="param-label block mb-1.5">Timeout (ms)</label>
                            <input v-model.number="addIdleMs" type="number"
                                   min="500" max="30000" step="500"
                                   class="field field-mono" />
                          </div>
                          <div class="flex items-end pb-px">
                            <label class="boolean-row">
                              <span class="toggle-wrap">
                                <input v-model="addSilence" type="checkbox" class="sr-only" />
                                <span class="toggle" :class="addSilence ? 'toggle-on' : ''"></span>
                              </span>
                              <span>
                                <span class="param-label">Silence on idle</span>
                                <span class="param-desc block">Emit silence frames so clients don't underrun</span>
                              </span>
                            </label>
                          </div>
                        </div>
                      </div>
                    </div>

                  </div>
                </section>

                <!-- Source URI (editable, synced) -->
                <section class="config-section">
                  <div class="flex items-center justify-between mb-2">
                    <p class="section-heading" style="margin-bottom:0;">Source URI</p>
                    <button v-if="uriOverridden" @click="resetRawUri"
                            class="uri-reset-btn">
                      <span class="mdi mdi-refresh" style="font-size:12px;"></span>
                      Reset to generated
                    </button>
                  </div>
                  <textarea
                    v-model="rawUri"
                    @input="onRawUriEdit"
                    rows="2"
                    class="field field-mono uri-textarea"
                    placeholder="Source URI will appear here"
                  ></textarea>
                  <p v-if="uriOverridden"
                     style="font-size:11px;color:#fbbf24;margin-top:4px;">
                    <span class="mdi mdi-pencil-outline"></span>
                    Manual override — form changes won't update this URI
                  </p>
                </section>

                <!-- TOML preview (collapsible) -->
                <section class="config-section">
                  <button @click="showToml = !showToml"
                          class="flex items-center gap-1.5 w-full"
                          style="font-size:11px;color:var(--text-muted);">
                    <span class="mdi" :class="showToml ? 'mdi-chevron-down' : 'mdi-chevron-right'"
                          style="font-size:13px;"></span>
                    <span class="section-heading" style="margin-bottom:0;">TOML preview</span>
                  </button>
                  <div v-if="showToml" class="toml-preview">
                    <pre class="toml-code">{{ tomlSnippet }}</pre>
                  </div>
                </section>

                <!-- Feedback -->
                <div v-if="addInfo" class="feedback-box"
                     :class="addInfo.startsWith('✓') ? 'feedback-ok' : 'feedback-err'">
                  {{ addInfo }}
                </div>

              </div>
            </div>
          </div>

          <!-- Dialog footer -->
          <div class="dialog-footer">
            <button @click="closeDialog" class="btn-ghost">Cancel</button>
            <button @click="submitAdd" :disabled="!canSave" class="btn-primary">
              <span v-if="saving" class="mdi mdi-loading spin"></span>
              {{ saving ? 'Saving…' : 'Save to config' }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <EqControl
      v-if="editingEqStreamId"
      :stream-id="editingEqStreamId"
      @close="editingEqStreamId = null"
    />
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

/* ── Stream list ── */
.stream-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  gap: 12px;
  border-color: var(--border);
  transition: background 0.15s;
}
.stream-row:hover { background: var(--bg-hover); }
.stream-icon {
  width: 32px; height: 32px;
  border-radius: 8px;
  display: flex; align-items: center; justify-content: center;
  background: var(--accent-dim);
  flex-shrink: 0;
}

/* ── Dialog overlay ── */
.dialog-overlay {
  position: fixed; inset: 0; z-index: 50;
  display: flex; align-items: center; justify-content: center;
  padding: 16px;
  background: rgba(2, 5, 10, 0.85);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
}
.dialog-panel {
  width: 100%; max-width: 920px; max-height: 92vh;
  overflow: hidden;
  display: flex; flex-direction: column;
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  border-radius: 18px;
  box-shadow: 0 40px 100px rgba(0,0,0,0.6), 0 0 0 1px rgba(56,189,248,0.04);
}
.dialog-header {
  display: flex; align-items: flex-start; justify-content: space-between; gap: 16px;
  padding: 20px 24px 16px;
  border-bottom: 1px solid var(--border);
}
.dialog-title {
  font-family: var(--font-display);
  font-size: 17px; font-weight: 700; color: var(--text-primary);
}
.dialog-sub { font-size: 12px; color: var(--text-muted); margin-top: 2px; }
.dialog-close {
  width: 32px; height: 32px;
  display: flex; align-items: center; justify-content: center;
  border-radius: 8px; color: var(--text-muted);
  background: transparent; border: none; cursor: pointer;
  transition: background 0.15s, color 0.15s; flex-shrink: 0;
}
.dialog-close:hover { background: var(--bg-hover); color: var(--text-primary); }

.dialog-body { flex: 1; overflow-y: auto; padding: 0; }
.dialog-grid {
  display: grid;
  min-height: 100%;
}
@media (min-width: 640px) {
  .dialog-grid { grid-template-columns: 220px 1fr; }
}
.dialog-footer {
  display: flex; justify-content: flex-end; gap: 10px;
  padding: 14px 24px;
  border-top: 1px solid var(--border);
}

/* ── Type panel (left) ── */
.type-panel {
  padding: 18px 16px;
  border-right: 1px solid var(--border);
  overflow-y: auto;
}
.group-label {
  font-size: 10px; font-weight: 600; letter-spacing: 0.08em;
  text-transform: uppercase; color: var(--text-muted);
  margin-bottom: 6px; margin-top: 2px;
}
.type-btn {
  display: flex; align-items: center; gap: 6px;
  padding: 7px 9px;
  border-radius: 7px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  transition: background 0.12s, color 0.12s, border-color 0.12s;
  font-family: var(--font-sans);
  text-align: left; width: 100%;
}
.type-btn:hover { background: var(--bg-hover); color: var(--text-secondary); }
.type-btn-active {
  background: var(--accent-dim);
  border-color: var(--accent-border);
  color: var(--accent);
}

.type-hint-box {
  margin-top: 16px;
  background: rgba(56, 189, 248, 0.04);
  border: 1px solid var(--accent-border);
  border-radius: 10px;
  padding: 12px 14px;
}

/* ── Config panel (right) ── */
.config-panel {
  padding: 20px 24px;
  display: flex; flex-direction: column; gap: 0;
  overflow-y: auto;
}
.config-section {
  padding-bottom: 18px;
  margin-bottom: 18px;
  border-bottom: 1px solid var(--border);
}
.config-section:last-child { border-bottom: none; margin-bottom: 0; padding-bottom: 0; }

.section-heading {
  font-size: 10.5px; font-weight: 700; letter-spacing: 0.07em;
  text-transform: uppercase; color: var(--text-muted);
  margin-bottom: 12px;
}

/* ── Param rows ── */
.param-label {
  font-size: 12px; font-weight: 600; color: var(--text-secondary);
}
.param-desc {
  font-size: 11px; color: var(--text-muted); margin-top: 1px;
}
.req { color: var(--accent); margin-left: 2px; }

.param-two-col {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
  align-items: start;
}

/* ── Toggle switch ── */
.boolean-row {
  display: flex; align-items: flex-start; gap: 10px; cursor: pointer;
}
.toggle-wrap {
  flex-shrink: 0;
  margin-top: 1px;
}
.toggle {
  display: block;
  width: 32px; height: 18px;
  border-radius: 9px;
  background: var(--bg-hover);
  border: 1px solid var(--border-mid);
  position: relative;
  transition: background 0.2s, border-color 0.2s;
}
.toggle::after {
  content: '';
  position: absolute;
  top: 2px; left: 2px;
  width: 12px; height: 12px;
  border-radius: 50%;
  background: var(--text-muted);
  transition: transform 0.2s, background 0.2s;
}
.toggle-on {
  background: var(--accent-mid);
  border-color: var(--accent);
}
.toggle-on::after {
  transform: translateX(14px);
  background: white;
}

/* ── No-params note ── */
.no-params-note {
  display: flex; align-items: center;
  padding: 12px 14px;
  border-radius: 10px;
  font-size: 12.5px;
  background: var(--green-dim);
  border: 1px solid var(--green-border);
  color: var(--text-secondary);
}

/* ── Idle section ── */
.idle-section {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 12px 14px;
}
.idle-extra {
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
}

/* ── Source URI ── */
.uri-textarea {
  width: 100%;
  resize: vertical;
  font-size: 11.5px;
  line-height: 1.6;
}
.uri-reset-btn {
  display: flex; align-items: center; gap: 4px;
  font-size: 11px; color: var(--text-muted);
  background: transparent; border: 1px solid var(--border-mid);
  border-radius: 6px; padding: 3px 8px; cursor: pointer;
  transition: color 0.15s, background 0.15s;
}
.uri-reset-btn:hover { color: var(--text-primary); background: var(--bg-hover); }

/* ── TOML preview ── */
.toml-preview {
  margin-top: 8px;
  background: rgba(4, 8, 15, 0.7);
  border: 1px solid var(--border-mid);
  border-radius: 10px;
  padding: 12px 14px;
}
.toml-code {
  font-family: var(--font-mono);
  font-size: 11.5px; color: var(--text-secondary);
  white-space: pre-wrap; word-break: break-all;
  line-height: 1.65;
}

/* ── Feedback ── */
.feedback-box {
  padding: 10px 14px; border-radius: 10px;
  font-size: 12px; line-height: 1.5;
}
.feedback-ok  { background: var(--green-dim); border: 1px solid var(--green-border); color: var(--green); }
.feedback-err { background: var(--red-dim);   border: 1px solid var(--red-border);   color: var(--red); }

/* ── Meta dual pane ── */
.meta-grid {
  display: grid; gap: 12px;
}
@media (min-width: 520px) {
  .meta-grid { grid-template-columns: 1fr 1fr; }
}
.meta-list {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  overflow: hidden;
  min-height: 120px;
}
.meta-stream-btn {
  display: flex; align-items: center; gap: 8px;
  width: 100%; padding: 8px 12px;
  background: transparent; border: none; border-bottom: 1px solid var(--border);
  cursor: pointer; text-align: left;
  transition: background 0.12s;
  color: var(--text-secondary);
}
.meta-stream-btn:last-child { border-bottom: none; }
.meta-stream-btn:hover { background: var(--bg-hover); }

.meta-chain-item {
  display: flex; align-items: center; gap: 8px;
  padding: 7px 10px;
  border-bottom: 1px solid var(--border);
  color: var(--text-secondary);
}
.meta-chain-item:last-child { border-bottom: none; }

.meta-priority {
  width: 20px; height: 20px;
  border-radius: 50%;
  background: var(--accent-dim);
  border: 1px solid var(--accent-border);
  display: flex; align-items: center; justify-content: center;
  font-size: 10px; font-weight: 700; color: var(--accent);
  flex-shrink: 0;
}
.chain-btn {
  width: 22px; height: 22px;
  display: flex; align-items: center; justify-content: center;
  background: transparent; border: 1px solid var(--border-mid);
  border-radius: 5px; cursor: pointer; color: var(--text-muted);
  font-size: 13px;
  transition: background 0.12s, color 0.12s;
}
.chain-btn:hover { background: var(--bg-hover); color: var(--text-primary); }
.chain-btn:disabled { opacity: 0.3; cursor: default; }
.chain-btn-remove:hover { background: var(--red-dim); border-color: var(--red-border); color: var(--red); }

.spin {
  animation: spin-icon 0.8s linear infinite;
  display: inline-block;
}
</style>
