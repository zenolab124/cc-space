<script setup lang="ts">
import { computed, inject, type ComputedRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ContentBlock } from '@/types'
import type { ToolResultAttachment } from '@/engines/types'
import type { ToolResultData } from '@/utils/toolPair'
import type { ToolUseBlock } from '@/utils/toolDisplay'
import { TOOL_EXECUTION_CONTEXT } from '@/composables/useToolDisplay'
import BlockImage from './blocks/BlockImage.vue'
import EngineAssetImage from './engine/EngineAssetImage.vue'

const props = defineProps<{
  tools: ToolUseBlock[]
}>()

const { t } = useI18n()
const context = inject(TOOL_EXECUTION_CONTEXT, null)
const legacyResults = inject<ComputedRef<Map<string, ToolResultData>>>('toolResultMap')

type ImageEntry =
  | {
      kind: 'inline'
      key: string
      image: Extract<ContentBlock, { type: 'image' }>
      recordUuid: string | null
    }
  | {
      kind: 'attachment'
      key: string
      attachment: ToolResultAttachment
    }

function resultOf(toolId: string): ToolResultData | undefined {
  return context?.results.value.get(toolId) ?? legacyResults?.value.get(toolId)
}

function isImageBlock(block: ContentBlock): block is Extract<ContentBlock, { type: 'image' }> {
  return block.type === 'image' && 'source' in block
}

const entries = computed<ImageEntry[]>(() => props.tools.flatMap(tool => {
  const result = resultOf(tool.id)
  if (!result) return []
  const inline = typeof result.content === 'string'
    ? []
    : result.content.flatMap((block, index): ImageEntry[] => isImageBlock(block)
      ? [{
          kind: 'inline',
          key: `${tool.id}:inline:${index}`,
          image: block,
          recordUuid: result.recordUuid ?? null,
        }]
      : [])
  const attachments = (result.attachments ?? []).flatMap((attachment, index): ImageEntry[] =>
    attachment.mediaType.startsWith('image/')
      ? [{
          kind: 'attachment',
          key: `${tool.id}:attachment:${attachment.asset.nativeId}:${index}`,
          attachment,
        }]
      : [])
  return [...inline, ...attachments]
}))

function stackStyle(index: number): Record<string, string> {
  const depth = Math.min(entries.value.length - index - 1, 4)
  return {
    '--stack-depth': String(depth),
    '--stack-tilt': `${depth % 2 === 0 ? depth * -0.45 : depth * 0.45}deg`,
    'z-index': String(index + 1),
  }
}
</script>

<template>
  <div
    v-if="entries.length"
    class="tool-image-stack"
    role="group"
    :aria-label="t('block.toolFold.imageCount', { count: entries.length })"
  >
    <div
      v-for="(entry, index) in entries"
      :key="entry.key"
      class="tool-image-stack-card"
      :style="stackStyle(index)"
    >
      <BlockImage
        v-if="entry.kind === 'inline'"
        :block="entry.image"
        :record-uuid="entry.recordUuid"
      />
      <EngineAssetImage
        v-else
        :attachment="entry.attachment"
        auto-load
        compact
      />
    </div>
    <span v-if="entries.length > 1" class="tool-image-stack-count">
      {{ entries.length }}
    </span>
  </div>
</template>

<style scoped>
.tool-image-stack {
  position: relative;
  width: 150px;
  height: 98px;
  flex: none;
  margin: 4px 5px 8px 14px;
  isolation: isolate;
}
.tool-image-stack-card {
  position: absolute;
  inset: 0;
  overflow: hidden;
  border: 1px solid color-mix(in srgb, var(--border) 88%, transparent);
  border-radius: 6px;
  background: var(--card);
  box-shadow: var(--shadow-paper);
  transform: translate(
    calc(var(--stack-depth) * -4px),
    calc(var(--stack-depth) * 4px)
  ) rotate(var(--stack-tilt));
  transform-origin: 70% 50%;
  transition: transform 160ms ease, box-shadow 160ms ease;
}
.tool-image-stack:hover .tool-image-stack-card,
.tool-image-stack:focus-within .tool-image-stack-card {
  transform: translate(
    calc(var(--stack-depth) * -7px),
    calc(var(--stack-depth) * 7px)
  ) rotate(var(--stack-tilt));
}
.tool-image-stack-card:focus-within {
  box-shadow: 0 0 0 2px var(--ring), var(--shadow-paper);
}
.tool-image-stack-card :deep(> div),
.tool-image-stack-card :deep(.engine-asset-image),
.tool-image-stack-card :deep(.engine-asset-image-open) {
  width: 100%;
  height: 100%;
  margin: 0;
}
.tool-image-stack-card :deep(.block-image-thumb),
.tool-image-stack-card :deep(.engine-asset-image-content) {
  display: block;
  width: 100%;
  max-width: none;
  height: 100%;
  max-height: none;
  border: 0;
  border-radius: 5px;
  object-fit: contain;
  background: var(--muted);
}
.tool-image-stack-card :deep(.block-image-loading) {
  min-width: 100%;
  min-height: 100%;
}
.tool-image-stack-card :deep(.engine-asset-image-load) {
  width: 100%;
  height: 100%;
  justify-content: center;
  overflow: hidden;
  padding: 8px;
  color: var(--muted-foreground);
  text-overflow: ellipsis;
}
.tool-image-stack-count {
  position: absolute;
  right: -6px;
  bottom: -7px;
  z-index: 10000;
  display: inline-flex;
  min-width: 18px;
  height: 18px;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border);
  border-radius: 999px;
  padding: 0 5px;
  color: var(--foreground);
  background: color-mix(in srgb, var(--card) 92%, transparent);
  box-shadow: var(--shadow-paper);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  line-height: 1;
}
@media (prefers-reduced-motion: reduce) {
  .tool-image-stack-card { transition: none; }
}
@container (max-width: 420px) {
  .tool-image-stack {
    width: 104px;
    height: 72px;
    margin-left: 9px;
  }
}
</style>
