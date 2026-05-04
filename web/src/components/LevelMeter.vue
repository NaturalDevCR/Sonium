<script setup lang="ts">
const props = defineProps<{ rmsDb: number }>();

const bars = 8;
const level = Math.max(0, Math.min(1, (props.rmsDb + 90) / 90));
const activeBars = Math.round(level * bars);
</script>

<template>
  <div class="flex items-end gap-[3px] h-4">
    <div
      v-for="i in bars"
      :key="i"
      class="w-[3px] rounded-full transition-all duration-150"
      :class="i <= activeBars
        ? 'bg-gradient-to-t from-cyan-400 to-violet-400'
        : 'bg-white/[0.06]'"
      :style="{ height: `${20 + i * 12}%` }"
    ></div>
  </div>
</template>
