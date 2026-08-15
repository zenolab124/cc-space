<script setup lang="ts">
import { computed } from 'vue'
import type { ContentBlock } from '@/types'
import {
  isOrchestrationTool,
  isOrchestrationToolSegment,
  isToolUseBlock,
  segmentToolBlocks,
} from '@/utils/toolDisplay'
import { useToolDisplayMode, type ToolDisplayMode } from '@/composables/useToolDisplay'
import MessageBlock from './MessageBlock.vue'
import ToolProcessGroup from './ToolProcessGroup.vue'
import ToolProcessItems from './ToolProcessItems.vue'

const props = defineProps<{
  blocks: ContentBlock[]
  blockRecordUuids?: Array<string | null | undefined>
  streaming?: boolean
  recordUuid?: string | null
  /** 调用方已按响应模型解析出的展示方式；缺省时沿用全局默认。 */
  displayMode?: ToolDisplayMode
}>()

const { toolDisplayMode: defaultToolDisplayMode } = useToolDisplayMode()
const toolDisplayMode = computed(() => props.displayMode ?? defaultToolDisplayMode.value)
const segments = computed(() => {
  let blockIndex = 0
  return segmentToolBlocks(props.blocks).map(segment => {
    if (segment.kind === 'block') {
      const recordUuids = [props.blockRecordUuids?.[blockIndex] ?? props.recordUuid]
      blockIndex += 1
      return { ...segment, recordUuids }
    }
    const recordUuids = segment.blocks.map(() => props.blockRecordUuids?.[blockIndex++] ?? props.recordUuid)
    return { ...segment, recordUuids }
  })
})
</script>

<template>
  <div class="content-block-list">
    <template v-for="segment in segments" :key="segment.key">
      <div v-if="segment.kind === 'block'" class="content-segment">
        <MessageBlock
          :block="segment.block"
          :streaming="streaming"
          :record-uuid="segment.recordUuids[0]"
        />
      </div>
      <div v-else-if="isOrchestrationToolSegment(segment.tools)" class="content-segment">
        <ToolProcessItems
          :blocks="segment.blocks"
          :block-record-uuids="segment.recordUuids"
          :streaming="streaming"
        />
      </div>
      <div v-else-if="toolDisplayMode === 'cards'" class="content-segment content-tool-cards">
        <template
          v-for="(block, index) in segment.blocks"
          :key="isToolUseBlock(block) ? block.id : `${block.type}:${index}`"
        >
          <div
            v-if="isToolUseBlock(block) && isOrchestrationTool(block)"
            class="content-tool-card"
            :data-tool-use-id="block.id"
          >
            <ToolProcessItems
              :blocks="[block]"
              :block-record-uuids="[segment.recordUuids[index]]"
              :streaming="streaming"
              nested
            />
          </div>
          <div
            v-else-if="isToolUseBlock(block)"
            class="content-tool-card"
            :data-tool-use-id="block.id"
          >
            <MessageBlock
              :block="block"
              :streaming="streaming"
              :record-uuid="segment.recordUuids[index]"
            />
          </div>
          <div v-else class="content-tool-card">
            <MessageBlock
              :block="block"
              :streaming="streaming"
              :record-uuid="segment.recordUuids[index]"
            />
          </div>
        </template>
      </div>
      <div v-else-if="toolDisplayMode === 'individual'" class="content-segment">
        <ToolProcessItems
          :blocks="segment.blocks"
          :block-record-uuids="segment.recordUuids"
          :streaming="streaming"
        />
      </div>
      <div v-else class="content-segment">
        <ToolProcessGroup
          :blocks="segment.blocks"
          :block-record-uuids="segment.recordUuids"
          :tools="segment.tools"
          :streaming="streaming"
        />
      </div>
    </template>
  </div>
</template>

<style scoped>
.content-block-list,
.content-tool-cards {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: var(--message-block-gap);
}
.content-segment,
.content-tool-card { min-width: 0; }

/* 外部节奏由内容流统一管理，避免各块根 margin 与 gap 重复叠加。 */
.content-segment > :deep(*),
.content-tool-card > :deep(*) {
  margin-top: 0;
  margin-bottom: 0;
}
</style>
