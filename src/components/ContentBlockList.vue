<script setup lang="ts">
import { computed } from 'vue'
import type { ContentBlock } from '@/types'
import { isToolUseBlock, segmentToolBlocks } from '@/utils/toolDisplay'
import { useToolDisplayMode } from '@/composables/useToolDisplay'
import MessageBlock from './MessageBlock.vue'
import ToolProcessGroup from './ToolProcessGroup.vue'
import ToolProcessItems from './ToolProcessItems.vue'

const props = defineProps<{
  blocks: ContentBlock[]
  blockRecordUuids?: Array<string | null | undefined>
  streaming?: boolean
  recordUuid?: string | null
}>()

const { toolDisplayMode } = useToolDisplayMode()
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
  <template v-for="segment in segments" :key="segment.key">
    <MessageBlock
      v-if="segment.kind === 'block'"
      :block="segment.block"
      :streaming="streaming"
      :record-uuid="segment.recordUuids[0]"
    />
    <template v-else-if="toolDisplayMode === 'cards'">
      <template
        v-for="(block, index) in segment.blocks"
        :key="isToolUseBlock(block) ? block.id : `${block.type}:${index}`"
      >
        <div
          v-if="isToolUseBlock(block)"
          :data-tool-use-id="block.id"
        >
          <MessageBlock
            :block="block"
            :streaming="streaming"
            :record-uuid="segment.recordUuids[index]"
          />
        </div>
        <MessageBlock
          v-else
          :block="block"
          :streaming="streaming"
          :record-uuid="segment.recordUuids[index]"
        />
      </template>
    </template>
    <template v-else-if="toolDisplayMode === 'individual'">
      <ToolProcessItems
        :blocks="segment.blocks"
        :block-record-uuids="segment.recordUuids"
        :streaming="streaming"
      />
    </template>
    <ToolProcessGroup
      v-else
      :blocks="segment.blocks"
      :block-record-uuids="segment.recordUuids"
      :tools="segment.tools"
      :streaming="streaming"
    />
  </template>
</template>
