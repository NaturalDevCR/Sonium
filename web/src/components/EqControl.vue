<template>
  <div class="eq-container" :class="{ 'eq-disabled': !enabled }">
    <!-- Header with Toggle -->
    <div class="eq-header">
      <div class="flex items-center gap-2">
        <span class="eq-title">Parametric EQ</span>
        <div 
          class="eq-status-indicator" 
          :class="enabled ? 'eq-status-active' : 'eq-status-inactive'"
        ></div>
      </div>
      
      <div class="flex items-center gap-4">
        <button class="eq-reset-btn" @click="reset" title="Reset all bands to flat">
          <span class="mdi mdi-restore mr-1"></span>Reset
        </button>
        <div class="eq-toggle-wrapper" @click="$emit('update:enabled', !enabled)">
          <div class="eq-toggle" :class="{ 'eq-toggle-on': enabled }">
            <div class="eq-toggle-handle"></div>
          </div>
          <span class="eq-toggle-label">{{ enabled ? 'Active' : 'Bypassed' }}</span>
        </div>
      </div>
    </div>

    <!-- Visualizer / Plot -->
    <div class="eq-plot-container" ref="plotContainer">
      <canvas ref="plotCanvas" class="eq-plot-canvas"></canvas>
      
      <!-- Interactive Points -->
      <div 
        v-for="(band, idx) in bands" 
        :key="idx"
        class="eq-node"
        :class="[`eq-node-${idx}`, { 'eq-node-active': activeBand === idx }]"
        :style="getNodeStyle(band)"
        @mousedown="startDrag(idx, $event)"
        @touchstart="startDrag(idx, $event)"
      >
        <span class="eq-node-label">{{ idx + 1 }}</span>
      </div>
    </div>

    <!-- Band Controls -->
    <div class="eq-bands-controls">
      <div 
        v-for="(band, idx) in bands" 
        :key="idx" 
        class="band-strip"
        :class="{ 'band-strip-active': activeBand === idx }"
        @mouseenter="activeBand = idx"
      >
        <div class="band-header">
          <span class="band-number">{{ idx + 1 }}</span>
          <select 
            v-model="band.filter_type" 
            class="band-type-select"
            @change="update"
          >
            <option value="peaking">Peak</option>
            <option value="high_pass">HPF</option>
            <option value="low_pass">LPF</option>
          </select>
        </div>

        <div class="band-params">
          <div class="param">
            <label>Freq</label>
            <div class="value-input">
              <input type="number" v-model.number="band.freq_hz" min="20" max="20000" @change="update" />
              <span>Hz</span>
            </div>
          </div>
          
          <div class="param" v-if="band.filter_type === 'peaking'">
            <label>Gain</label>
            <div class="value-input">
              <input type="number" v-model.number="band.gain_db" min="-18" max="18" step="0.1" @change="update" />
              <span>dB</span>
            </div>
          </div>

          <div class="param">
            <label>Q</label>
            <div class="value-input">
              <input type="number" v-model.number="band.q" min="0.1" max="10" step="0.1" @change="update" />
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, computed } from 'vue';
import type { EqBand, FilterType } from '@/lib/api';
import { useServerStore } from '@/stores/server';

const props = defineProps<{
  streamId: string;
  modelValue: EqBand[] | undefined;
  enabled: boolean;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', bands: EqBand[]): void;
  (e: 'update:enabled', val: boolean): void;
}>();

const store = useServerStore();

// Internal state
const bands = ref<EqBand[]>([]);
const activeBand = ref<number | null>(null);
const plotCanvas = ref<HTMLCanvasElement | null>(null);
const plotContainer = ref<HTMLDivElement | null>(null);

const DEFAULT_BANDS: EqBand[] = [
  { filter_type: 'peaking', freq_hz: 100,   gain_db: 0, q: 1.0 },
  { filter_type: 'peaking', freq_hz: 500,   gain_db: 0, q: 1.0 },
  { filter_type: 'peaking', freq_hz: 2000,  gain_db: 0, q: 1.0 },
  { filter_type: 'peaking', freq_hz: 8000,  gain_db: 0, q: 1.0 },
];

