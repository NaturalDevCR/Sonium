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
        :class="[`eq-node-${idx}`, { 'eq-node-active': activeBand === idx, 'eq-node-disabled': !band.enabled }]"
        :style="getNodeStyle(band)"
        @mousedown="startDrag(idx, $event)"
        @touchstart="startDrag(idx, $event)"
        @wheel.prevent="adjustQ(idx, $event)"
        :title="`Band ${idx + 1}: scroll to change Q`"
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
        :class="{ 'band-strip-active': activeBand === idx, 'band-strip-disabled': !band.enabled }"
        @mouseenter="activeBand = idx"
      >
        <div class="band-header">
          <div class="flex items-center gap-2">
            <button 
              class="band-toggle-mini" 
              :class="{ 'band-toggle-mini-on': band.enabled }"
              @click="band.enabled = !band.enabled; update()"
              :title="band.enabled ? 'Bypass Band' : 'Enable Band'"
            >
              <span class="mdi" :class="band.enabled ? 'mdi-power' : 'mdi-power-off'"></span>
            </button>
            <span class="band-number">{{ idx + 1 }}</span>
          </div>
          <select 
            v-model="band.filter_type" 
            class="band-type-select"
            @change="update"
          >
            <option value="peaking">Peak</option>
            <option value="low_shelf">Low Shelf</option>
            <option value="high_shelf">High Shelf</option>
            <option value="high_pass">HPF</option>
            <option value="low_pass">LPF</option>
            <option value="notch">Notch</option>
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
          
          <div class="param" v-if="['peaking', 'low_shelf', 'high_shelf'].includes(band.filter_type)">
            <label>Gain</label>
            <div class="value-input">
              <input type="number" v-model.number="band.gain_db" min="-24" max="24" step="0.1" @change="update" />
              <span>dB</span>
            </div>
          </div>

          <div class="param">
            <label>Q</label>
            <div class="value-input">
              <input type="number" v-model.number="band.q" min="0.1" max="10" step="0.1" @change="update" />
            </div>
          </div>

          <div class="param" v-if="['high_pass', 'low_pass'].includes(band.filter_type)">
            <label>Slope</label>
            <div class="value-input">
              <select v-model.number="band.slope_db_per_oct" @change="update">
                <option :value="12">12 dB/oct</option>
                <option :value="24">24 dB/oct</option>
                <option :value="36">36 dB/oct</option>
                <option :value="48">48 dB/oct</option>
              </select>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, nextTick } from 'vue';
import type { EqBand } from '@/lib/api';
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
let resizeObserver: ResizeObserver | null = null;

const DEFAULT_BANDS: EqBand[] = [
  { filter_type: 'peaking', freq_hz: 63,    gain_db: 0, q: 1.0, slope_db_per_oct: 12, enabled: true },
  { filter_type: 'peaking', freq_hz: 160,   gain_db: 0, q: 1.0, slope_db_per_oct: 12, enabled: true },
  { filter_type: 'peaking', freq_hz: 400,   gain_db: 0, q: 1.0, slope_db_per_oct: 12, enabled: true },
  { filter_type: 'peaking', freq_hz: 1000,  gain_db: 0, q: 1.0, slope_db_per_oct: 12, enabled: true },
  { filter_type: 'peaking', freq_hz: 2500,  gain_db: 0, q: 1.0, slope_db_per_oct: 12, enabled: true },
  { filter_type: 'peaking', freq_hz: 8000,  gain_db: 0, q: 1.0, slope_db_per_oct: 12, enabled: true },
];

// Initialize bands from props
const initBands = () => {
  if (props.modelValue && props.modelValue.length > 0) {
    // Fill up to 6 bands if fewer are provided.
    const existing: EqBand[] = props.modelValue.map(b => ({ ...b, slope_db_per_oct: b.slope_db_per_oct ?? 12 }));
    while (existing.length < 6) {
      existing.push({ ...DEFAULT_BANDS[existing.length] });
    }
    bands.value = existing.slice(0, 6);
  } else {
    bands.value = DEFAULT_BANDS.map(b => ({ ...b }));
  }
};

watch(() => props.modelValue, () => {
  // Only update if external value is different and not currently dragging
  if (!isDragging.value) {
    initBands();
    scheduleRender({ recalculateCurve: true });
  }
}, { deep: true });

watch(() => props.enabled, () => {
  scheduleRender();
});

