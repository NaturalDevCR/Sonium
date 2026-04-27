<script setup lang="ts">
import { useRouter, useRoute, RouterView } from 'vue-router';
import { useAuthStore } from '@/stores/auth';

const auth   = useAuthStore();
const router = useRouter();
const route  = useRoute();

const nav = [
  { name: 'admin-dashboard', icon: 'mdi-view-dashboard-outline', label: 'Overview'  },
  { name: 'admin-streams',   icon: 'mdi-music-box-multiple-outline', label: 'Streams'  },
  { name: 'admin-groups',    icon: 'mdi-speaker-multiple',      label: 'Groups'    },
  { name: 'admin-clients',   icon: 'mdi-devices',               label: 'Clients'   },
  { name: 'admin-system',    icon: 'mdi-toolbox-outline',        label: 'System'    },
  { name: 'admin-config',    icon: 'mdi-code-braces',            label: 'Config'    },
  { name: 'admin-users',     icon: 'mdi-account-multiple-outline', label: 'Users'   },
];

const mobileNav = nav.slice(0, 5);

function isActive(name: string) {
  return route.name === name;
}
</script>

<template>
  <div class="admin-shell min-h-screen flex flex-col lg:flex-row">

    <!-- ── Sidebar (desktop) ──────────────────────────────────────────────── -->
    <aside class="hidden lg:flex flex-col w-52 shrink-0 sidebar-panel min-h-screen">

      <!-- Logo -->
      <div class="px-4 py-5 border-b" style="border-color: var(--border);">
        <div class="flex items-center gap-3">
          <img src="/sonium-logo.png" alt="" class="h-8 w-8 object-contain shrink-0" />
          <div>
            <p class="sidebar-brand">SONIUM</p>
            <p class="sidebar-sub">Audio Server</p>
          </div>
        </div>
      </div>

      <!-- Nav -->
      <nav class="flex-1 px-3 py-4 space-y-0.5 overflow-y-auto">
        <div class="section-label px-1 mb-2">Navigation</div>
        <button
          v-for="item in nav" :key="item.name"
          @click="router.push({ name: item.name })"
          class="nav-item"
          :class="{ active: isActive(item.name) }"
        >
          <span class="mdi text-base shrink-0" :class="item.icon"></span>
          {{ item.label }}
        </button>
      </nav>

      <!-- Bottom actions -->
      <div class="px-3 pb-4 pt-3 border-t space-y-0.5" style="border-color: var(--border);">
        <button
          @click="router.push('/')"
          class="nav-item"
        >
          <span class="mdi mdi-remote text-base shrink-0"></span>
          Control Panel
        </button>
        <button
          @click="auth.logout(); router.push('/login')"
          class="nav-item"
          style="color: #f87171 !important;"
        >
          <span class="mdi mdi-logout text-base shrink-0"></span>
          Sign Out
        </button>
      </div>
    </aside>

    <!-- ── Main content ───────────────────────────────────────────────────── -->
    <div class="flex-1 flex flex-col min-w-0">

      <!-- Mobile top bar -->
      <header class="lg:hidden sticky top-0 z-20 mobile-topbar safe-top px-4 py-3 flex items-center justify-between">
        <div class="flex items-center gap-2.5">
          <img src="/sonium-logo.png" alt="" class="h-7 w-7 object-contain" />
          <span class="sidebar-brand text-sm">SONIUM</span>
        </div>
        <div class="flex items-center gap-2">
          <button
            @click="router.push('/')"
            class="w-8 h-8 flex items-center justify-center rounded-lg transition-colors"
            style="color: var(--text-muted);"
          >
            <span class="mdi mdi-remote text-lg"></span>
          </button>
        </div>
      </header>

      <!-- Page content -->
      <main class="flex-1 p-4 lg:p-6 max-w-4xl w-full mx-auto pb-24 lg:pb-6">
        <RouterView />
      </main>

      <!-- Mobile bottom nav -->
      <nav class="lg:hidden fixed bottom-0 inset-x-0 z-20 mobile-bottomnav safe-bottom">
        <div class="flex">
          <button
            v-for="item in mobileNav" :key="item.name"
            @click="router.push({ name: item.name })"
            class="flex-1 flex flex-col items-center gap-1 py-2.5 transition-colors"
            :style="isActive(item.name)
              ? 'color: var(--accent);'
              : 'color: var(--text-muted);'"
          >
            <span class="mdi text-xl" :class="item.icon"></span>
            <span class="text-xs font-medium">{{ item.label }}</span>
          </button>
        </div>
      </nav>
    </div>
  </div>
</template>

<style scoped>
.admin-shell {
  background: var(--bg-base);
}

.sidebar-panel {
  background: var(--bg-surface);
  border-right: 1px solid var(--border);
  position: relative;
}
/* Subtle glow on sidebar */
.sidebar-panel::before {
  content: '';
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: 1px;
  background: linear-gradient(to bottom, transparent, rgba(56, 189, 248, 0.06), transparent);
  pointer-events: none;
}

.sidebar-brand {
  font-family: var(--font-display);
  font-size: 13px;
  font-weight: 800;
  letter-spacing: 0.16em;
  color: var(--text-primary);
}

.sidebar-sub {
  font-size: 10px;
  color: var(--text-muted);
  letter-spacing: 0.04em;
}

.mobile-topbar {
  background: rgba(4, 8, 15, 0.9);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border-bottom: 1px solid var(--border);
}

.mobile-bottomnav {
  background: rgba(4, 8, 15, 0.94);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-top: 1px solid var(--border);
}
</style>
