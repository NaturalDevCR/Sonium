<script setup lang="ts">
import { ref } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { useAuthStore } from '@/stores/auth';

const auth   = useAuthStore();
const router = useRouter();
const route  = useRoute();

const username = ref('');
const password = ref('');
const error    = ref('');
const loading  = ref(false);

async function submit() {
  error.value   = '';
  loading.value = true;
  try {
    await auth.login(username.value, password.value);
    const redirect = (route.query.redirect as string) || '/';
    router.push(redirect);
  } catch (e: any) {
    error.value = e.message || 'Login failed';
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <div class="login-bg min-h-screen flex items-center justify-center p-4">

    <!-- Background decoration -->
    <div class="login-orb login-orb-1"></div>
    <div class="login-orb login-orb-2"></div>

    <div class="w-full max-w-xs relative z-10 anim-slide-up">

      <!-- Brand header -->
      <div class="text-center mb-10">
        <img src="/sonium-logo.png" alt="Sonium" class="h-16 w-auto mx-auto object-contain mb-5 opacity-90" />
        <h1 class="login-brand">SONIUM</h1>
        <p class="login-tagline">Multiroom Audio Server</p>
      </div>

      <!-- Card -->
      <div class="login-card">
        <div class="space-y-4">

          <div>
            <label class="block section-label mb-2">Username</label>
            <input
              v-model="username"
              type="text"
              autocomplete="username"
              placeholder="admin"
              @keyup.enter="submit"
              class="field"
            />
          </div>

          <div>
            <label class="block section-label mb-2">Password</label>
            <input
              v-model="password"
              type="password"
              autocomplete="current-password"
              @keyup.enter="submit"
              class="field"
            />
          </div>

          <div v-if="error" class="flex items-center gap-2 px-3 py-2.5 rounded-xl text-xs" style="background: var(--red-dim); color: var(--red); border: 1px solid var(--red-border);">
            <span class="mdi mdi-alert-circle shrink-0"></span>
            {{ error }}
          </div>

          <button
            @click="submit"
            :disabled="loading || !username || !password"
            class="btn-primary w-full justify-center py-2.5 mt-1"
          >
            <span v-if="loading" class="mdi mdi-loading spin"></span>
            {{ loading ? 'Signing in…' : 'Sign in' }}
          </button>
        </div>
      </div>

      <p class="text-center mt-6" style="font-size: 11px; color: var(--text-muted); letter-spacing: 0.04em;">
        Secure access · Sonium Audio
      </p>
    </div>
  </div>
</template>

<style scoped>
.login-bg {
  background: var(--bg-base);
  overflow: hidden;
  position: relative;
}

/* Subtle dot-grid overlay */
.login-bg::before {
  content: '';
  position: absolute;
  inset: 0;
  background-image:
    radial-gradient(circle, rgba(148, 163, 184, 0.04) 1px, transparent 1px);
  background-size: 28px 28px;
  pointer-events: none;
}

.login-orb {
  position: absolute;
  border-radius: 50%;
  filter: blur(80px);
  pointer-events: none;
}
.login-orb-1 {
  width: 360px;
  height: 360px;
  background: radial-gradient(circle, rgba(56, 189, 248, 0.07) 0%, transparent 70%);
  top: -80px;
  left: -80px;
}
.login-orb-2 {
  width: 280px;
  height: 280px;
  background: radial-gradient(circle, rgba(14, 165, 233, 0.05) 0%, transparent 70%);
  bottom: -60px;
  right: -60px;
}

.login-brand {
  font-family: var(--font-display);
  font-size: 26px;
  font-weight: 800;
  letter-spacing: 0.22em;
  color: var(--text-primary);
  margin-bottom: 6px;
}

.login-tagline {
  font-size: 11px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.login-card {
  background: var(--bg-elevated);
  border: 1px solid var(--border-mid);
  border-radius: 16px;
  padding: 28px;
  box-shadow:
    0 0 0 1px rgba(56, 189, 248, 0.03),
    0 24px 60px rgba(0, 0, 0, 0.5),
    0 4px 16px rgba(0, 0, 0, 0.3);
}
</style>
