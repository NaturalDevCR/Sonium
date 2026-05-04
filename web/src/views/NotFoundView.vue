<script setup lang="ts">
import { useRouter } from 'vue-router';

const router = useRouter();
</script>

<template>
  <div class="notfound-root">
    <div class="text-center">
      <span class="mdi mdi-magnify-close text-6xl block mb-4" style="color: var(--text-muted);"></span>
      <h1 class="notfound-title">Page not found</h1>
      <p class="notfound-sub">The page you are looking for doesn't exist.</p>
      <button @click="router.push('/')" class="btn-primary mt-6 justify-center">
        <span class="mdi mdi-arrow-left mr-1"></span>
        Back to Dashboard
      </button>
    </div>
  </div>
</template>

<style scoped>
.notfound-root {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 60vh;
}
.notfound-title {
  font-family: var(--font-display);
  font-size: 22px;
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: 4px;
}
.notfound-sub {
  font-size: 13px;
  color: var(--text-muted);
}
</style>