// Initialize bands from props
const initBands = () => {
  if (props.modelValue && props.modelValue.length > 0) {
    // Fill up to 4 bands if fewer are provided
    const existing = props.modelValue.map(b => ({ ...b }));
    while (existing.length < 4) {
      existing.push({ ...DEFAULT_BANDS[existing.length] });
    }
    bands.value = existing.slice(0, 4);
  } else {
    bands.value = DEFAULT_BANDS.map(b => ({ ...b }));
  }
};

watch(() => props.modelValue, () => {
  // Only update if external value is different and not currently dragging
  if (!isDragging.value) {
    initBands();
  }
}, { deep: true });

onMounted(() => {
  initBands();
  setTimeout(() => {
    recalculateCurve();
    startAnimation();
  }, 100); // Wait for layout
});

onUnmounted(() => {
  stopAnimation();
});

// ── Update Logic ──────────────────────────────────────────────────────────

const update = () => {
  emit('update:modelValue', bands.value.map(b => ({ ...b })));
  recalculateCurve();
  draw();
};

const reset = () => {
  bands.value = DEFAULT_BANDS.map(b => ({ ...b }));
  update();
};

// ── Dragging Logic ────────────────────────────────────────────────────────

const isDragging = ref(false);
const dragIdx = ref(-1);

const startDrag = (idx: number, e: MouseEvent | TouchEvent) => {
  isDragging.value = true;
  dragIdx.value = idx;
  activeBand.value = idx;
  
  window.addEventListener('mousemove', handleDrag);
  window.addEventListener('mouseup', stopDrag);
  window.addEventListener('touchmove', handleDrag);
  window.addEventListener('touchend', stopDrag);
  
  e.preventDefault();
};

const handleDrag = (e: MouseEvent | TouchEvent) => {
  if (!isDragging.value || !plotContainer.value) return;
  
  const rect = plotContainer.value.getBoundingClientRect();
  const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX;
  const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY;
  
  const x = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
  const y = Math.max(0, Math.min(1, (clientY - rect.top) / rect.height));
  
  // Frequency: Logarithmic scale 20Hz - 20kHz
  const minF = Math.log10(20);
  const maxF = Math.log10(20000);
  const freq = Math.pow(10, minF + x * (maxF - minF));
  
  const band = bands.value[dragIdx.value];
  band.freq_hz = Math.round(freq);
  
  if (band.filter_type === 'peaking') {
    // Gain: linear +/- 18dB
    band.gain_db = Math.round((0.5 - y) * 36 * 10) / 10;
  }
  
  update();
};

const stopDrag = () => {
  isDragging.value = false;
  dragIdx.value = -1;
  window.removeEventListener('mousemove', handleDrag);
  window.removeEventListener('mouseup', stopDrag);
  window.removeEventListener('touchmove', handleDrag);
  window.removeEventListener('touchend', stopDrag);
};

// ── Plot Geometry ─────────────────────────────────────────────────────────

const freqToX = (f: number, width: number) => {
  const minF = Math.log10(20);
  const maxF = Math.log10(20000);
  return ((Math.log10(f) - minF) / (maxF - minF)) * width;
};

const gainToY = (g: number, height: number) => {
  // Range +/- 18dB
  const range = 18;
  return height / 2 - (g / range) * (height / 2);
};

const getNodeStyle = (band: EqBand) => {
  if (!plotContainer.value) return {};
  const width = plotContainer.value.clientWidth;
  const height = plotContainer.value.clientHeight;
  
  const x = freqToX(band.freq_hz, width);
  const y = band.filter_type === 'peaking' ? gainToY(band.gain_db, height) : height / 2;
  
  return {
    left: `${x}px`,
    top: `${y}px`,
  };
};

// ── Drawing Logic ─────────────────────────────────────────────────────────