watch(() => store.streamLevels[props.streamId], (level) => {
  rmsHistory.push(level ?? -90);
  if (rmsHistory.length > 40) rmsHistory.shift();
  scheduleRender();
});

onMounted(async () => {
  initBands();
  await nextTick();
  if (plotContainer.value && typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(() => {
      scheduleRender({ recalculateCurve: true });
    });
    resizeObserver.observe(plotContainer.value);
  }
  scheduleRender({ recalculateCurve: true });
});

onUnmounted(() => {
  stopDrag();
  resizeObserver?.disconnect();
  if (renderFrameId !== null) cancelAnimationFrame(renderFrameId);
  if (dragFrameId !== null) cancelAnimationFrame(dragFrameId);
});

// ── Update Logic ──────────────────────────────────────────────────────────

const update = () => {
  scheduleRender({ emitModel: true, recalculateCurve: true });
};

const reset = () => {
  bands.value = DEFAULT_BANDS.map(b => ({ ...b }));
  update();
};

const adjustQ = (idx: number, e: WheelEvent) => {
  const band = bands.value[idx];
  if (!band) return;
  activeBand.value = idx;
  const direction = e.deltaY < 0 ? 1 : -1;
  const step = e.shiftKey ? 0.5 : 0.1;
  band.q = Math.round(Math.max(0.1, Math.min(10, band.q + direction * step)) * 10) / 10;
  update();
};

// ── Dragging Logic ────────────────────────────────────────────────────────

const isDragging = ref(false);
const dragIdx = ref(-1);
let dragFrameId: number | null = null;
let pendingDragPosition: { clientX: number; clientY: number } | null = null;

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

  const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX;
  const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY;

  pendingDragPosition = { clientX, clientY };
  if (dragFrameId === null) {
    dragFrameId = requestAnimationFrame(() => {
      dragFrameId = null;
      applyPendingDrag();
    });
  }
  e.preventDefault();
};

const stopDrag = () => {
  isDragging.value = false;
  dragIdx.value = -1;
  pendingDragPosition = null;
  window.removeEventListener('mousemove', handleDrag);
  window.removeEventListener('mouseup', stopDrag);
  window.removeEventListener('touchmove', handleDrag);
  window.removeEventListener('touchend', stopDrag);
};

const applyPendingDrag = () => {
  if (!isDragging.value || !plotContainer.value || !pendingDragPosition) return;

  const rect = plotContainer.value.getBoundingClientRect();
  const x = Math.max(0, Math.min(1, (pendingDragPosition.clientX - rect.left) / rect.width));
  const y = Math.max(0, Math.min(1, (pendingDragPosition.clientY - rect.top) / rect.height));

  const minF = Math.log10(20);
  const maxF = Math.log10(20000);
  const freq = Math.pow(10, minF + x * (maxF - minF));

  const band = bands.value[dragIdx.value];
  if (!band) return;
  band.freq_hz = Math.round(freq);

  if (['peaking', 'low_shelf', 'high_shelf'].includes(band.filter_type)) {
    band.gain_db = Math.round((0.5 - y) * 48 * 10) / 10;
  }

  update();
};

// ── Plot Geometry ─────────────────────────────────────────────────────────

const freqToX = (f: number, width: number) => {
  const minF = Math.log10(20);
  const maxF = Math.log10(20000);
  return ((Math.log10(f) - minF) / (maxF - minF)) * width;
};

const gainToY = (g: number, height: number) => {
  // Range +/- 24dB for visualization
  const range = 24;
  return height / 2 - (g / range) * (height / 2);
};

// Pre-calculate frequency map for the curve to avoid Math.pow/log in the loop
let frequencyMap: number[] = [];
const updateFrequencyMap = (width: number) => {
  const map = [];
  const minF = Math.log10(20);
  const maxF = Math.log10(20000);
  for (let x = 0; x <= width; x += 6) { // Increased step to 6
    const ratio = x / width;
    map.push(Math.pow(10, minF + ratio * (maxF - minF)));
  }
  frequencyMap = map;
};

const getNodeStyle = (band: EqBand) => {
  if (!plotContainer.value) return {};
  const width = plotContainer.value.clientWidth;
  const height = plotContainer.value.clientHeight;
  
  const x = freqToX(band.freq_hz, width);
  const hasGain = ['peaking', 'low_shelf', 'high_shelf'].includes(band.filter_type);
  const y = hasGain ? gainToY(band.gain_db, height) : height / 2;
  
  return {
    left: `${x}px`,
    top: `${y}px`,
  };
};

