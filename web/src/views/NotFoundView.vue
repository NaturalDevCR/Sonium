<script setup lang="ts">
import { useRouter } from 'vue-router';
const router = useRouter();
</script>

<template>
  <div class="flex flex-col items-center justify-center py-24 animate-fade-up">
    <div class="relative mb-6">
      <div class="w-20 h-20 rounded-2xl bg-gradient-to-br from-cyan-500/10 to-violet-500/10 border border-white/[0.08] flex items-center justify-center">
        <span class="mdi mdi-magnify-close text-3xl text-slate-600"></span>
      </div>
      <div class="absolute -inset-3 bg-cyan-500/5 blur-2xl rounded-full"></div>
    </div>
    <h1 class="text-xl font-display font-bold text-white mb-1">Page not found</h1>
    <p class="text-sm text-slate-500 mb-6">The page you are looking for doesn't exist.</p>
    <button @click="router.push('/')" class="btn-gradient">
      <span class="mdi mdi-arrow-left"></span>
      Back to Dashboard
    </button>
  </div>
</template>
