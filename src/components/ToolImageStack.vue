<script setup lang="ts">
import { computed, inject, ref, type ComputedRef } from 'vue'
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
const expanded = ref(false)
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
  const fullDepth = entries.value.length - index - 1
  const depth = Math.min(fullDepth, 4)
  const tilt = `${depth % 2 === 0 ? depth * -0.45 : depth * 0.45}deg`
  return {
    '--stack-depth': String(depth),
    '--stack-tilt': tilt,
    'transform': expanded.value
      ? `rotate(${tilt})`
      : `translate(${-depth * 4}px, ${depth * 4}px) rotate(${tilt})`,
    'z-index': String(index + 1),
  }
}
</script>

<template>
  <div
    v-if="entries.length"
    class="tool-image-stack"
    :class="{ 'is-expanded': expanded }"
    role="group"
    :aria-label="t('block.toolFold.imageCount', { count: entries.length })"
    @keydown.esc.stop="expanded = false"
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
    <button
      v-if="!expanded"
      type="button"
      class="tool-image-stack-open"
      :aria-expanded="false"
      :aria-label="`${t('common.expand')} · ${t('block.toolFold.imageCount', { count: entries.length })}`"
      @click="expanded = true"
    >
      <span class="tool-image-stack-count-badge">{{ entries.length }}</span>
    </button>
    <button
      v-else
      type="button"
      class="tool-image-stack-count"
      :aria-expanded="true"
      :aria-label="t('common.collapse')"
      @click="expanded = false"
    >
      <span class="i-carbon-chevron-right h-2.5 w-2.5 rotate-180" aria-hidden="true" />
      {{ entries.length }}
    </button>
  </div>
</template>

<style scoped>
.tool-image-stack {
  position: absolute;
  top: -70px;
  right: 6px;
  bottom: auto;
  z-index: 12;
  width: 150px;
  height: 98px;
  isolation: isolate;
}
.tool-image-stack.is-expanded {
  left: 6px;
  z-index: 80;
  display: flex;
  width: auto;
  align-items: stretch;
  justify-content: flex-end;
  gap: 7px;
}
.tool-image-stack-card {
  position: absolute;
  inset: 0;
  overflow: hidden;
  border: 1px solid color-mix(in srgb, var(--border) 86%, var(--foreground) 14%);
  border-radius: 4px;
  padding: 3px;
  background: linear-gradient(
    145deg,
    color-mix(in srgb, var(--card) 96%, white 4%),
    var(--card)
  );
  box-shadow:
    0 1px 1px color-mix(in srgb, var(--foreground) 12%, transparent),
    0 4px 10px color-mix(in srgb, var(--foreground) 13%, transparent),
    var(--shadow-paper);
  transform: translate(
    calc(var(--stack-depth) * -4px),
    calc(var(--stack-depth) * 4px)
  ) rotate(var(--stack-tilt));
  transform-origin: 70% 50%;
  transition: transform 190ms cubic-bezier(0.2, 0.78, 0.2, 1), box-shadow 160ms ease;
}
.tool-image-stack.is-expanded .tool-image-stack-card {
  position: relative;
  inset: auto;
  width: auto;
  min-width: 42px;
  max-width: 150px;
  flex: 1 1 150px;
  overflow: visible;
  box-shadow:
    0 1px 1px color-mix(in srgb, var(--foreground) 14%, transparent),
    0 7px 18px color-mix(in srgb, var(--foreground) 18%, transparent),
    var(--shadow-paper);
}
.tool-image-stack-card:focus-within {
  box-shadow: 0 0 0 2px var(--ring), var(--shadow-paper);
}
.tool-image-stack:not(.is-expanded) .tool-image-stack-card { pointer-events: none; }
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
  border-radius: 2px;
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
.tool-image-stack-open {
  position: absolute;
  inset: -5px;
  z-index: 10001;
  border: 0;
  border-radius: 6px;
  background: transparent;
  cursor: zoom-in;
}
.tool-image-stack-open:focus-visible,
.tool-image-stack-count:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 2px;
}
.tool-image-stack-count-badge {
  position: absolute;
  right: -1px;
  bottom: -2px;
  display: inline-flex;
  min-width: 18px;
  height: 18px;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border);
  border-radius: 999px;
  padding: 0 5px;
  color: var(--foreground);
  background: color-mix(in srgb, var(--card) 94%, transparent);
  box-shadow: var(--shadow-paper);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  line-height: 1;
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
  gap: 3px;
  border: 1px solid var(--border);
  border-radius: 999px;
  padding: 0 5px;
  color: var(--foreground);
  background: color-mix(in srgb, var(--card) 92%, transparent);
  box-shadow: var(--shadow-paper);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  line-height: 1;
  cursor: pointer;
}
@media (prefers-reduced-motion: reduce) {
  .tool-image-stack-card { transition: none; }
}
@container (max-width: 420px) {
  .tool-image-stack {
    width: 104px;
    height: 72px;
    right: 3px;
  }
}
</style>
