<script setup lang="ts">
import { computed, inject, nextTick, ref, watch, type ComputedRef } from 'vue'
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
const rail = ref<HTMLElement | null>(null)
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

interface DisplayEntry {
  entry: ImageEntry
  chronologicalIndex: number
}

const EXPANDED_POSES = [
  { tilt: -0.8, offsetY: 1 },
  { tilt: 0.45, offsetY: -1 },
  { tilt: -0.25, offsetY: 2 },
  { tilt: 0.75, offsetY: 0 },
  { tilt: -0.55, offsetY: -2 },
  { tilt: 0.2, offsetY: 1 },
] as const

const displayEntries = computed<DisplayEntry[]>(() => {
  const allEntries = entries.value.map((entry, chronologicalIndex) => ({
    entry,
    chronologicalIndex,
  }))
  // 折叠时只有最上层（最后出现）的图片需要真实挂载；其余纸片由 CSS 绘制。
  // 展开时再挂载完整时间线，收起后 Vue 会卸载其余图片并释放对象 URL。
  return expanded.value ? allEntries : allEntries.slice(-1)
})

watch([expanded, () => entries.value.length], async ([isExpanded]) => {
  if (!isExpanded) return
  await nextTick()
  if (rail.value) rail.value.scrollLeft = rail.value.scrollWidth
}, { flush: 'post' })

function stackStyle(chronologicalIndex: number): Record<string, string> {
  if (expanded.value) {
    // 姿态按时间序号循环，而不是按折叠深度计算。这样即使前面的图片原先在
    // 横向视口之外，滚动出现时也仍有稳定且参差的纸片姿态。
    const pose = EXPANDED_POSES[chronologicalIndex % EXPANDED_POSES.length]!
    return {
      'transform': `translateY(${pose.offsetY}px) rotate(${pose.tilt}deg)`,
      'z-index': String(chronologicalIndex + 1),
    }
  }

  const fullDepth = entries.value.length - chronologicalIndex - 1
  const depth = Math.min(fullDepth, 4)
  const tilt = `${depth % 2 === 0 ? depth * -0.45 : depth * 0.45}deg`
  return {
    '--stack-depth': String(depth),
    '--stack-tilt': tilt,
    'transform': `translate(${-depth * 4}px, ${depth * 4}px) rotate(${tilt})`,
    'z-index': String(chronologicalIndex + 1),
  }
}
</script>

<template>
  <div
    v-if="entries.length"
    class="tool-image-stack"
    :class="{
      'is-expanded': expanded,
      'has-multiple': entries.length > 1,
      'has-many': entries.length > 2,
    }"
    role="group"
    :aria-label="t('block.toolFold.imageCount', { count: entries.length })"
    @keydown.esc.stop="expanded = false"
  >
    <div
      ref="rail"
      class="tool-image-stack-rail"
      :tabindex="expanded ? 0 : undefined"
    >
      <div
        v-for="item in displayEntries"
        :key="item.entry.key"
        class="tool-image-stack-card"
        :style="stackStyle(item.chronologicalIndex)"
      >
        <BlockImage
          v-if="item.entry.kind === 'inline'"
          :block="item.entry.image"
          :record-uuid="item.entry.recordUuid"
        />
        <EngineAssetImage
          v-else
          :attachment="item.entry.attachment"
          auto-load
          compact
        />
      </div>
    </div>
    <button
      v-if="!expanded && entries.length > 1"
      type="button"
      class="tool-image-stack-expand"
      :aria-expanded="false"
      :aria-label="`${t('common.expand')} · ${t('block.toolFold.imageCount', { count: entries.length })}`"
      @click.stop="expanded = true"
    >
      <span class="i-carbon-expand-all h-3 w-3" aria-hidden="true" />
      <span>{{ entries.length }}</span>
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
  top: -80px;
  left: 6px;
  z-index: 80;
  width: auto;
  height: 126px;
}
.tool-image-stack-rail {
  position: absolute;
  inset: 0;
}
.tool-image-stack.has-multiple:not(.is-expanded) .tool-image-stack-rail::after,
.tool-image-stack.has-many:not(.is-expanded) .tool-image-stack-rail::before {
  position: absolute;
  inset: 0;
  border: 1px solid color-mix(in srgb, var(--border) 86%, var(--foreground) 14%);
  border-radius: 4px;
  background: var(--card);
  box-shadow: var(--shadow-paper);
  content: '';
  pointer-events: none;
}
.tool-image-stack:not(.is-expanded) .tool-image-stack-rail::before {
  transform: translate(-8px, 8px) rotate(-1.2deg);
}
.tool-image-stack:not(.is-expanded) .tool-image-stack-rail::after {
  transform: translate(-4px, 4px) rotate(0.75deg);
}
.tool-image-stack.is-expanded .tool-image-stack-rail {
  display: flex;
  box-sizing: border-box;
  align-items: stretch;
  justify-content: flex-start;
  gap: 7px;
  overflow-x: auto;
  overflow-y: hidden;
  padding: 10px 12px 18px;
  -ms-overflow-style: none;
  overscroll-behavior-inline: contain;
  scrollbar-width: none;
}
.tool-image-stack.is-expanded .tool-image-stack-rail::-webkit-scrollbar {
  display: none;
  width: 0;
  height: 0;
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
  width: 150px;
  min-width: 150px;
  max-width: 150px;
  flex: 0 0 150px;
  overflow: visible;
  box-shadow:
    0 1px 1px color-mix(in srgb, var(--foreground) 14%, transparent),
    0 7px 18px color-mix(in srgb, var(--foreground) 18%, transparent),
    var(--shadow-paper);
}
.tool-image-stack.is-expanded .tool-image-stack-card:first-child {
  margin-inline-start: auto;
}
.tool-image-stack-card:focus-within {
  box-shadow: 0 0 0 2px var(--ring), var(--shadow-paper);
}
.tool-image-stack-rail:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 2px;
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
.tool-image-stack-expand {
  position: absolute;
  right: -6px;
  bottom: -7px;
  z-index: 10001;
  display: inline-flex;
  min-width: 30px;
  height: 22px;
  align-items: center;
  justify-content: center;
  gap: 3px;
  border: 1px solid var(--border);
  border-radius: 999px;
  padding: 0 6px;
  color: var(--foreground);
  background: color-mix(in srgb, var(--card) 94%, transparent);
  box-shadow: var(--shadow-paper);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  line-height: 1;
  cursor: pointer;
  transition: box-shadow 160ms ease, transform 160ms ease;
}
.tool-image-stack-expand:hover {
  box-shadow: var(--shadow-paper-lifted);
  transform: translateY(-1px);
}
.tool-image-stack-expand:focus-visible,
.tool-image-stack-count:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 2px;
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
  .tool-image-stack-card,
  .tool-image-stack-expand { transition: none; }
}
@container (max-width: 420px) {
  .tool-image-stack {
    width: 104px;
    height: 72px;
    right: 3px;
  }
}
</style>
