<script setup lang="ts">
import { computed } from 'vue'
import type { EngineAccent } from '@/engines/presentation'

const props = withDefaults(defineProps<{
  engineName: string
  title: string
  accent?: EngineAccent
  tags?: string[]
}>(), {
  accent: 'primary',
  tags: () => [],
})

const color = computed(() => `var(--${props.accent})`)
</script>

<template>
  <header class="session-identity-bar shrink-0 flex items-center gap-2 border-b border-border bg-card px-3 py-2">
    <span
      class="shrink-0 rounded border px-1.5 py-0.5 text-[10px] font-semibold"
      :style="{
        color,
        borderColor: `color-mix(in srgb, ${color} 22%, transparent)`,
        background: `color-mix(in srgb, ${color} 10%, transparent)`,
      }"
    >{{ engineName }}</span>
    <div class="min-w-0 flex-1 truncate text-xs font-semibold">{{ title }}</div>
    <span
      v-for="tag in tags.slice(0, 2)"
      :key="tag"
      class="max-w-24 truncate rounded bg-secondary px-1.5 py-0.5 text-[9px] text-muted-foreground"
    >{{ tag }}</span>
  </header>
</template>
