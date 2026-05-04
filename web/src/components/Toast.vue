<script setup lang="ts">
import { toasts, dismissToast } from '@/lib/toast';
</script>

<template>
  <Teleport to="body">
    <div class="fixed top-4 right-4 z-[9999] flex flex-col gap-2 max-w-sm pointer-events-none" aria-live="polite">
      <TransitionGroup name="toast">
        <div
          v-for="t in toasts"
          :key="t.id"
          class="pointer-events-auto flex items-center gap-3 px-4 py-3 rounded-xl text-sm font-medium backdrop-blur-xl border shadow-lg cursor-pointer"
          :class="{
            'bg-rose-500/10 border-rose-500/20 text-rose-300': t.variant === 'error',
            'bg-emerald-500/10 border-emerald-500/20 text-emerald-300': t.variant === 'success',
            'bg-cyan-500/10 border-cyan-500/20 text-cyan-300': t.variant === 'info',
          }"
          @click="dismissToast(t.id)"
        >
          <span class="mdi"
            :class="{
              'mdi-alert-circle-outline': t.variant === 'error',
              'mdi-check-circle-outline': t.variant === 'success',
              'mdi-information-outline': t.variant === 'info',
            }"
          ></span>
          <span class="flex-1">{{ t.text }}</span>
          <button class="opacity-60 hover:opacity-100 transition-opacity" @click.stop="dismissToast(t.id)">
            <span class="mdi mdi-close text-xs"></span>
          </button>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<style scoped>
.toast-enter-active, .toast-leave-active { transition: all 0.25s ease; }
.toast-enter-from { opacity: 0; transform: translateX(20px) scale(0.95); }
.toast-leave-to { opacity: 0; transform: translateX(20px) scale(0.95); }
</style>