// Pre-calculate curve points to avoid heavy math in draw loop
const curvePoints = ref<{x: number, y: number}[]>([]);
const sampleRate = 44100;

const recalculateCurve = () => {
  if (!plotContainer.value) return;
  const width = plotContainer.value.clientWidth;
  const height = plotContainer.value.clientHeight;
  const points: {x: number, y: number}[] = [];
  
  for (let x = 0; x <= width; x += 3) { // Increased step to 3 for better performance
    const ratio = x / width;
    const minF = Math.log10(20);
    const maxF = Math.log10(20000);
    const f = Math.pow(10, minF + ratio * (maxF - minF));
    
    let totalGain = 0;
    bands.value.forEach(b => {
      totalGain += getMagnitude(f, sampleRate, b);
    });
    
    points.push({ x, y: gainToY(totalGain, height) });
  }
  curvePoints.value = points;
};

let animationId: number | null = null;
const rmsHistory = ref<number[]>(new Array(40).fill(-90));

const startAnimation = () => {
  let lastRtaUpdate = 0;
  const loop = (timestamp: number) => {
    // Throttled RTA update (30fps is enough for visualizer)
    if (timestamp - lastRtaUpdate > 33) {
      const currentRms = getStreamRms();
      rmsHistory.value.push(currentRms);
      if (rmsHistory.value.length > 40) rmsHistory.value.shift();
      lastRtaUpdate = timestamp;
      draw();
    }
    animationId = requestAnimationFrame(loop);
  };
  animationId = requestAnimationFrame(loop);
};

const stopAnimation = () => {
  if (animationId) cancelAnimationFrame(animationId);
};

const getStreamRms = () => {
  return store.streamLevels[props.streamId] || -90;
};

// Biquad Magnitude Response calculation
// This is a simplified version for visualization
const getMagnitude = (f: number, sampleRate: number, band: EqBand) => {
  const w0 = 2 * Math.PI * band.freq_hz / sampleRate;
  const alpha = Math.sin(w0) / (2 * band.q);
  const A = Math.pow(10, band.gain_db / 40);
  const w = 2 * Math.PI * f / sampleRate;
  const cosW = Math.cos(w);
  const cosW0 = Math.cos(w0);

  let b0, b1, b2, a0, a1, a2;

  if (band.filter_type === 'peaking') {
    b0 = 1 + alpha * A;
    b1 = -2 * cosW0;
    b2 = 1 - alpha * A;
    a0 = 1 + alpha / A;
    a1 = -2 * cosW0;
    a2 = 1 - alpha / A;
  } else if (band.filter_type === 'high_pass') {
    b0 = (1 + cosW0) / 2;
    b1 = -(1 + cosW0);
    b2 = (1 + cosW0) / 2;
    a0 = 1 + alpha;
    a1 = -2 * cosW0;
    a2 = 1 - alpha;
  } else { // low_pass
    b0 = (1 - cosW0) / 2;
    b1 = 1 - cosW0;
    b2 = (1 - cosW0) / 2;
    a0 = 1 + alpha;
    a1 = -2 * cosW0;
    a2 = 1 - alpha;
  }

  // Transfer function H(z) = (b0 + b1*z^-1 + b2*z^-2) / (a0 + a1*z^-1 + a2*z^-2)
  // evaluated at z = exp(j*w)
  const phi = Math.sin(w / 2) ** 2;
  const num = (b0 + b1 + b2) ** 2 - 4 * (b0 * b1 + 4 * b0 * b2 + b1 * b2) * phi + 16 * b0 * b2 * phi * phi;
  const den = (a0 + a1 + a2) ** 2 - 4 * (a0 * a1 + 4 * a0 * a2 + a1 * a2) * phi + 16 * a0 * a2 * phi * phi;
  
  if (den <= 0) return 0;
  return 10 * Math.log10(num / den);
};

