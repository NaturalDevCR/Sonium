<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue';
import { useServerStore } from '@/stores/server';

const server = useServerStore();

onMounted(() => server.init());
onUnmounted(() => server.stopLiveUpdates());
</script>

<template>
  <div class="admin-root">
    <RouterView />
  </div>
</template>

<style scoped>
.admin-root {
  max-width: 1000px;
  margin: 0 auto;
}
</style>
