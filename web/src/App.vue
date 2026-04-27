<script setup lang="ts">
import { onMounted } from 'vue';
import { RouterView } from 'vue-router';
import Toast from '@/components/Toast.vue';
import { showToast } from '@/lib/toast';
import { setErrorHandler } from '@/lib/api';

onMounted(() => {
  setErrorHandler((msg) => showToast(msg, 'error'));
});
</script>

<template>
  <RouterView />
  <Toast />
</template>
