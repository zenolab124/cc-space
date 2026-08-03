<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { formatTokens, shortModel } from '@/types'
import type { AssistantResponseMeta } from '@/utils/assistantResponse'

const props = defineProps<{
  meta?: AssistantResponseMeta | null
  position: 'header' | 'footer'
}>()

const { t } = useI18n()

const modelLabel = computed(() => props.meta?.model ? shortModel(props.meta.model) : null)
const usageParts = computed(() => {
  const usage = props.meta?.usage
  if (!usage) return []
  return [
    `${formatTokens(usage.input_tokens)} in`,
    `${formatTokens(usage.cache_read_input_tokens)} cache`,
    `${formatTokens(usage.cache_creation_input_tokens)} new`,
    `${formatTokens(usage.output_tokens)} out`,
  ]
})
</script>

<template>
  <div
    class="assistant-meta-line"
    :class="position === 'header' ? 'is-header' : 'is-footer'"
    v-tooltip="position === 'footer' ? meta?.completedFull : undefined"
  >
    <span v-if="position === 'header'" class="assistant-meta-speaker">{{ t('session.claude') }}</span>
    <span v-if="position === 'footer' && meta?.completedText">{{ meta.completedText }}</span>
    <span v-if="modelLabel">{{ modelLabel }}</span>
    <span v-if="meta?.tier">{{ t('topbar.roleTier', { role: meta.tier }) }}</span>
    <span v-for="part in usageParts" :key="part">{{ part }}</span>
  </div>
</template>

<style scoped>
.assistant-meta-line {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 0;
  min-height: 16px;
  color: color-mix(in srgb, var(--muted-foreground) 72%, transparent);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  letter-spacing: 0.01em;
  line-height: 16px;
}
.assistant-meta-line > span + span::before {
  content: "\00b7";
  margin: 0 5px;
  color: color-mix(in srgb, var(--muted-foreground) 42%, transparent);
}
.assistant-meta-line.is-header {
  margin-bottom: 5px;
  color: var(--muted-foreground);
}
.assistant-meta-line.is-footer {
  margin-top: 7px;
}
.assistant-meta-speaker {
  color: var(--claude);
  font-size: 12px;
  font-weight: 600;
}
</style>