// ── Drawing Logic ─────────────────────────────────────────────────────────

// Pre-calculate curve points to avoid heavy math in draw loop
// Non-reactive state for high-frequency updates
let curvePoints: {x: number, y: number}[] = [];
let rmsHistory: number[] = new Array(40).fill(-90);
const sampleRate = 44100;
let renderFrameId: number | null = null;
let renderNeedsCurve = false;
let renderNeedsEmit = false;

// Grid cache
let gridCanvas: HTMLCanvasElement | null = null;

const recalculateCurve = () => {
  if (!plotContainer.value) return;
  const width = plotContainer.value.clientWidth;
  const height = plotContainer.value.clientHeight;
  
  if (frequencyMap.length === 0 || frequencyMap.length !== Math.floor(width / 6) + 1) {
    updateFrequencyMap(width);
  }

  const points: {x: number, y: number}[] = [];
  
  // Pre-calculate band coefficients once
  const activeBands = bands.value
    .filter(b => b.enabled)
    .flatMap(b => {
      const coeffs = getCoefficients(b, sampleRate);
      if (!coeffs) return [];
      const sections = ['high_pass', 'low_pass'].includes(b.filter_type)
        ? Math.max(1, Math.min(4, Math.round((b.slope_db_per_oct ?? 12) / 12)))
        : 1;
      return Array.from({ length: sections }, () => coeffs);
    })
    .filter(Boolean);

  frequencyMap.forEach((f, i) => {
    const x = i * 6;
    let totalGain = 0;
    activeBands.forEach(coeffs => {
      totalGain += getMagnitudeFromCoeffs(f, sampleRate, coeffs);
    });
    points.push({ x, y: gainToY(totalGain, height) });
  });
  
  curvePoints = points;
};

const flushRender = () => {
  renderFrameId = null;

  if (renderNeedsCurve) {
    recalculateCurve();
    renderNeedsCurve = false;
  }

  if (renderNeedsEmit) {
    emit('update:modelValue', bands.value.map(b => ({ ...b })));
    renderNeedsEmit = false;
  }

  draw();
};

const scheduleRender = (options: { emitModel?: boolean; recalculateCurve?: boolean } = {}) => {
  renderNeedsEmit ||= options.emitModel ?? false;
  renderNeedsCurve ||= options.recalculateCurve ?? false;

  if (renderFrameId !== null) return;
  renderFrameId = requestAnimationFrame(flushRender);
};

// Biquad Magnitude Response calculation
// Pre-calculate coefficients to avoid heavy math in frequency loop
const getCoefficients = (band: EqBand, sampleRate: number) => {
  const w0 = 2 * Math.PI * band.freq_hz / sampleRate;
  const alpha = Math.sin(w0) / (2 * band.q);
  const A = Math.pow(10, band.gain_db / 40);
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
  } else if (band.filter_type === 'low_pass') {
    b0 = (1 - cosW0) / 2;
    b1 = 1 - cosW0;
    b2 = (1 - cosW0) / 2;
    a0 = 1 + alpha;
    a1 = -2 * cosW0;
    a2 = 1 - alpha;
  } else if (band.filter_type === 'low_shelf') {
    const sqrtA = Math.sqrt(A);
    b0 = A * ((A + 1) - (A - 1) * cosW0 + 2 * sqrtA * alpha);
    b1 = 2 * A * ((A - 1) - (A + 1) * cosW0);
    b2 = A * ((A + 1) - (A - 1) * cosW0 - 2 * sqrtA * alpha);
    a0 = (A + 1) + (A - 1) * cosW0 + 2 * sqrtA * alpha;
    a1 = -2 * ((A - 1) + (A + 1) * cosW0);
    a2 = (A + 1) + (A - 1) * cosW0 - 2 * sqrtA * alpha;
  } else if (band.filter_type === 'high_shelf') {
    const sqrtA = Math.sqrt(A);
    b0 = A * ((A + 1) + (A - 1) * cosW0 + 2 * sqrtA * alpha);
    b1 = -2 * A * ((A - 1) + (A + 1) * cosW0);
    b2 = A * ((A + 1) + (A - 1) * cosW0 - 2 * sqrtA * alpha);
    a0 = (A + 1) - (A - 1) * cosW0 + 2 * sqrtA * alpha;
    a1 = 2 * ((A - 1) - (A + 1) * cosW0);
    a2 = (A + 1) - (A - 1) * cosW0 - 2 * sqrtA * alpha;
  } else if (band.filter_type === 'notch') {
    b0 = 1;
    b1 = -2 * cosW0;
    b2 = 1;
    a0 = 1 + alpha;
    a1 = -2 * cosW0;
    a2 = 1 - alpha;
  } else {
    return null;
  }
  return { b0, b1, b2, a0, a1, a2 };
};

