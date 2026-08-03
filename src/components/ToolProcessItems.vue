<script setup lang="ts">
import type { ContentBlock } from '@/types'
import { isToolUseBlock, type ToolUseBlock } from '@/utils/toolDisplay'
import MessageBlock from './MessageBlock.vue'
import ToolProcessItem from './ToolProcessItem.vue'

const props = defineProps<{
  blocks: ContentBlock[]
  blockRecordUuids?: Array<string | null | undefined>
  streaming?: boolean
}>()

function toolOf(block: ContentBlock): ToolUseBlock | null {
  return isToolUseBlock(block) ? block : null
}
</script>

<template>
  <div class="tool-process-list">
    <template v-for="(block, index) in blocks" :key="isToolUseBlock(block) ? block.id : `${block.type}:${index}`">
      <ToolProcessItem
        v-if="toolOf(block)"
        :tool="toolOf(block)!"
        :streaming="streaming"
      />
      <MessageBlock
        v-else
        :block="block"
        :streaming="streaming"
        :record-uuid="blockRecordUuids?.[index]"
      />
    </template>
  </div>
</template>

<style scoped>
.tool-process-list { min-width: 0; }
.tool-process-list > :deep(.thinking-block) {
  margin-top: var(--message-block-gap);
  margin-bottom: var(--message-block-gap);
}
</style>
