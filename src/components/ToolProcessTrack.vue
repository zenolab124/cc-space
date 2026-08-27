<script setup lang="ts">
import { computed } from 'vue'
import type { ContentBlock } from '@/types'
import {
  isOrchestrationTool,
  isToolUseBlock,
  type ToolUseBlock,
} from '@/utils/toolDisplay'
import ToolImageStack from './ToolImageStack.vue'
import ToolProcessGroup from './ToolProcessGroup.vue'

const props = defineProps<{
  blocks: ContentBlock[]
  tools: ToolUseBlock[]
  streaming?: boolean
}>()

const orchestrationTools = computed(() => props.tools.filter(isOrchestrationTool))
const orchestrationIds = computed(() => new Set(orchestrationTools.value.map(tool => tool.id)))
const regularTools = computed(() => props.tools.filter(tool => !isOrchestrationTool(tool)))
const regularBlocks = computed(() => props.blocks.filter(block => {
  if (isToolUseBlock(block)) return !isOrchestrationTool(block)
  if (block.type === 'tool_result' && typeof block.tool_use_id === 'string') {
    return !orchestrationIds.value.has(block.tool_use_id)
  }
  return true
}))
</script>

<template>
  <div class="tool-process-track">
    <div class="tool-process-track-main">
      <ToolProcessGroup
        v-if="regularTools.length"
        :blocks="regularBlocks"
        :tools="regularTools"
        :streaming="streaming"
        :show-images="false"
      />
      <ToolProcessGroup
        v-if="orchestrationTools.length"
        :blocks="[]"
        :tools="orchestrationTools"
        :streaming="streaming"
        latest-only
        titles-only
        :show-images="false"
      />
    </div>
    <ToolImageStack :tools="tools" />
  </div>
</template>

<style scoped>
.tool-process-track {
  position: relative;
  min-width: 0;
  container-type: inline-size;
}
.tool-process-track-main {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 1px;
}
.tool-process-track:has(> .tool-image-stack) .tool-process-track-main {
  padding-right: 164px;
}
@container (max-width: 420px) {
  .tool-process-track:has(> .tool-image-stack) .tool-process-track-main {
    padding-right: 112px;
  }
}
</style>
