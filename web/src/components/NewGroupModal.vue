<script setup lang="ts">
import { ref } from 'vue';
import type { Stream } from '@/lib/api';
import { api } from '@/lib/api';

const props = defineProps<{ streams: Stream[] }>();
const emit  = defineEmits<{ close: [] }>();

const name     = ref('');
const streamId = ref(props.streams[0]?.id ?? '');
const busy     = ref(false);
const err      = ref('');

async function submit() {
  if (!name.value.trim()) { err.value = 'Name required'; return; }
  busy.value = true;
  err.value  = '';
  try {
    await api.createGroup(name.value.trim(), streamId.value);
    emit('close');
  } catch (e) {
    err.value = String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="modal">
      <h3>New Group</h3>
      <label class="field">
        <span>Name</span>
        <input v-model="name" placeholder="Living Room" @keydown.enter="submit" />
      </label>
      <label class="field">
        <span>Stream</span>
        <select v-model="streamId">
          <option value="">— none —</option>
          <option v-for="s in streams" :key="s.id" :value="s.id">
            {{ s.id }} ({{ s.codec }})
          </option>
        </select>
      </label>
      <p v-if="err" class="error">{{ err }}</p>
      <div class="actions">
        <button class="btn-secondary" @click="emit('close')">Cancel</button>
        <button class="btn-primary" :disabled="busy" @click="submit">
          {{ busy ? 'Creating…' : 'Create' }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed; inset: 0;
  background: #00000088;
  display: flex; align-items: center; justify-content: center;
  z-index: 50;
}
.modal {
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 24px;
  min-width: 320px;
  display: flex; flex-direction: column; gap: 14px;
}
h3 { margin: 0; font-size: 1rem; }
.field { display: flex; flex-direction: column; gap: 4px; font-size: 0.82rem; color: var(--muted); }
.field input, .field select {
  background: var(--input-bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  font-size: 0.88rem;
  padding: 6px 10px;
}
.error { color: #ff6b6b; font-size: 0.8rem; margin: 0; }
.actions { display: flex; justify-content: flex-end; gap: 8px; }
.btn-secondary {
  background: none; border: 1px solid var(--border);
  border-radius: 6px; color: var(--muted); cursor: pointer;
  font-size: 0.85rem; padding: 6px 14px;
}
.btn-primary {
  background: var(--accent); border: none;
  border-radius: 6px; color: #fff; cursor: pointer;
  font-size: 0.85rem; font-weight: 600; padding: 6px 14px;
}
.btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
