import { createRouter, createWebHistory } from 'vue-router';
import { useAuthStore } from '@/stores/auth';

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
      meta: { public: true },
    },
    {
      path: '/',
      name: 'control',
      component: () => import('@/views/ControlView.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/admin',
      name: 'admin',
      component: () => import('@/views/AdminView.vue'),
      meta: { requiresAuth: true, requiresAdmin: true },
      children: [
        { path: '',          redirect: { name: 'admin-dashboard' } },
        { path: 'dashboard', name: 'admin-dashboard', component: () => import('@/views/admin/DashboardTab.vue') },
        { path: 'streams',   name: 'admin-streams',   component: () => import('@/views/admin/StreamsTab.vue')   },
        { path: 'groups',    name: 'admin-groups',    component: () => import('@/views/admin/GroupsTab.vue')    },
        { path: 'clients',   name: 'admin-clients',   component: () => import('@/views/admin/ClientsTab.vue')   },
        { path: 'system',    name: 'admin-system',    component: () => import('@/views/admin/SystemTab.vue')    },
        { path: 'config',    name: 'admin-config',    component: () => import('@/views/admin/ConfigTab.vue')    },
        { path: 'users',     name: 'admin-users',     component: () => import('@/views/admin/UsersTab.vue')     },
      ],
    },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
});

router.beforeEach(async (to) => {
  const auth = useAuthStore();
  if (to.meta.public) return true;
  if (!auth.isAuthenticated) return { name: 'login', query: { redirect: to.fullPath } };
  if (!(await auth.validateSession())) return { name: 'login', query: { redirect: to.fullPath } };
  if (to.meta.requiresAdmin && auth.user?.role !== 'admin') return { name: 'control' };
  return true;
});

export { router };
