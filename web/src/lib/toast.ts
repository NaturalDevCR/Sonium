/**
 * Global toast notification state.
 *
 * Kept outside of `<script setup>` so it can be imported by other modules
 * without triggering the "cannot export from <script setup>" compiler error.
 */
import { ref } from 'vue';

export interface ToastMessage {
  id:      number;
  text:    string;
  variant: 'error' | 'success' | 'info';
}

export const toasts = ref<ToastMessage[]>([]);

let _next = 1;

export function showToast(text: string, variant: ToastMessage['variant'] = 'error') {
  const id = _next++;
  toasts.value.push({ id, text, variant });
  setTimeout(() => dismissToast(id), 4000);
}

export function dismissToast(id: number) {
  toasts.value = toasts.value.filter((t) => t.id !== id);
}
