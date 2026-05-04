<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import { useServerStore } from '@/stores/server';
import ConnectionStatus from '@/components/ConnectionStatus.vue';

const auth = useAuthStore();
const store = useServerStore();
const route = useRoute();
const router = useRouter();

const version = computed(() => store.serverVersion || '0.1.79');

const isAdminRoute = computed(() => route.path.startsWith('/admin'));

// Collapsible sidebar on desktop
const sidebarCollapsed = ref(false);

// Navigation items (operator+ see all; viewer sees limited)
const mainNav = computed(() => [
  { path: '/',          name: 'dashboard',    icon: 'mdi-view-dashboard',     label: 'Dashboard',   show: true },
  { path: '/matrix',    name: 'matrix',       icon: 'mdi-grid',               label: 'Matrix',      show: auth.isOperator },
  { path: '/sync',      name: 'sync',         icon: 'mdi-sync',               label: 'Sync',        show: auth.isOperator },
  { path: '/admin',     name: 'admin',        icon: 'mdi-cog-outline',        label: 'Admin',       show: auth.isAdmin },
]);

const adminSubNav = [
  { path: '/admin',          name: 'admin-dashboard', icon: 'mdi-view-dashboard-outline', label: 'Overview' },
  { path: '/admin/streams',  name: 'admin-streams',   icon: 'mdi-music-box-multiple-outline', label: 'Streams' },
  { path: '/admin/groups',   name: 'admin-groups',    icon: 'mdi-speaker-multiple',      label: 'Groups' },
  { path: '/admin/clients',  name: 'admin-clients',   icon: 'mdi-devices',               label: 'Clients' },
  { path: '/admin/health',   name: 'admin-health',    icon: 'mdi-heart-pulse',           label: 'Health' },
  { path: '/admin/system',   name: 'admin-system',    icon: 'mdi-toolbox-outline',       label: 'System' },
  { path: '/admin/config',   name: 'admin-config',    icon: 'mdi-code-braces',           label: 'Config' },
  { path: '/admin/users',    name: 'admin-users',     icon: 'mdi-account-multiple-outline', label: 'Users' },
];

function isActive(itemPath: string) {
  if (itemPath === '/') return route.path === '/';
  if (itemPath === '/admin') return route.path === '/admin' || route.path === '/admin/';
  return route.path.startsWith(itemPath);
}

const mobileNav = computed(() => {
  const items = mainNav.value.filter(n => n.show);
  // On admin pages, show admin sub-nav in mobile bottom bar too
  if (isAdminRoute.value) {
    return adminSubNav.slice(0, 5).map(n => ({ ...n, path: n.path }));
  }
  return items.map(n => ({ ...n, path: n.path }));
});

watch(() => route.path, () => {
  // Auto-expand sidebar when entering admin on desktop
  if (isAdminRoute.value) sidebarCollapsed.value = false;
});
</script>