const getMagnitudeFromCoeffs = (f: number, sampleRate: number, coeffs: any) => {
  const w = 2 * Math.PI * f / sampleRate;
  const phi = Math.sin(w / 2) ** 2;
  const { b0, b1, b2, a0, a1, a2 } = coeffs;
  const num = (b0 + b1 + b2) ** 2 - 4 * (b0 * b1 + 4 * b0 * b2 + b1 * b2) * phi + 16 * b0 * b2 * phi * phi;
  const den = (a0 + a1 + a2) ** 2 - 4 * (a0 * a1 + 4 * a0 * a2 + a1 * a2) * phi + 16 * a0 * a2 * phi * phi;
  
  if (den <= 0) return 0;
  return 10 * Math.log10(num / den);
};

const drawGrid = (width: number, height: number, dpr: number) => {
  if (!gridCanvas) gridCanvas = document.createElement('canvas');
  if (gridCanvas.width !== width * dpr || gridCanvas.height !== height * dpr) {
    gridCanvas.width = width * dpr;
    gridCanvas.height = height * dpr;
  } else {
    return gridCanvas;
  }

  const ctx = gridCanvas.getContext('2d');
  if (!ctx) return null;

  ctx.scale(dpr, dpr);
  ctx.strokeStyle = '#222';
  ctx.lineWidth = 1;
  ctx.beginPath();
  // Hz markers
  [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000].forEach(f => {
    const x = freqToX(f, width);
    ctx.moveTo(x, 0);
    ctx.lineTo(x, height);
    
    ctx.fillStyle = '#444';
    ctx.font = '500 9px Inter';
    const label = f >= 1000 ? `${f/1000}k` : f.toString();
    ctx.fillText(label, x + 2, height - 6);
  });
  // dB markers
  [-24, -12, 0, 12, 24].forEach(db => {
    const y = gainToY(db, height);
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
    ctx.fillStyle = '#444';
    ctx.fillText(`${db > 0 ? '+' : ''}${db}dB`, 4, y - 4);
  });
  ctx.stroke();

  // Zero line highlight
  ctx.strokeStyle = 'rgba(255, 255, 255, 0.05)';
  ctx.beginPath();
  ctx.moveTo(0, height / 2);
  ctx.lineTo(width, height / 2);
  ctx.stroke();

  return gridCanvas;
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
    gridCanvas = null; // Invalidate grid cache
  }
  
  const ctx = canvas.getContext('2d', { alpha: false }); // Disable alpha for slight perf gain
  if (!ctx) return;
  
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  
  // Background
  ctx.fillStyle = '#0f0f0f';
  ctx.fillRect(0, 0, width, height);

  // ── 1. Draw Grid (from cache) ───────────────────────────────────────────
  const grid = drawGrid(width, height, dpr);
  if (grid) {
    ctx.drawImage(grid, 0, 0, width, height);
  }

  // ── 2. Draw level history from server RMS events ───────────────────────
  if (props.enabled) {
    const barCount = rmsHistory.length;
    const barWidth = width / barCount;
    ctx.fillStyle = 'rgba(56, 189, 248, 0.1)';

    for (let i = 0; i < barCount; i++) {
      const normalized = Math.max(0, (rmsHistory[i] + 70) / 70);
      const h = normalized * height * 0.5;
      if (h > 2) {
        ctx.fillRect(i * barWidth, height - h, barWidth - 1, h);
      }
    }
  }

  // ── 3. Draw Combined Curve ──────────────────────────────────────────────
  if (curvePoints.length > 0) {
    ctx.beginPath();
    ctx.lineWidth = 2.5; // Slightly thinner
    ctx.lineJoin = 'round';
    ctx.strokeStyle = props.enabled ? '#38bdf8' : '#555';
    
    // Use moveTo/lineTo for the curve
    for (let i = 0; i < curvePoints.length; i++) {
      const p = curvePoints[i];
      if (i === 0) ctx.moveTo(p.x, p.y);
      else ctx.lineTo(p.x, p.y);
    }
    ctx.stroke();

    // Fill area under curve
    if (props.enabled) {
      ctx.lineTo(curvePoints[curvePoints.length - 1].x, height);
      ctx.lineTo(0, height);
      ctx.closePath();
      
      const fillGradient = ctx.createLinearGradient(0, 0, 0, height);
      fillGradient.addColorStop(0, 'rgba(56, 189, 248, 0.12)');
      fillGradient.addColorStop(0.8, 'rgba(56, 189, 248, 0)');
      ctx.fillStyle = fillGradient;
      ctx.fill();
    }
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
  background: #0f0f0f;
  border-radius: 10px;
  position: relative;
  overflow: hidden;
  border: 1px solid #1a1a1a;
  margin-bottom: 24px;
  box-shadow: inset 0 2px 10px rgba(0,0,0,0.5);
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
  transition: transform 0.2s, box-shadow 0.2s, opacity 0.2s;
  background: rgba(15, 15, 15, 0.9);
  border: 2px solid #38bdf8;
  color: #38bdf8;
  box-shadow: 0 0 10px rgba(56, 189, 248, 0.3);
}

