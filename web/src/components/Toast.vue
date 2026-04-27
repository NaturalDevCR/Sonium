<script setup lang="ts">
import { toasts, dismissToast } from '@/lib/toast';
</script>

<template>
  <Teleport to="body">
    <div class="toast-stack" aria-live="polite">
      <TransitionGroup name="toast">
        <div
          v-for="t in toasts"
          :key="t.id"
          class="toast-item"
          :class="`toast-${t.variant}`"
          @click="dismissToast(t.id)"
        >
          <span class="mdi text-base"
            :class="{
              'mdi-alert-circle-outline': t.variant === 'error',
              'mdi-check-circle-outline': t.variant === 'success',
              'mdi-information-outline':  t.variant === 'info',
            }"
          ></span>
          <span>{{ t.text }}</span>
          <button class="toast-close" @click.stop="dismissToast(t.id)" aria-label="Dismiss">
            <span class="mdi mdi-close text-sm"></span>
          </button>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<style scoped>
.toast-stack {
  position: fixed;
  top: 16px;
  right: 16px;
  z-index: 9999;
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-width: 360px;
  pointer-events: none;
}

.toast-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 11px 14px;
  border-radius: 10px;
  font-size: 13px;
  font-weight: 500;
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border: 1px solid transparent;
  cursor: pointer;
  pointer-events: all;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.35);
}

.toast-error {
  background: rgba(220, 38, 38, 0.18);
  border-color: rgba(220, 38, 38, 0.35);
  color: #fca5a5;
}

.toast-success {
  background: rgba(34, 197, 94, 0.18);
  border-color: rgba(34, 197, 94, 0.35);
  color: #86efac;
}

.toast-info {
  background: rgba(14, 165, 233, 0.18);
  border-color: rgba(14, 165, 233, 0.35);
  color: #7dd3fc;
}

.toast-close {
  margin-left: auto;
  background: transparent;
  border: none;
  cursor: pointer;
  color: inherit;
  opacity: 0.6;
  padding: 0;
  line-height: 1;
}
.toast-close:hover { opacity: 1; }

/* Transitions */
.toast-enter-active { transition: all 0.2s ease; }
.toast-leave-active { transition: all 0.2s ease; }
.toast-enter-from  { opacity: 0; transform: translateX(24px); }
.toast-leave-to    { opacity: 0; transform: translateX(24px); }
</style>