const draw = () => {
  const canvas = plotCanvas.value;
  if (!canvas || !plotContainer.value) return;
  
  const dpr = window.devicePixelRatio || 1;
  const width = plotContainer.value.clientWidth;
  const height = plotContainer.value.clientHeight;
  
  if (canvas.width !== width * dpr || canvas.height !== height * dpr) {
    canvas.width = width * dpr;
    canvas.height = height * dpr;
  }
  
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  
  ctx.scale(dpr, dpr);
  ctx.clearRect(0, 0, width, height);

  // ── 1. Draw Grid ────────────────────────────────────────────────────────
  ctx.strokeStyle = '#2d2d2d';
  ctx.lineWidth = 1;
  ctx.beginPath();
  // Hz markers
  [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000].forEach(f => {
    const x = freqToX(f, width);
    ctx.moveTo(x, 0);
    ctx.lineTo(x, height);
    
    ctx.fillStyle = '#666';
    ctx.font = '9px Inter';
    const label = f >= 1000 ? `${f/1000}k` : f.toString();
    ctx.fillText(label, x + 2, height - 4);
  });
  // dB markers
  [-12, -6, 0, 6, 12].forEach(db => {
    const y = gainToY(db, height);
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
    if (db !== 0) {
      ctx.fillStyle = '#666';
      ctx.fillText(`${db}dB`, 4, y - 2);
    }
  });
  ctx.stroke();

  // Zero line highlight
  ctx.strokeStyle = '#444';
  ctx.beginPath();
  ctx.moveTo(0, height / 2);
  ctx.lineTo(width, height / 2);
  ctx.stroke();

  // ── 2. Draw Simulated RTA ───────────────────────────────────────────────
  // We use RMS history to create a moving "spectrum" effect
  if (props.enabled) {
    const rms = rmsHistory.value[rmsHistory.value.length - 1];
    const normalized = Math.max(0, (rms + 80) / 80); // 0 to 1
    
    ctx.fillStyle = 'rgba(56, 189, 248, 0.08)';
    const barCount = 40;
    const barWidth = width / barCount;
    for (let i = 0; i < barCount; i++) {
      // Simulate frequency peaks using some noise and volume
      const noise = Math.sin(Date.now() / 500 + i * 0.5) * 0.2 + 0.8;
      const h = normalized * height * 0.6 * noise * (1 - Math.abs(i - 20) / 30);
      ctx.fillRect(i * barWidth, height - h, barWidth - 1, h);
    }
  }

  // ── 3. Draw Combined Curve ──────────────────────────────────────────────
  if (curvePoints.value.length > 0) {
    ctx.beginPath();
    ctx.lineWidth = 3;
    ctx.lineJoin = 'round';
    ctx.strokeStyle = props.enabled ? '#38bdf8' : '#666';
    
    curvePoints.value.forEach((p, i) => {
      if (i === 0) ctx.moveTo(p.x, p.y);
      else ctx.lineTo(p.x, p.y);
    });
    ctx.stroke();

    // Fill area under curve
    const gradient = ctx.createLinearGradient(0, 0, 0, height);
    gradient.addColorStop(0, 'rgba(56, 189, 248, 0.2)');
    gradient.addColorStop(0.5, 'rgba(56, 189, 248, 0.05)');
    gradient.addColorStop(1, 'rgba(56, 189, 248, 0)');
    ctx.fillStyle = gradient;
    ctx.lineTo(curvePoints.value[curvePoints.value.length - 1].x, height);
    ctx.lineTo(0, height);
    ctx.closePath();
    if (props.enabled) ctx.fill();
  }
};

</script>

<style scoped>
.eq-container {
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  padding: 16px;
  margin-top: 12px;
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  box-shadow: 0 4px 20px rgba(0,0,0,0.1);
}

.eq-disabled {
  border-color: var(--border-dim);
  box-shadow: none;
}

.eq-disabled .eq-plot-container {
  opacity: 0.6;
  filter: grayscale(0.5);
}

.eq-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 20px;
}