.eq-node:active { cursor: grabbing; transform: scale(1.15); }
.eq-node-active { border-width: 3px; box-shadow: 0 0 18px rgba(56, 189, 248, 0.6); z-index: 11; }
.eq-node-disabled { opacity: 0.3; border-style: dashed; filter: grayscale(1); box-shadow: none; }

.eq-node-label {
  font-size: 10px;
  font-weight: 900;
  user-select: none;
}

/* Band Strips */
.eq-bands-controls {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 12px;
}

.band-strip {
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  border-radius: 12px;
  padding: 12px;
  transition: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);
  position: relative;
  overflow: hidden;
}
.band-strip::before {
  content: '';
  position: absolute;
  top: 0; left: 0; width: 100%; height: 3px;
  background: var(--accent);
  opacity: 0;
  transition: opacity 0.2s;
}
.band-strip-active {
  border-color: var(--accent-border);
  background: var(--bg-hover);
  transform: translateY(-2px);
  box-shadow: 0 4px 12px rgba(0,0,0,0.15);
}
.band-strip-active::before { opacity: 1; }
.band-strip-disabled { opacity: 0.5; filter: grayscale(0.5); }

.band-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 14px;
}

.band-number {
  font-weight: 900;
  font-size: 12px;
  color: var(--text-muted);
  width: 18px;
  height: 18px;
  background: var(--bg-surface);
  border-radius: 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-dim);
}
.band-strip-active .band-number {
  color: var(--accent);
  border-color: var(--accent-border);
}

.band-toggle-mini {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  background: var(--bg-surface);
  border: 1px solid var(--border-mid);
  color: var(--text-muted);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  transition: all 0.2s;
  cursor: pointer;
}
.band-toggle-mini:hover {
  background: var(--bg-hover);
  color: var(--text-secondary);
}
.band-toggle-mini-on {
  color: var(--accent);
  border-color: var(--accent-border);
  background: rgba(56, 189, 248, 0.08);
}

.band-type-select {
  background: var(--bg-surface);
  border: 1px solid var(--border-mid);
  border-radius: 6px;
  padding: 3px 8px;
  font-size: 10px;
  font-weight: 700;
  color: var(--text-primary);
  text-transform: uppercase;
  letter-spacing: 0.05em;
  cursor: pointer;
}

.band-params {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.param {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.param label {
  font-size: 10px;
  text-transform: uppercase;
  font-weight: 800;
  color: var(--text-muted);
  letter-spacing: 0.08em;
}

.value-input {
  display: flex;
  align-items: center;
  gap: 6px;
}
.value-input input,
.value-input select {
  background: var(--bg-surface);
  border: 1px solid var(--border-mid);
  border-radius: 5px;
  padding: 2px 6px;
  width: 58px;
  font-size: 11px;
  font-weight: 700;
  color: var(--text-primary);
  text-align: right;
  font-family: var(--font-mono);
  outline: none;
  transition: border-color 0.2s;
}
.value-input select {
  width: 96px;
  text-align: left;
}
.value-input input:focus,
.value-input select:focus {
  border-color: var(--accent);
}
.value-input span {
  font-size: 10px;
  color: var(--text-muted);
  font-weight: 600;
  width: 20px;
}
</style>
