<script setup lang="ts">
import { computed } from 'vue'
import type { ContentBlock } from '@/types'
import type { ToolResultAttachment } from '@/engines/types'
import BlockImage from './blocks/BlockImage.vue'
import EngineAssetImage from './engine/EngineAssetImage.vue'

const props = withDefaults(defineProps<{
  contentId: string
  text: string
  images: Array<Extract<ContentBlock, { type: 'image' }>>
  attachments?: ToolResultAttachment[]
  recordUuid?: string | null
  expanded: boolean
  isError?: boolean
}>(), {
  attachments: () => [],
  recordUuid: null,
  isError: false,
})

const emit = defineEmits<{ (event: 'toggle', value: MouseEvent): void }>()

const imageAttachments = computed(() => props.attachments.filter(attachment => attachment.mediaType.startsWith('image/')))
const hasImages = computed(() => props.images.length > 0 || imageAttachments.value.length > 0)
const clampClass = computed(() => hasImages.value ? 'tool-result-clamp-2' : 'tool-result-clamp-3')
</script>

<template>
  <div
    :id="contentId"
    class="tool-result-preview-card"
    :class="{ 'is-expanded': expanded, 'is-error': isError }"
  >
    <div v-if="hasImages" class="tool-result-preview-images">
      <BlockImage
        v-for="(image, index) in images"
        :key="`inline:${index}`"
        :block="image"
        :record-uuid="recordUuid"
      />
      <EngineAssetImage
        v-for="attachment in imageAttachments"
        :key="attachment.asset.nativeId"
        :attachment="attachment"
        auto-load
        :compact="!expanded"
      />
    </div>

    <button
      v-if="text"
      type="button"
      class="tool-result-preview-toggle"
      :aria-expanded="expanded"
      :aria-label="$t(expanded ? 'common.collapse' : 'common.expand')"
      :title="$t(expanded ? 'common.collapse' : 'common.expand')"
      @click="emit('toggle', $event)"
    >
      <pre
        class="tool-result-preview-text"
        :class="expanded ? '' : clampClass"
      >{{ text }}</pre>
      <span
        class="tool-result-preview-cue"
        :class="expanded ? 'i-carbon-chevron-up' : 'i-carbon-chevron-down'"
        aria-hidden="true"
      />
    </button>
  </div>
</template>

<style scoped>
.tool-result-preview-card {
  min-width: 0;
  margin: 3px 0 7px 18px;
  overflow: hidden;
  border: 1px solid color-mix(in srgb, var(--border) 82%, transparent);
  border-radius: 6px;
  color: var(--muted-foreground);
  background: color-mix(in srgb, var(--card) 72%, transparent);
  box-shadow: 0 1px 0 color-mix(in srgb, var(--foreground) 4%, transparent);
  transition: border-color 120ms ease, background-color 120ms ease;
}
.tool-result-preview-card:hover,
.tool-result-preview-card:focus-within {
  border-color: color-mix(in srgb, var(--primary) 28%, var(--border));
  background: color-mix(in srgb, var(--card) 90%, transparent);
}
.tool-result-preview-card.is-error {
  border-color: color-mix(in srgb, var(--destructive) 28%, var(--border));
  color: var(--destructive);
}
.tool-result-preview-images {
  display: flex;
  min-width: 0;
  align-items: flex-start;
  gap: 5px;
  overflow-x: auto;
  overflow-y: hidden;
  padding: 5px 7px 0;
}
.tool-result-preview-card:not(.is-expanded) .tool-result-preview-images {
  height: 38px;
}
.tool-result-preview-card:not(.is-expanded) .tool-result-preview-images :deep(.block-image-thumb),
.tool-result-preview-card:not(.is-expanded) .tool-result-preview-images :deep(.engine-asset-image-content) {
  width: 44px;
  min-width: 44px;
  height: 28px;
  min-height: 28px;
  border-radius: 4px;
  object-fit: cover;
}
.tool-result-preview-card:not(.is-expanded) .tool-result-preview-images :deep(.block-image-loading) {
  min-width: 44px;
  min-height: 28px;
}
.tool-result-preview-card:not(.is-expanded) .tool-result-preview-images :deep(.engine-asset-image) {
  flex: none;
  margin: 0;
}
.tool-result-preview-card:not(.is-expanded) .tool-result-preview-images :deep(.engine-asset-image-load) {
  height: 28px;
  max-width: 132px;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 0 7px;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.tool-result-preview-card.is-expanded .tool-result-preview-images {
  flex-wrap: wrap;
  padding: 7px 9px 0;
}
.tool-result-preview-card.is-expanded .tool-result-preview-images :deep(.engine-asset-image) {
  margin: 0;
}
.tool-result-preview-toggle {
  position: relative;
  display: block;
  width: 100%;
  border: 0;
  padding: 6px 28px 7px 9px;
  color: inherit;
  background: transparent;
  text-align: left;
  cursor: pointer;
}
.tool-result-preview-toggle:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: -2px;
}
.tool-result-preview-text {
  min-width: 0;
  margin: 0;
  overflow-wrap: anywhere;
  font-family: inherit;
  font-size: 11.5px;
  line-height: 1.5;
  white-space: pre-wrap;
}
.tool-result-clamp-2,
.tool-result-clamp-3 {
  display: -webkit-box;
  overflow: hidden;
  -webkit-box-orient: vertical;
}
.tool-result-clamp-2 { -webkit-line-clamp: 2; }
.tool-result-clamp-3 { -webkit-line-clamp: 3; }
.tool-result-preview-cue {
  position: absolute;
  right: 8px;
  bottom: 8px;
  width: 11px;
  height: 11px;
  color: color-mix(in srgb, currentColor 58%, transparent);
}
</style>
