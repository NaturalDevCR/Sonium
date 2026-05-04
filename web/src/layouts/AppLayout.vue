<script setup lang="ts">
import { computed, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import { useServerStore } from '@/stores/server';

const auth = useAuthStore();
const store = useServerStore();
const route = useRoute();
const router = useRouter();

const mobileMenuOpen = ref(false);

const isAdminRoute = computed(() => route.path.startsWith('/admin'));

const navItems = computed(() => [
  { to: '/',        label: 'Dashboard', icon: 'mdi-view-dashboard',    show: true },
  { to: '/matrix',  label: 'Matrix',    icon: 'mdi-grid',              show: auth.isOperator },
  { to: '/sync',    label: 'Sync',      icon: 'mdi-sync',              show: auth.isOperator },
  { to: '/admin',   label: 'Admin',     icon: 'mdi-shield-crown-outline', show: auth.isAdmin },
]);

const adminItems = [
  { to: '/admin',          label: 'Overview', icon: 'mdi-view-dashboard-outline' },
  { to: '/admin/streams',  label: 'Streams',  icon: 'mdi-music-box-multiple-outline' },
  { to: '/admin/groups',   label: 'Groups',   icon: 'mdi-speaker-multiple' },
  { to: '/admin/clients',  label: 'Clients',  icon: 'mdi-devices' },
  { to: '/admin/health',   label: 'Health',   icon: 'mdi-heart-pulse' },
  { to: '/admin/system',   label: 'System',   icon: 'mdi-toolbox-outline' },
  { to: '/admin/config',   label: 'Config',   icon: 'mdi-code-braces' },
  { to: '/admin/users',    label: 'Users',    icon: 'mdi-account-multiple-outline' },
];

function isActive(path: string) {
  if (path === '/') return route.path === '/';
  return route.path.startsWith(path);
}

const connectionColor = computed(() => {
  if (store.connected) return 'bg-emerald-400';
  if (store.connecting) return 'bg-amber-400';
  return 'bg-rose-400';
});

const connectionText = computed(() => {
  if (store.connected) return 'Live';
  if (store.connecting) return 'Connecting';
  return 'Offline';
});
</script>

<template>
  <div class="min-h-screen bg-slate-950 text-slate-100 flex">
    <!-- Ambient background glows -->
    <div class="fixed inset-0 overflow-hidden pointer-events-none z-0">
      <div class="absolute -top-[20%] -left-[10%] w-[50%] h-[50%] rounded-full bg-cyan-500/[0.03] blur-[120px]"></div>
      <div class="absolute top-[40%] -right-[10%] w-[40%] h-[40%] rounded-full bg-violet-500/[0.03] blur-[120px]"></div>
      <div class="absolute -bottom-[10%] left-[20%] w-[35%] h-[35%] rounded-full bg-rose-500/[0.02] blur-[120px]"></div>
    </div>

    <!-- ── Desktop Sidebar ─────────────────────────────────────────────── -->
    <aside class="hidden lg:flex flex-col w-[220px] shrink-0 fixed inset-y-0 left-0 z-40">
      <!-- Glass background -->
      <div class="absolute inset-0 bg-slate-950/80 backdrop-blur-xl border-r border-white/[0.06]"></div>

      <div class="relative flex flex-col h-full">
        <!-- Brand -->
        <div class="px-5 pt-6 pb-4">
          <div class="flex items-center gap-3">
            <div class="relative">
              <img src="/sonium-logo.png" alt="Sonium" class="w-8 h-8 object-contain relative z-10" />
              <div class="absolute inset-0 bg-cyan-400/20 blur-lg rounded-full"></div>
            </div>
            <div>
              <div class="font-display text-sm font-extrabold tracking-[0.15em] text-white">SONIUM</div>
              <div class="text-[10px] text-slate-500 font-medium tracking-wider">SERVER UI</div>
            </div>
          </div>
        </div>

        <!-- Connection indicator -->
        <div class="px-5 pb-4">
          <div class="flex items-center gap-2 px-3 py-1.5 rounded-full bg-white/[0.03] border border-white/[0.06] w-fit">
            <span class="w-1.5 h-1.5 rounded-full animate-pulse-glow" :class="connectionColor"></span>
            <span class="text-[11px] font-semibold text-slate-400">{{ connectionText }}</span>
          </div>
        </div>

        <!-- Main Nav -->
        <nav class="px-3 flex flex-col gap-1">
          <router-link
            v-for="item in navItems.filter(n => n.show)"
            :key="item.to"
            :to="item.to"
            class="group flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm font-medium transition-all duration-200"
            :class="isActive(item.to)
              ? 'bg-white/[0.08] text-cyan-300 border border-white/[0.08] shadow-[0_0_20px_rgba(34,211,238,0.08)]'
              : 'text-slate-400 hover:text-slate-200 hover:bg-white/[0.04] border border-transparent'"
          >
            <span class="mdi text-lg transition-transform group-hover:scale-110" :class="item.icon"></span>
            {{ item.label }}
          </router-link>
        </nav>

        <!-- Admin subnav -->
        <div v-if="isAdminRoute" class="px-3 mt-4">
          <div class="px-3 mb-2 text-[10px] font-bold tracking-[0.12em] text-slate-600 uppercase">Administration</div>
          <nav class="flex flex-col gap-0.5">
            <router-link
              v-for="item in adminItems"
              :key="item.to"
              :to="item.to"
              class="group flex items-center gap-3 px-3 py-2 rounded-lg text-xs font-medium transition-all duration-200"
              :class="isActive(item.to)
                ? 'text-cyan-300 bg-white/[0.05]'
                : 'text-slate-500 hover:text-slate-300 hover:bg-white/[0.03]'"
            >
              <span class="mdi text-base" :class="item.icon"></span>
              {{ item.label }}
            </router-link>
          </nav>
        </div>

        <div class="flex-1"></div>

        <!-- User + Logout -->
        <div class="p-4 border-t border-white/[0.06]">
          <div class="flex items-center gap-3 mb-3 px-1">
            <div class="w-8 h-8 rounded-full bg-gradient-to-br from-cyan-400/20 to-violet-500/20 border border-white/[0.08] flex items-center justify-center">
              <span class="mdi mdi-account text-slate-300 text-sm"></span>
            </div>
            <div class="min-w-0">
              <div class="text-xs font-semibold text-slate-300 truncate">{{ auth.user?.username }}</div>
              <div class="text-[10px] text-slate-500 uppercase tracking-wider">{{ auth.user?.role }}</div>
            </div>
          </div>
          <button
            @click="auth.logout(); router.push('/login')"
            class="w-full flex items-center justify-center gap-2 px-3 py-2 rounded-xl text-xs font-medium text-rose-400/80 hover:text-rose-300 hover:bg-rose-500/[0.08] border border-transparent hover:border-rose-500/20 transition-all"
          >
            <span class="mdi mdi-logout"></span>
            Sign out
          </button>
        </div>
      </div>
    </aside>

    <!-- ── Main Content ────────────────────────────────────────────────── -->
    <div class="flex-1 flex flex-col min-w-0 lg:ml-[220px] relative z-10">
      <!-- Mobile Header -->
      <header class="lg:hidden flex items-center justify-between px-4 py-3 sticky top-0 z-40 bg-slate-950/90 backdrop-blur-xl border-b border-white/[0.06]">
        <div class="flex items-center gap-2.5">
          <img src="/sonium-logo.png" alt="" class="w-7 h-7 object-contain" />
          <span class="font-display text-sm font-extrabold tracking-[0.15em]">SONIUM</span>
        </div>
        <div class="flex items-center gap-2">
          <div class="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-white/[0.03] border border-white/[0.06]">
            <span class="w-1.5 h-1.5 rounded-full" :class="connectionColor"></span>
            <span class="text-[10px] text-slate-400">{{ connectionText }}</span>
          </div>
          <button @click="mobileMenuOpen = !mobileMenuOpen" class="w-8 h-8 flex items-center justify-center rounded-lg bg-white/[0.05] text-slate-300">
            <span class="mdi text-lg" :class="mobileMenuOpen ? 'mdi-close' : 'mdi-menu'"></span>
          </button>
        </div>
      </header>

      <!-- Mobile Menu Dropdown -->
      <div v-if="mobileMenuOpen" class="lg:hidden fixed inset-x-0 top-[53px] z-30 bg-slate-950/95 backdrop-blur-xl border-b border-white/[0.06] p-4 space-y-1 animate-fade-up">
        <router-link
          v-for="item in navItems.filter(n => n.show)"
          :key="item.to"
          :to="item.to"
          @click="mobileMenuOpen = false"
          class="flex items-center gap-3 px-4 py-3 rounded-xl text-sm font-medium transition-all"
          :class="isActive(item.to) ? 'bg-white/[0.08] text-cyan-300' : 'text-slate-400'"
        >
          <span class="mdi text-lg" :class="item.icon"></span>
          {{ item.label }}
        </router-link>
        <div v-if="isAdminRoute" class="pt-2 mt-2 border-t border-white/[0.06]">
          <div class="px-4 py-2 text-[10px] font-bold tracking-wider text-slate-600 uppercase">Admin</div>
          <router-link
            v-for="item in adminItems"
            :key="item.to"
            :to="item.to"
            @click="mobileMenuOpen = false"
            class="flex items-center gap-3 px-4 py-2.5 rounded-xl text-xs font-medium transition-all"
            :class="isActive(item.to) ? 'text-cyan-300 bg-white/[0.05]' : 'text-slate-500'"
          >
            <span class="mdi text-base" :class="item.icon"></span>
            {{ item.label }}
          </router-link>
        </div>
        <button
          @click="auth.logout(); router.push('/login'); mobileMenuOpen = false"
          class="w-full flex items-center gap-3 px-4 py-3 rounded-xl text-sm font-medium text-rose-400/80 mt-2"
        >
          <span class="mdi mdi-logout"></span>
          Sign out
        </button>
      </div>

      <!-- Desktop Header -->
      <header class="hidden lg:flex items-center justify-between px-8 py-5">
        <div>
          <h1 class="text-xl font-display font-bold text-white tracking-tight">{{ route.meta?.title || 'Sonium' }}</h1>
          <p class="text-xs text-slate-500 mt-0.5">Multi-room audio server control</p>
        </div>
        <div class="flex items-center gap-3">
          <div v-if="store.connectedClients.length" class="flex items-center gap-2 px-4 py-2 rounded-full glass text-xs font-medium text-slate-300">
            <span class="mdi mdi-speaker text-emerald-400"></span>
            {{ store.connectedClients.length }} clients online
          </div>
          <div class="flex items-center gap-2 px-4 py-2 rounded-full glass text-xs font-medium text-slate-300">
            <span class="mdi mdi-account-circle text-slate-400"></span>
            {{ auth.user?.username }}
            <span class="px-1.5 py-0.5 rounded-md bg-cyan-500/10 text-cyan-400 text-[10px] font-bold uppercase tracking-wider border border-cyan-500/20">{{ auth.user?.role }}</span>
          </div>
        </div>
      </header>

      <!-- Page Content -->
      <main class="flex-1 px-4 pb-24 lg:px-8 lg:pb-8">
        <slot />
      </main>
    </div>
  </div>
</template>
