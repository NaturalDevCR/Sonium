<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue';
import { useServerStore } from '@/stores/server';

const server = useServerStore();
onMounted(() => server.init());
onUnmounted(() => server.stopLiveUpdates());
</script>

<template>
  <div class="max-w-5xl mx-auto">
    <RouterView />
  </div>
</template>
