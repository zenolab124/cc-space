<script setup lang="ts">
import type { AssistantResponseMeta } from '@/utils/assistantResponse'
import AssistantMetaLine from './AssistantMetaLine.vue'

defineProps<{
  meta?: AssistantResponseMeta | null
  showFooter?: boolean
}>()
</script>

<template>
  <div class="assistant-response-frame msg-block">
    <div class="assistant-response-rail" aria-hidden="true" />
    <div class="assistant-response-column">
      <AssistantMetaLine position="header" :meta="meta" />
      <div class="assistant-response-content">
        <slot />
      </div>
      <AssistantMetaLine v-if="showFooter && meta" position="footer" :meta="meta" />
    </div>
  </div>
</template>

<style scoped>
.assistant-response-frame {
  display: flex;
  gap: 12px;
}
.assistant-response-rail {
  width: 2px;
  flex: none;
  align-self: stretch;
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--claude) 60%, transparent);
}
.assistant-response-column {
  min-width: 0;
  flex: 1;
}
.assistant-response-content {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: var(--message-block-gap);
}
</style>
