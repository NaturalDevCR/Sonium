<script setup lang="ts">
import type { Stream } from '@/lib/api';

const props = defineProps<{ stream: Stream | undefined }>();
</script>

<template>
  <span v-if="props.stream" class="badge" :class="props.stream.status">
    {{ props.stream.codec.toUpperCase() }} · {{ props.stream.status }}
  </span>
  <span v-else class="badge idle">no stream</span>
</template>

<style scoped>
.badge {
  font-size: 0.7rem;
  font-weight: 600;
  padding: 2px 8px;
  border-radius: 999px;
  letter-spacing: 0.03em;
  text-transform: uppercase;
}
.playing { background: #1a6640; color: #6effa8; }
.idle    { background: #2a2a3a; color: #888;    }
.error   { background: #5a1a1a; color: #ff8080; }
</style>