.eq-title {
  font-family: var(--font-display);
  font-weight: 700;
  font-size: 14px;
  color: var(--text-primary);
  letter-spacing: 0.02em;
}

.eq-status-indicator {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  transition: all 0.3s ease;
}
.eq-status-active { 
  background: var(--accent); 
  box-shadow: 0 0 10px var(--accent);
}
.eq-status-inactive { background: var(--text-muted); }

.eq-reset-btn {
  font-size: 11px;
  font-weight: 500;
  color: var(--text-muted);
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  padding: 4px 10px;
  border-radius: 6px;
  transition: all 0.2s;
  display: flex;
  align-items: center;
}
.eq-reset-btn:hover {
  background: var(--bg-hover);
  color: var(--text-secondary);
  border-color: var(--accent-border);
}

/* Custom Toggle Switch */
.eq-toggle-wrapper {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
}

.eq-toggle {
  width: 36px;
  height: 18px;
  background: var(--border-mid);
  border-radius: 20px;
  position: relative;
  transition: background 0.3s;
}
.eq-toggle-on { background: var(--accent); }

.eq-toggle-handle {
  width: 14px;
  height: 14px;
  background: white;
  border-radius: 50%;
  position: absolute;
  top: 2px;
  left: 2px;
  transition: transform 0.3s cubic-bezier(0.175, 0.885, 0.32, 1.275);
  box-shadow: 0 1px 3px rgba(0,0,0,0.2);
}
.eq-toggle-on .eq-toggle-handle { transform: translateX(18px); }

.eq-toggle-label {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  min-width: 50px;
}

/* Plot Styles */
.eq-plot-container {
  width: 100%;
  height: 180px;
  background: #121212;
  border-radius: 10px;
  position: relative;
  overflow: hidden;
  border: 1px solid #1a1a1a;
  margin-bottom: 24px;
}

.eq-plot-canvas {
  width: 100%;
  height: 100%;
  display: block;
}

.eq-node {
  position: absolute;
  width: 24px;
  height: 24px;
  margin-left: -12px;
  margin-top: -12px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: grab;
  z-index: 10;
  transition: transform 0.2s, box-shadow 0.2s;
  background: rgba(18, 18, 18, 0.8);
  border: 2px solid #38bdf8;
  color: #38bdf8;
  box-shadow: 0 0 8px rgba(56, 189, 248, 0.3);
}

.eq-node:active { cursor: grabbing; transform: scale(1.1); }
.eq-node-active { border-width: 3px; box-shadow: 0 0 15px rgba(56, 189, 248, 0.6); }

.eq-node-label {
  font-size: 10px;
  font-weight: 800;
  font-family: var(--font-mono);
}

/* Band Controls */
.eq-bands-controls {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 12px;
}

.band-strip {
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  border-radius: 10px;
  padding: 10px;
  transition: all 0.2s;
}

.band-strip-active {
  border-color: var(--accent-border);
  background: var(--bg-hover);
}

.band-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}

.band-number {
  width: 20px;
  height: 20px;
  background: var(--bg-surface);
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  font-weight: 700;
  color: var(--text-secondary);
}

.band-type-select {
  background: transparent;
  border: none;
  font-size: 11px;
  font-weight: 700;
  color: var(--text-primary);
  text-transform: uppercase;
  letter-spacing: 0.05em;
  cursor: pointer;
  outline: none;
}

.band-params {
  display: grid;
  grid-template-columns: 1fr;
  gap: 8px;
}

.param {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.param label {
  font-size: 10px;
  color: var(--text-muted);
  text-transform: uppercase;
}

.value-input {
  display: flex;
  align-items: center;
  gap: 4px;
}

.value-input input {
  width: 54px;
  background: var(--bg-surface);
  border: 1px solid var(--border-mid);
  border-radius: 4px;
  padding: 2px 4px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-primary);
  text-align: right;
  outline: none;
}

.value-input input:focus { border-color: var(--accent); }

.value-input span {
  font-size: 10px;
  color: var(--text-muted);
  width: 18px;
}

</style>
