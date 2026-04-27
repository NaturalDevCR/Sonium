import { defineStore } from 'pinia';
import { ref, computed, watch } from 'vue';
import { api, setApiToken, setUnauthorizedHandler } from '@/lib/api';

export interface AuthUser {
  id:       string;
  username: string;
  role:     'admin' | 'operator' | 'viewer';
  must_change_password: boolean;
}

export const useAuthStore = defineStore('auth', () => {
  const token = ref<string | null>(null);
  const user  = ref<AuthUser | null>(null);

  const isAuthenticated = computed(() => token.value !== null);
  const isAdmin         = computed(() => user.value?.role === 'admin');
  const isOperator      = computed(() => user.value?.role === 'admin' || user.value?.role === 'operator');

  function authHeaders(): Record<string, string> {
    return token.value ? { Authorization: `Bearer ${token.value}` } : {};
  }

  async function login(username: string, password: string): Promise<void> {
    const r = await fetch('/api/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username, password }),
    });
    if (!r.ok) {
      const msg = await r.text();
      throw new Error(msg || 'Login failed');
    }
    const data = await r.json();
    token.value = data.token;
    user.value  = data.user;
  }

  async function validateSession(): Promise<boolean> {
    if (!token.value) return false;
    const r = await fetch('/api/auth/me', { headers: authHeaders() });
    if (r.ok) {
      user.value = await r.json();
      return true;
    }
    if (r.status === 401) { token.value = null; user.value = null; }
    return false;
  }

  async function logout() {
    try { await api.logout(); } catch { /* server may be down; clear token anyway */ }
    token.value = null;
    user.value  = null;
  }

  // Keep the api module in sync with the current token (works after hydration too)
  watch(token, (t) => setApiToken(t), { immediate: true });
  setUnauthorizedHandler(() => { token.value = null; user.value = null; });

  return { token, user, isAuthenticated, isAdmin, isOperator, authHeaders, login, validateSession, logout };
}, {
  persist: { paths: ['token', 'user'] },
});
