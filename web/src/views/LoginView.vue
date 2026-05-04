<script setup lang="ts">
import { ref } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { useAuthStore } from '@/stores/auth';

const auth = useAuthStore();
const router = useRouter();
const route = useRoute();

const username = ref('');
const password = ref('');
const error = ref('');
const loading = ref(false);

async function submit() {
  error.value = '';
  loading.value = true;
  try {
    await auth.login(username.value, password.value);
    const redirect = (route.query.redirect as string) || (auth.isAdmin ? '/admin' : '/');
    router.push(redirect);
  } catch (e: any) {
    error.value = e.message || 'Login failed';
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <div class="min-h-screen flex items-center justify-center relative overflow-hidden bg-slate-950">
    <!-- Ambient background -->
    <div class="absolute inset-0 overflow-hidden pointer-events-none">
      <div class="absolute top-[10%] left-[15%] w-[400px] h-[400px] rounded-full bg-cyan-500/[0.04] blur-[100px] animate-float"></div>
      <div class="absolute bottom-[10%] right-[15%] w-[350px] h-[350px] rounded-full bg-violet-500/[0.04] blur-[100px] animate-float" style="animation-delay: 1.5s;"></div>
      <div class="absolute top-[50%] left-[50%] -translate-x-1/2 -translate-y-1/2 w-[500px] h-[500px] rounded-full bg-rose-500/[0.02] blur-[120px]"></div>
    </div>

    <!-- Grid pattern overlay -->
    <div class="absolute inset-0 opacity-[0.03]" style="background-image: radial-gradient(circle, #fff 1px, transparent 1px); background-size: 32px 32px;"></div>

    <div class="relative z-10 w-full max-w-sm px-4">
      <!-- Logo + Brand -->
      <div class="text-center mb-10 animate-fade-up">
        <div class="relative inline-block mb-5">
          <img src="/sonium-logo.png" alt="Sonium" class="w-16 h-16 object-contain relative z-10 mx-auto" />
          <div class="absolute inset-0 bg-cyan-400/20 blur-2xl rounded-full"></div>
        </div>
        <h1 class="font-display text-3xl font-extrabold tracking-[0.15em] text-white mb-2">SONIUM</h1>
        <p class="text-sm text-slate-500 tracking-wide">Multi-room Audio Server</p>
      </div>

      <!-- Login Card -->
      <div class="glass p-7 space-y-5 animate-fade-up delay-100">
        <div>
          <label class="text-[10px] font-bold text-slate-500 uppercase tracking-wider mb-2 block">Username</label>
          <input
            v-model="username"
            type="text"
            autocomplete="username"
            placeholder="admin"
            @keyup.enter="submit"
            class="input-glass"
          />
        </div>

        <div>
          <label class="text-[10px] font-bold text-slate-500 uppercase tracking-wider mb-2 block">Password</label>
          <input
            v-model="password"
            type="password"
            autocomplete="current-password"
            @keyup.enter="submit"
            class="input-glass"
          />
        </div>

        <div v-if="error" class="flex items-center gap-2 px-4 py-3 rounded-xl text-xs bg-rose-500/10 border border-rose-500/20 text-rose-400">
          <span class="mdi mdi-alert-circle"></span>
          {{ error }}
        </div>

        <button
          @click="submit"
          :disabled="loading || !username || !password"
          class="w-full btn-gradient justify-center py-2.5"
        >
          <span v-if="loading" class="mdi mdi-loading animate-spin"></span>
          {{ loading ? 'Signing in…' : 'Sign in' }}
        </button>
      </div>

      <p class="text-center mt-6 text-[11px] text-slate-600 tracking-wide">
        Secure access · Sonium Audio
      </p>
    </div>
  </div>
</template>
