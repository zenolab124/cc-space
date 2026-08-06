<script setup lang="ts">
import type { ContentBlock } from '@/types'

defineProps<{
  block: Extract<ContentBlock, { type: 'redacted_thinking' }>
}>()
</script>

<template>
  <div class="thinking-block mt-2 mb-1.5 text-xs text-muted-foreground flex items-center gap-1">
    <span class="i-carbon-locked w-3 h-3 shrink-0" />
    {{ $t('block.thinkingRedacted') }}
  </div>
</template>