<template>
  <div class="app-shell">
    <!-- ── Desktop Sidebar ─────────────────────────────────────────────── -->
    <aside
      class="hidden lg:flex flex-col sidebar"
      :class="{ collapsed: sidebarCollapsed }"
    >
      <!-- Brand -->
      <div class="sidebar-brand">
        <img src="/sonium-logo.png" alt="Sonium" class="logo-img" />
        <div v-if="!sidebarCollapsed" class="brand-text">
          <span class="brand-name">SONIUM</span>
          <span class="brand-version">v{{ version }}</span>
        </div>
      </div>

      <!-- Connection status -->
      <div class="sidebar-conn">
        <ConnectionStatus />
      </div>

      <!-- Main nav -->
      <nav class="sidebar-nav">
        <router-link
          v-for="item in mainNav.filter(n => n.show)"
          :key="item.name"
          :to="item.path"
          class="nav-row"
          :class="{ active: isActive(item.path) }"
          :title="item.label"
        >
          <span class="mdi text-lg" :class="item.icon"></span>
          <span v-if="!sidebarCollapsed" class="nav-label">{{ item.label }}</span>
        </router-link>
      </nav>

      <!-- Admin sub-nav (only when inside admin) -->
      <nav v-if="isAdminRoute && !sidebarCollapsed" class="sidebar-subnav">
        <div class="subnav-divider"></div>
        <span class="subnav-title">Administration</span>
        <router-link
          v-for="item in adminSubNav"
          :key="item.name"
          :to="item.path"
          class="nav-row sub"
          :class="{ active: route.name === item.name }"
        >
          <span class="mdi text-base" :class="item.icon"></span>
          <span class="nav-label">{{ item.label }}</span>
        </router-link>
      </nav>

      <div class="flex-1"></div>

      <!-- Bottom actions -->
      <div class="sidebar-footer">
        <button
          v-if="!sidebarCollapsed"
          @click="sidebarCollapsed = true"
          class="nav-row"
          title="Collapse sidebar"
        >
          <span class="mdi mdi-chevron-left text-lg"></span>
          <span class="nav-label">Collapse</span>
        </button>
        <button
          v-else
          @click="sidebarCollapsed = false"
          class="nav-row justify-center"
          title="Expand sidebar"
        >
          <span class="mdi mdi-chevron-right text-lg"></span>
        </button>

        <button
          @click="auth.logout(); router.push('/login')"
          class="nav-row danger"
          title="Sign out"
        >
          <span class="mdi mdi-logout text-lg"></span>
          <span v-if="!sidebarCollapsed" class="nav-label">Sign out</span>
        </button>
      </div>
    </aside>

    <!-- ── Main area ───────────────────────────────────────────────────── -->
    <div class="main-area">
      <!-- Mobile top bar -->
      <header class="lg:hidden mobile-header safe-top">
        <div class="flex items-center gap-3">
          <img src="/sonium-logo.png" alt="" class="h-7 w-7 object-contain" />
          <span class="font-display text-sm font-extrabold tracking-widest text-primary">SONIUM</span>
        </div>
        <ConnectionStatus />
      </header>

      <!-- Desktop top bar (contextual) -->
      <header class="hidden lg:flex desktop-header">
        <div class="flex items-center gap-3">
          <h1 class="page-title">{{ route.meta?.title || 'Sonium' }}</h1>
        </div>
        <div class="flex items-center gap-3">
          <div v-if="store.connectedClients.length" class="header-stat">
            <span class="mdi mdi-speaker text-sm"></span>
            {{ store.connectedClients.length }} online
          </div>
          <div class="flex items-center gap-2">
            <div class="user-badge">
              <span class="mdi mdi-account-circle text-lg"></span>
              <span class="text-sm">{{ auth.user?.username || 'User' }}</span>
              <span class="role-pill">{{ auth.user?.role }}</span>
            </div>
          </div>
        </div>
      </header>

      <!-- Page content -->
      <main class="page-content safe-bottom">
        <slot />
      </main>

      <!-- Mobile bottom nav -->
      <nav class="lg:hidden mobile-bottomnav safe-bottom">
        <router-link
          v-for="item in mobileNav"
          :key="item.name"
          :to="item.path"
          class="mobile-nav-item"
          :class="{ active: isActive(item.path) }"
        >
          <span class="mdi text-xl" :class="item.icon"></span>
          <span class="text-[10px] font-medium mt-0.5">{{ item.label }}</span>
        </router-link>
      </nav>
    </div>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  min-height: 100vh;
  background: var(--bg-base);
}

/* ── Sidebar ──────────────────────────────────────────────────────────── */
.sidebar {
  width: 240px;
  background: var(--bg-surface);
  border-right: 1px solid var(--border);
  position: fixed;
  inset: 0 auto 0 0;
  z-index: 40;
  transition: width 0.25s cubic-bezier(0.16, 1, 0.3, 1);
}
.sidebar.collapsed {
  width: 64px;
}
.sidebar.collapsed .sidebar-brand {
  justify-content: center;
  padding: 16px 0;
}
.sidebar.collapsed .logo-img {
  width: 32px;
  height: 32px;
}

