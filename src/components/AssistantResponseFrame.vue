<script setup lang="ts">
import type { EngineAccent } from '@/engines/presentation'
import type { AssistantResponseMeta } from '@/utils/assistantResponse'
import AssistantMetaLine from './AssistantMetaLine.vue'

defineProps<{
  meta?: AssistantResponseMeta | null
  showFooter?: boolean
  speaker?: string
  accent?: EngineAccent
}>()
</script>

<template>
  <div
    class="assistant-response-frame msg-block"
    :class="accent === 'primary' ? 'is-primary' : accent === 'codex' ? 'is-codex' : undefined"
  >
    <div class="assistant-response-rail" aria-hidden="true" />
    <div class="assistant-response-column">
      <AssistantMetaLine position="header" :meta="meta" :speaker="speaker" />
      <div class="assistant-response-content">
        <slot />
      </div>
      <AssistantMetaLine v-if="showFooter && meta" position="footer" :meta="meta" />
    </div>
  </div>
</template>

<style scoped>
.assistant-response-frame {
  --assistant-accent: var(--claude);
  display: flex;
  gap: 12px;
}
.assistant-response-frame.is-primary {
  --assistant-accent: var(--primary);
}
.assistant-response-frame.is-codex {
  --assistant-accent: var(--codex);
}
.assistant-response-rail {
  width: 2px;
  flex: none;
  align-self: stretch;
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--assistant-accent) 60%, transparent);
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
