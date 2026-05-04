<script setup lang="ts">
import { computed } from 'vue';
interface Props {
  status: 'good' | 'fair' | 'poor' | 'unknown';
  issueCount?: number;
  totalCount?: number;
}
const props = withDefaults(defineProps<Props>(), { issueCount: 0, totalCount: 0 });

const cfg = computed(() => {
  switch (props.status) {
    case 'good': return { color: '#10b981', glow: 'rgba(16,185,129,0.4)', label: 'Sync OK', icon: 'mdi-check-circle' };
    case 'fair': return { color: '#f59e0b', glow: 'rgba(245,158,11,0.4)', label: `Fair (${props.issueCount}/${props.totalCount})`, icon: 'mdi-alert-circle' };
    case 'poor': return { color: '#f43f5e', glow: 'rgba(244,63,94,0.4)', label: `Poor (${props.issueCount}/${props.totalCount})`, icon: 'mdi-close-circle' };
    default: return { color: '#475569', glow: 'transparent', label: 'Unknown', icon: 'mdi-help-circle' };
  }
});
</script>

<template>
  <div class="inline-flex items-center gap-2 px-3 py-1.5 rounded-full bg-white/[0.03] border border-white/[0.06]" :title="cfg.label">
    <span class="w-2 h-2 rounded-full" :style="`background: ${cfg.color}; box-shadow: 0 0 6px ${cfg.glow};`"></span>
    <span class="text-[11px] font-semibold text-slate-400 whitespace-nowrap">{{ cfg.label }}</span>
  </div>
</template>
