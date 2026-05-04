import { createRouter, createWebHistory } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import AppLayout from '@/layouts/AppLayout.vue';

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
      meta: { public: true, title: 'Sign In' },
    },
    {
      path: '/change-password',
      name: 'change-password',
      component: () => import('@/views/PasswordChangeView.vue'),
      meta: { requiresAuth: true, title: 'Change Password' },
    },
    {
      path: '/',
      component: AppLayout,
      meta: { requiresAuth: true },
      children: [
        {
          path: '',
          name: 'dashboard',
          component: () => import('@/views/HomeView.vue'),
          meta: { title: 'Dashboard' },
        },
        {
          path: 'sync',
          name: 'sync',
          component: () => import('@/views/SyncMonitorView.vue'),
          meta: { title: 'Sync Monitor' },
        },
        {
          path: 'matrix',
          name: 'matrix',
          component: () => import('@/views/MatrixView.vue'),
          meta: { title: 'Audio Matrix' },
        },
      ],
    },
    {
      path: '/admin',
      component: AppLayout,
      meta: { requiresAuth: true, requiresAdmin: true },
      children: [
        { path: '', redirect: { name: 'admin-dashboard' } },
        { path: 'dashboard', name: 'admin-dashboard', component: () => import('@/views/admin/DashboardTab.vue'), meta: { title: 'Admin · Overview' } },
        { path: 'streams',   name: 'admin-streams',   component: () => import('@/views/admin/StreamsTab.vue'),   meta: { title: 'Admin · Streams' } },
        { path: 'groups',    name: 'admin-groups',    component: () => import('@/views/admin/GroupsTab.vue'),    meta: { title: 'Admin · Groups' } },
        { path: 'clients',   name: 'admin-clients',   component: () => import('@/views/admin/ClientsTab.vue'),   meta: { title: 'Admin · Clients' } },
        { path: 'health',    name: 'admin-health',    component: () => import('@/views/admin/HealthTab.vue'),    meta: { title: 'Admin · Health' } },
        { path: 'system',    name: 'admin-system',    component: () => import('@/views/admin/SystemTab.vue'),    meta: { title: 'Admin · System' } },
        { path: 'config',    name: 'admin-config',    component: () => import('@/views/admin/ConfigTab.vue'),    meta: { title: 'Admin · Config' } },
        { path: 'users',     name: 'admin-users',     component: () => import('@/views/admin/UsersTab.vue'),     meta: { title: 'Admin · Users' } },
      ],
    },
    {
      path: '/:pathMatch(.*)*',
      name: 'not-found',
      component: () => import('@/views/NotFoundView.vue'),
      meta: { title: 'Not Found' },
    },
  ],
});

router.beforeEach(async (to) => {
  const auth = useAuthStore();

  // Update document title
  if (to.meta?.title) {
    document.title = `${to.meta.title} — Sonium`;
  } else {
    document.title = 'Sonium';
  }

  if (to.meta.public) return true;
  if (!auth.isAuthenticated) return { name: 'login', query: { redirect: to.fullPath } };
  if (!(await auth.validateSession())) return { name: 'login', query: { redirect: to.fullPath } };

  if (auth.user?.must_change_password && to.name !== 'change-password') {
    return { name: 'change-password' };
  }
  if (!auth.user?.must_change_password && to.name === 'change-password') {
    return { name: 'dashboard' };
  }

  if (to.meta.requiresAdmin && auth.user?.role !== 'admin') {
    return { name: 'dashboard' };
  }
  return true;
});

export { router };
