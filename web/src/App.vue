<script setup lang="ts">
import { ref, computed } from 'vue';
import { useServerStore } from './stores/server';
import ClientCard    from './components/ClientCard.vue';
import GroupCard     from './components/GroupCard.vue';
import NewGroupModal from './components/NewGroupModal.vue';

const store = useServerStore();

const showNewGroup = ref(false);

const clientNames = computed(() =>
  Object.fromEntries(
    store.clients.map((c) => [c.id, c.client_name || c.hostname]),
  ),
);

function formatUptime(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = s % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${ss}s`;
  return `${ss}s`;
}
</script>

<template>
  <div class="app">
    <!-- Header -->
    <header>
      <div class="logo">
        <span class="logo-icon">🎵</span>
        <span class="logo-text">Sonium</span>
      </div>
      <div class="status-row">
        <span class="stat">
          <span class="stat-val">{{ store.connectedClients.length }}</span>
          <span class="stat-label">clients</span>
        </span>
        <span class="stat">
          <span class="stat-val">{{ store.groups.length }}</span>
          <span class="stat-label">groups</span>
        </span>
        <span class="stat">
          <span class="stat-val">{{ store.streams.length }}</span>
          <span class="stat-label">streams</span>
        </span>
        <span v-if="store.uptime" class="stat">
          <span class="stat-val">{{ formatUptime(store.uptime) }}</span>
          <span class="stat-label">uptime</span>
        </span>
      </div>
    </header>

    <!-- Loading / error -->
    <div v-if="store.loading" class="center-msg">Loading…</div>
    <div v-else-if="store.error" class="center-msg error">
      {{ store.error }}
      <button @click="store.loadAll()">Retry</button>
    </div>

    <!-- Main content -->
    <main v-else>
      <!-- Clients section -->
      <section>
        <h2 class="section-title">Clients</h2>
        <p v-if="store.clients.length === 0" class="empty-msg">
          No clients detected yet. Start a Sonium client on any device on your network.
        </p>
        <div class="grid">
          <ClientCard
            v-for="client in store.clients"
            :key="client.id"
            :client="client"
            :groups="store.groups"
          />
        </div>
      </section>

      <!-- Groups section -->
      <section>
        <div class="section-header">
          <h2 class="section-title">Groups</h2>
          <button class="btn-add" @click="showNewGroup = true">+ New Group</button>
        </div>
        <div class="grid">
          <GroupCard
            v-for="group in store.groups"
            :key="group.id"
            :group="group"
            :streams="store.streams"
            :client-names="clientNames"
          />
        </div>
      </section>

      <!-- Streams section -->
      <section>
        <h2 class="section-title">Streams</h2>
        <p v-if="store.streams.length === 0" class="empty-msg">No streams configured.</p>
        <div class="streams-list">
          <div v-for="s in store.streams" :key="s.id" class="stream-row">
            <span class="stream-id">{{ s.id }}</span>
            <span class="stream-codec">{{ s.codec.toUpperCase() }} · {{ s.format }}</span>
            <span class="stream-status" :class="s.status">{{ s.status }}</span>
          </div>
        </div>
      </section>
    </main>

    <!-- New group modal -->
    <NewGroupModal
      v-if="showNewGroup"
      :streams="store.streams"
      @close="showNewGroup = false"
    />
  </div>
</template>

<style>
/* ── CSS Variables (dark theme) ─────────────────────────────────────────── */
:root {
  --bg:       #0f0f14;
  --card-bg:  #16161f;
  --input-bg: #1e1e2a;
  --border:   #2a2a3a;
  --text:     #e8e8f0;
  --muted:    #6b6b80;
  --accent:   #5c6ef8;
  --hover:    #ffffff0d;
  --chip-bg:  #2a2a3a;
}

*, *::before, *::after { box-sizing: border-box; }

html, body {
  margin: 0; padding: 0;
  background: var(--bg);
  color: var(--text);
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  font-size: 14px;
  -webkit-font-smoothing: antialiased;
}

select, input, button { font-family: inherit; }
</style>

<style scoped>
.app { min-height: 100vh; display: flex; flex-direction: column; }

header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 24px;
  border-bottom: 1px solid var(--border);
  position: sticky; top: 0;
  background: var(--bg);
  z-index: 10;
}
.logo { display: flex; align-items: center; gap: 8px; }
.logo-icon { font-size: 1.3rem; }
.logo-text  { font-weight: 700; font-size: 1.1rem; letter-spacing: -0.02em; }

.status-row { display: flex; gap: 20px; }
.stat { display: flex; flex-direction: column; align-items: center; line-height: 1.2; }
.stat-val   { font-weight: 700; font-size: 1.1rem; }
.stat-label { font-size: 0.65rem; color: var(--muted); text-transform: uppercase; letter-spacing: 0.05em; }

main { padding: 24px; display: flex; flex-direction: column; gap: 32px; max-width: 1200px; width: 100%; margin: 0 auto; }

.section-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px; }
.section-title  { margin: 0 0 12px; font-size: 0.75rem; font-weight: 700; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); }
.section-header .section-title { margin-bottom: 0; }

.btn-add {
  background: var(--accent);
  border: none; border-radius: 6px;
  color: #fff; cursor: pointer;
  font-size: 0.78rem; font-weight: 600;
  padding: 5px 12px;
  transition: opacity 0.15s;
}
.btn-add:hover { opacity: 0.85; }

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 12px;
}

.streams-list { display: flex; flex-direction: column; gap: 8px; }
.stream-row {
  display: flex; align-items: center; gap: 12px;
  background: var(--card-bg); border: 1px solid var(--border);
  border-radius: 8px; padding: 10px 14px;
  font-size: 0.82rem;
}
.stream-id     { font-weight: 600; flex: 1; }
.stream-codec  { color: var(--muted); }
.stream-status { font-size: 0.7rem; font-weight: 600; text-transform: uppercase; border-radius: 999px; padding: 2px 8px; }
.stream-status.playing { background: #1a6640; color: #6effa8; }
.stream-status.idle    { background: #2a2a3a; color: #888; }
.stream-status.error   { background: #5a1a1a; color: #ff8080; }

.center-msg {
  flex: 1; display: flex; align-items: center; justify-content: center;
  flex-direction: column; gap: 12px; color: var(--muted);
}
.center-msg.error { color: #ff6b6b; }
.center-msg button {
  background: var(--accent); border: none; border-radius: 6px;
  color: #fff; cursor: pointer; font-size: 0.85rem; padding: 6px 16px;
}

.empty-msg { color: var(--muted); font-size: 0.82rem; font-style: italic; margin: 0; }
</style>
