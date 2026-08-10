<script setup lang="ts">
import { computed } from 'vue'
import type { ContentBlock } from '@/types'
import type { EngineSegment } from '@/engines/types'
import MessageBlock from '@/components/MessageBlock.vue'
import EngineAssetImage from './EngineAssetImage.vue'

const props = withDefaults(defineProps<{
  segment: EngineSegment
  compact?: boolean
  streaming?: boolean
}>(), {
  compact: false,
})
const contentBlock = computed<ContentBlock | null>(() => {
  const segment = props.segment
  if (segment.kind === 'text') return { type: 'text', text: segment.text }
  if (segment.kind === 'reasoning') {
    return segment.visibility === 'redacted'
      ? { type: 'redacted_thinking' }
      : { type: 'thinking', thinking: segment.text }
  }
  if (segment.kind === 'unknown' && segment.summary?.trim()) {
    return { type: segment.typeName, summary: segment.summary }
  }
  return null
})

const attachment = computed(() => props.segment.kind === 'attachment' ? props.segment : null)
</script>

<template>
  <div v-if="contentBlock" :class="compact && 'engine-compact-text'">
    <MessageBlock :block="contentBlock" :streaming="streaming" />
  </div>

  <EngineAssetImage v-else-if="attachment" :attachment="attachment" />

</template>

<style scoped>
.engine-compact-text {
  color: var(--muted-foreground);
}
.engine-compact-text :deep(.prose-msg.message-prose) {
  font-size: 12px;
  line-height: 1.6;
}
</style>
