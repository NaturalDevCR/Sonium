<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useAuthStore } from '@/stores/auth';
import { api, type UserView } from '@/lib/api';

const auth = useAuthStore();

const users   = ref<UserView[]>([]);
const loading = ref(true);
const error   = ref<string | null>(null);

async function load() {
  loading.value = true;
  error.value   = null;
  try {
    users.value = await api.users();
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

onMounted(load);

// ── Add user ──────────────────────────────────────────────────────────────
const showAdd   = ref(false);
const addUser   = ref({ username: '', password: '', role: 'viewer' as UserView['role'] });
const addError  = ref('');
const addBusy   = ref(false);

async function submitAdd() {
  if (!addUser.value.username || !addUser.value.password) return;
  addBusy.value  = true;
  addError.value = '';
  try {
    await api.createUser(addUser.value.username, addUser.value.password, addUser.value.role);
    showAdd.value = false;
    addUser.value = { username: '', password: '', role: 'viewer' };
    await load();
  } catch (e) {
    addError.value = String(e);
  } finally {
    addBusy.value = false;
  }
}

// ── Edit user ─────────────────────────────────────────────────────────────
const editing    = ref<UserView | null>(null);
const editRole   = ref<UserView['role']>('viewer');
const editPass   = ref('');
const editError  = ref('');
const editBusy   = ref(false);

function startEdit(u: UserView) {
  editing.value  = u;
  editRole.value = u.role;
  editPass.value = '';
  editError.value = '';
}

async function submitEdit() {
  if (!editing.value) return;
  editBusy.value  = true;
  editError.value = '';
  try {
    const data: Record<string, string> = { role: editRole.value };
    if (editPass.value) data.password = editPass.value;
    await api.updateUser(editing.value.id, data);
    editing.value = null;
    await load();
  } catch (e) {
    editError.value = String(e);
  } finally {
    editBusy.value = false;
  }
}

// ── Delete user ───────────────────────────────────────────────────────────
const confirmDelete = ref<UserView | null>(null);
const deleteBusy    = ref(false);

async function deleteUser() {
  if (!confirmDelete.value) return;
  deleteBusy.value = true;
  try {
    await api.deleteUser(confirmDelete.value.id);
    confirmDelete.value = null;
    await load();
  } finally {
    deleteBusy.value = false;
  }
}

// ── Role badge styling ────────────────────────────────────────────────────
const roleCls: Record<UserView['role'], string> = {
  admin:    'bg-blue-900/40 text-blue-400 border border-blue-800/40',
  operator: 'bg-purple-900/40 text-purple-400 border border-purple-800/40',
  viewer:   'bg-slate-800/60 text-slate-400 border border-slate-700/40',
};
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <h1 class="text-xl font-bold text-white">Users</h1>
      <button
        v-if="auth.isAdmin"
        @click="showAdd = !showAdd"
        class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-sm font-medium transition-colors"
      >
        <span class="mdi mdi-plus"></span>
        Add user
      </button>
    </div>

    <!-- Add user panel -->
    <Transition name="slide">
      <div v-if="showAdd && auth.isAdmin" class="card p-5 space-y-4">
        <h2 class="font-semibold text-white">New user</h2>
        <div class="grid sm:grid-cols-3 gap-4">
          <div>
            <label class="block text-sm text-slate-400 mb-1">Username</label>
            <input v-model="addUser.username" type="text" placeholder="alice"
              class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-white text-sm
                     focus:outline-none focus:ring-2 focus:ring-blue-500" />
          </div>
          <div>
            <label class="block text-sm text-slate-400 mb-1">Password</label>
            <input v-model="addUser.password" type="password" placeholder="••••••••"
              class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-white text-sm
                     focus:outline-none focus:ring-2 focus:ring-blue-500" />
          </div>
          <div>
            <label class="block text-sm text-slate-400 mb-1">Role</label>
            <select v-model="addUser.role"
              class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-white text-sm
                     focus:outline-none focus:ring-2 focus:ring-blue-500">
              <option value="admin">Admin</option>
              <option value="operator">Operator</option>
              <option value="viewer">Viewer</option>
            </select>
          </div>
        </div>
        <p v-if="addError" class="text-sm text-red-400">{{ addError }}</p>
        <div class="flex gap-3">
          <button @click="showAdd = false"
            class="flex-1 py-2 rounded-lg border border-slate-700 text-slate-300 hover:bg-slate-800 text-sm transition-colors">
            Cancel
          </button>
          <button @click="submitAdd" :disabled="!addUser.username || !addUser.password || addBusy"
            class="flex-1 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-sm font-semibold
                   disabled:opacity-50 transition-colors">
            {{ addBusy ? 'Creating…' : 'Create' }}
          </button>
        </div>
      </div>
    </Transition>

    <!-- Users table -->
    <div v-if="loading" class="card p-4 animate-pulse space-y-3">
      <div v-for="i in 3" :key="i" class="h-10 bg-slate-800 rounded-lg"></div>
    </div>

    <div v-else-if="error" class="card p-4 text-red-400 text-sm">{{ error }}</div>

    <div v-else class="card divide-y divide-slate-800">
      <div v-if="users.length === 0" class="p-6 text-center text-slate-600">No users found.</div>

      <div v-for="u in users" :key="u.id" class="flex items-center justify-between px-4 py-3 gap-3">
        <div class="flex items-center gap-3 min-w-0">
          <span class="mdi mdi-account-circle text-slate-500 text-2xl shrink-0"></span>
          <div class="min-w-0">
            <p class="font-medium text-white">
              {{ u.username }}
              <span v-if="u.id === auth.user?.id" class="text-xs text-slate-500 ml-1">(you)</span>
            </p>
          </div>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          <span class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium capitalize" :class="roleCls[u.role]">
            {{ u.role }}
          </span>
          <button
            v-if="auth.isAdmin"
            @click="startEdit(u)"
            class="p-1 text-slate-500 hover:text-blue-400 transition-colors"
            title="Edit"
          >
            <span class="mdi mdi-pencil-outline text-lg"></span>
          </button>
          <button
            v-if="auth.isAdmin && u.id !== auth.user?.id"
            @click="confirmDelete = u"
            class="p-1 text-slate-600 hover:text-red-400 transition-colors"
            title="Delete"
          >
            <span class="mdi mdi-delete-outline text-lg"></span>
          </button>
        </div>
      </div>
    </div>

    <!-- Role descriptions -->
    <div class="card p-4 space-y-2">
      <h3 class="text-sm font-semibold text-slate-400">Role permissions</h3>
      <div class="grid sm:grid-cols-3 gap-3 text-xs text-slate-500">
        <div>
          <span class="text-blue-400 font-semibold">Admin</span> — Full access: manage users, edit config, manage all groups and clients.
        </div>
        <div>
          <span class="text-purple-400 font-semibold">Operator</span> — Control groups, clients, streams, and volume. Cannot edit config or users.
        </div>
        <div>
          <span class="text-slate-400 font-semibold">Viewer</span> — Read-only access. Can see status but cannot make changes.
        </div>
      </div>
    </div>

    <!-- Edit modal -->
    <Teleport to="body">
      <Transition name="fade">
        <div
          v-if="editing"
          class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm"
          @click.self="editing = null"
        >
          <div class="card w-full max-w-sm p-5 space-y-4">
            <h3 class="font-bold text-white">Edit <span class="text-blue-400">{{ editing.username }}</span></h3>

            <div>
              <label class="block text-sm text-slate-400 mb-1">Role</label>
              <select v-model="editRole"
                class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-white text-sm
                       focus:outline-none focus:ring-2 focus:ring-blue-500">
                <option value="admin">Admin</option>
                <option value="operator">Operator</option>
                <option value="viewer">Viewer</option>
              </select>
            </div>

            <div>
              <label class="block text-sm text-slate-400 mb-1">New password <span class="text-slate-600">(leave blank to keep current)</span></label>
              <input v-model="editPass" type="password" placeholder="••••••••"
                class="w-full bg-slate-800 border border-slate-700 rounded-lg px-3 py-2 text-white text-sm
                       focus:outline-none focus:ring-2 focus:ring-blue-500" />
            </div>

            <p v-if="editError" class="text-sm text-red-400">{{ editError }}</p>

            <div class="flex gap-3 pt-1">
              <button @click="editing = null"
                class="flex-1 py-2 rounded-lg border border-slate-700 text-slate-300 hover:bg-slate-800 text-sm transition-colors">
                Cancel
              </button>
              <button @click="submitEdit" :disabled="editBusy"
                class="flex-1 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-sm font-semibold
                       disabled:opacity-50 transition-colors">
                {{ editBusy ? 'Saving…' : 'Save' }}
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>

    <!-- Delete confirmation modal -->
    <Teleport to="body">
      <Transition name="fade">
        <div
          v-if="confirmDelete"
          class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm"
          @click.self="confirmDelete = null"
        >
          <div class="card w-full max-w-sm p-5 space-y-4">
            <h3 class="font-bold text-white">Delete <span class="text-red-400">{{ confirmDelete.username }}</span>?</h3>
            <p class="text-sm text-slate-400">This cannot be undone.</p>
            <div class="flex gap-3">
              <button @click="confirmDelete = null"
                class="flex-1 py-2 rounded-lg border border-slate-700 text-slate-300 hover:bg-slate-800 text-sm transition-colors">
                Cancel
              </button>
              <button @click="deleteUser" :disabled="deleteBusy"
                class="flex-1 py-2 rounded-lg bg-red-600 hover:bg-red-500 text-white text-sm font-semibold
                       disabled:opacity-50 transition-colors">
                {{ deleteBusy ? 'Deleting…' : 'Delete' }}
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.slide-enter-active, .slide-leave-active { transition: all .2s ease; }
.slide-enter-from, .slide-leave-to { opacity: 0; transform: translateY(-8px); }
.fade-enter-active, .fade-leave-active { transition: opacity .2s; }
.fade-enter-from, .fade-leave-to { opacity: 0; }
</style>
