<script setup lang="ts">
import type { ContentBlock } from '@/types'
import type { ToolResultAttachment } from '@/engines/types'
import BlockImage from './blocks/BlockImage.vue'
import EngineAssetImage from './engine/EngineAssetImage.vue'

withDefaults(defineProps<{
  images?: Array<Extract<ContentBlock, { type: 'image' }>>
  attachments?: ToolResultAttachment[]
  recordUuid?: string | null
}>(), {
  images: () => [],
  attachments: () => [],
  recordUuid: null,
})
</script>

<template>
  <div class="tool-result-images">
    <div
      v-for="(image, index) in images"
      :key="`inline:${index}`"
      class="tool-result-image-cell"
    >
      <BlockImage :block="image" :record-uuid="recordUuid" />
    </div>
    <div
      v-for="attachment in attachments"
      :key="attachment.asset.nativeId"
      class="tool-result-image-cell"
    >
      <EngineAssetImage :attachment="attachment" auto-load compact />
    </div>
  </div>
</template>

<style scoped>
.tool-result-images {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-items: flex-start;
  gap: 7px;
  margin: 3px 0 7px 18px;
  border: 1px solid color-mix(in srgb, var(--border) 82%, transparent);
  border-radius: 6px;
  padding: 7px;
  background: color-mix(in srgb, var(--card) 72%, transparent);
}
.tool-result-image-cell {
  display: flex;
  max-width: 100%;
  flex: 0 1 240px;
  align-items: flex-start;
}
.tool-result-image-cell :deep(.block-image-thumb),
.tool-result-image-cell :deep(.engine-asset-image-content) {
  width: 240px;
  max-width: 100%;
  height: 160px;
  max-height: 160px;
  object-fit: contain;
  background: var(--muted);
}
.tool-result-image-cell :deep(.block-image-loading) {
  min-width: min(160px, 100%);
  min-height: 120px;
}
.tool-result-image-cell :deep(.engine-asset-image) {
  width: 100%;
  margin: 0;
}
.tool-result-image-cell :deep(.engine-asset-image-open) {
  width: 100%;
}
.tool-result-image-cell :deep(.engine-asset-image-load) {
  min-height: 32px;
  max-width: 100%;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 0 8px;
  white-space: nowrap;
  text-overflow: ellipsis;
}
</style>