.sidebar-brand {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 18px 16px 12px;
  transition: all 0.25s;
}
.logo-img {
  width: 28px;
  height: 28px;
  object-fit: contain;
  flex-shrink: 0;
}
.brand-name {
  font-family: var(--font-display);
  font-size: 14px;
  font-weight: 800;
  letter-spacing: 0.14em;
  color: var(--text-primary);
  line-height: 1;
}
.brand-version {
  font-size: 10px;
  color: var(--text-muted);
  letter-spacing: 0.02em;
}
.brand-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.sidebar-conn {
  padding: 0 16px 12px;
}
.sidebar.collapsed .sidebar-conn {
  display: flex;
  justify-content: center;
  padding: 0 0 12px;
}
.sidebar.collapsed .sidebar-conn :deep(.conn-label) {
  display: none;
}

.sidebar-nav {
  padding: 0 10px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.sidebar-subnav {
  padding: 0 10px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: 8px;
}
.subnav-divider {
  height: 1px;
  background: var(--border);
  margin: 4px 6px 10px;
}
.subnav-title {
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-muted);
  padding: 0 8px 6px;
}

.nav-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 10px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-muted);
  transition: all 0.15s ease;
  cursor: pointer;
  border: 1px solid transparent;
  text-decoration: none;
}
.nav-row:hover {
  color: var(--text-secondary);
  background: var(--bg-hover);
}
.nav-row.active {
  color: var(--accent);
  background: var(--accent-dim);
  border-color: var(--accent-border);
}
.nav-row.sub {
  font-size: 12.5px;
  padding: 7px 10px;
}
.nav-row.danger:hover {
  color: var(--red);
  background: var(--red-dim);
  border-color: var(--red-border);
}
.nav-label {
  white-space: nowrap;
  overflow: hidden;
}

.sidebar-footer {
  padding: 10px;
  border-top: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  gap: 2px;
}

/* ── Main area ────────────────────────────────────────────────────────── */
.main-area {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  margin-left: 240px;
  transition: margin-left 0.25s cubic-bezier(0.16, 1, 0.3, 1);
}
.sidebar.collapsed ~ .main-area {
  margin-left: 64px;
}

/* Desktop header */
.desktop-header {
  position: sticky;
  top: 0;
  z-index: 30;
  background: rgba(4, 8, 15, 0.85);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-bottom: 1px solid var(--border);
  padding: 12px 24px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.page-title {
  font-family: var(--font-display);
  font-size: 18px;
  font-weight: 700;
  color: var(--text-primary);
  letter-spacing: -0.01em;
}
.header-stat {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  border-radius: 20px;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  font-size: 12px;
  color: var(--text-secondary);
}
.user-badge {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 12px;
  border-radius: 20px;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  color: var(--text-secondary);
}
.role-pill {
  font-size: 9px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  padding: 2px 7px;
  border-radius: 6px;
  background: var(--accent-dim);
  color: var(--accent);
  border: 1px solid var(--accent-border);
}

/* Mobile header */
.mobile-header {
  position: sticky;
  top: 0;
  z-index: 30;
  background: rgba(4, 8, 15, 0.92);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border-bottom: 1px solid var(--border);
  padding: 10px 16px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}

/* Page content */
.page-content {
  flex: 1;
  padding: 16px;
  max-width: 1200px;
  width: 100%;
  margin: 0 auto;
}
@media (min-width: 1024px) {
  .page-content {
    padding: 24px;
  }
}

/* Mobile bottom nav */
.mobile-bottomnav {
  position: fixed;
  bottom: 0;
  left: 0;
  right: 0;
  z-index: 40;
  background: rgba(4, 8, 15, 0.94);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-top: 1px solid var(--border);
  display: flex;
  justify-content: space-around;
  padding-bottom: env(safe-area-inset-bottom);
}
.mobile-nav-item {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 8px 4px;
  color: var(--text-muted);
  text-decoration: none;
  transition: color 0.15s;
}
.mobile-nav-item.active {
  color: var(--accent);
}
</style>
