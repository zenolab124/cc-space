<script setup lang="ts">
import { computed, inject, watch, type ComputedRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ContentBlock } from '@/types'
import { flattenResultText, type ToolResultData } from '@/utils/toolPair'
import type { ToolUseBlock } from '@/utils/toolDisplay'
import { isOrchestrationTool, toolDisplayTitle, toolSummary } from '@/utils/toolDisplay'
import MessageBlock from './MessageBlock.vue'
import BlockImage from './blocks/BlockImage.vue'
import EngineAssetImage from './engine/EngineAssetImage.vue'
import {
  TOOL_EXECUTION_CONTEXT,
  TOOL_FOLD_INTERACTION,
  deriveToolVisualState,
  useToolFoldState,
  type AsyncToolState,
} from '@/composables/useToolDisplay'

const props = withDefaults(defineProps<{
  tool: ToolUseBlock
  streaming?: boolean
  /** 分组模式由外层统一折叠,单独模式才显示工具项折叠按钮。 */
  foldable?: boolean
}>(), {
  foldable: true,
})

const { t } = useI18n()
const foldState = useToolFoldState()
const onInteraction = inject(TOOL_FOLD_INTERACTION, () => {})
const context = inject(TOOL_EXECUTION_CONTEXT, null)
const legacyResults = inject<ComputedRef<Map<string, ToolResultData>>>('toolResultMap')

const result = computed(() => context?.results.value.get(props.tool.id) ?? legacyResults?.value.get(props.tool.id))
const orchestration = computed(() => isOrchestrationTool(props.tool))
const title = computed(() => toolDisplayTitle(props.tool))
const canExpand = computed(() => !orchestration.value || result.value !== undefined)
const resultText = computed(() => result.value ? flattenResultText(result.value.content).trim() : '')
const resultContentId = computed(() => `tool-result-${props.tool.id.replace(/[^a-zA-Z0-9_-]/g, '-')}`)
const resultImages = computed(() => {
  const content = result.value?.content
  if (!content || typeof content === 'string') return []
  return content.filter((block): block is Extract<ContentBlock, { type: 'image' }> => block.type === 'image')
})
const asyncState = computed<AsyncToolState | null>(() => context?.asyncStates?.value.get(props.tool.id) ?? null)
const waitingPermission = computed(() => {
  const request = context?.permissionRequest?.value
  return !!request && request.toolUseId === props.tool.id
})

const state = computed(() => context?.visualStates?.value.get(props.tool.id)
  ?? deriveToolVisualState({
    result: result.value,
    asyncState: asyncState.value,
    waitingPermission: waitingPermission.value,
    streaming: props.streaming,
    runInBackground: props.tool.name === 'Bash' && props.tool.input.run_in_background === true,
  }))

const autoExpanded = computed(() => state.value === 'permission')
const expanded = computed(() => {
  if (foldState.collapsedItems.has(props.tool.id)) return false
  return foldState.expandedItems.has(props.tool.id)
    || foldState.itemDefaultExpanded.value
    || autoExpanded.value
})

const iconClass = computed(() => {
  const name = props.tool.name.toLowerCase()
  if (name === 'read') return 'i-carbon-document-view'
  if (name === 'bash') return 'i-carbon-terminal'
  if (name === 'edit' || name === 'write' || name === 'notebookedit') return 'i-carbon-edit'
  if (name === 'grep' || name === 'glob' || name.includes('search')) return 'i-carbon-search'
  if (name === 'task' || name === 'agent' || name === 'workflow') return 'i-carbon-task'
  return 'i-carbon-tool-kit'
})

const stateLabel = computed(() => {
  if (orchestration.value && state.value === 'done') return ''
  if (state.value === 'running') return t('block.toolFold.running')
  if (state.value === 'permission') return t('block.toolFold.permission')
  if (state.value === 'error') return t('block.toolFold.failed')
  if (state.value === 'background') return t('block.toolFold.background')
  if (state.value === 'interrupted') return t('block.toolFold.interrupted')
  if (state.value === 'done') return t('block.toolFold.done')
  return ''
})

function toggle(event: MouseEvent) {
  onInteraction()
  if (event.shiftKey) {
    foldState.setAllItems(!expanded.value)
    return
  }
  if (expanded.value) {
    foldState.expandedItems.delete(props.tool.id)
    foldState.collapsedItems.add(props.tool.id)
  } else {
    foldState.collapsedItems.delete(props.tool.id)
    foldState.expandedItems.add(props.tool.id)
  }
}

watch(() => foldState.requestedToolId.value, requested => {
  if (requested === props.tool.id) foldState.expandedItems.add(props.tool.id)
}, { immediate: true })
</script>

<template>
  <div
    class="tool-fold-item"
    :class="{ 'is-orchestration': orchestration }"
    :data-tool-use-id="tool.id"
  >
    <button
      v-if="foldable && canExpand"
      type="button"
      class="tool-fold-line"
      :aria-expanded="expanded"
      :aria-controls="resultContentId"
      :title="$t('block.foldShiftHint')"
      @click="toggle"
    >
      <span
        class="i-carbon-chevron-right tool-fold-chevron"
        :class="{ 'rotate-90': expanded }"
      />
      <span :class="[iconClass, 'tool-fold-icon']" />
      <span class="tool-fold-main">
        <b>{{ title }}</b>
        <span v-if="!orchestration && toolSummary(tool) !== tool.name"> · {{ toolSummary(tool) }}</span>
      </span>
      <span
        v-if="stateLabel"
        class="tool-fold-state"
        :class="`is-${state}`"
      >
        {{ stateLabel }}
        <span v-if="state === 'running'" class="tool-fold-dots" aria-hidden="true"><i /><i /><i /></span>
      </span>
    </button>
    <div v-else class="tool-fold-line tool-fold-line-static">
      <span class="tool-fold-chevron" aria-hidden="true" />
      <span :class="[iconClass, 'tool-fold-icon']" />
      <span class="tool-fold-main">
        <b>{{ title }}</b>
        <span v-if="!orchestration && toolSummary(tool) !== tool.name"> · {{ toolSummary(tool) }}</span>
      </span>
      <span
        v-if="stateLabel"
        class="tool-fold-state"
        :class="`is-${state}`"
      >
        {{ stateLabel }}
        <span v-if="state === 'running'" class="tool-fold-dots" aria-hidden="true"><i /><i /><i /></span>
      </span>
    </div>
    <div
      v-if="!orchestration && (!foldable || expanded)"
      :id="resultContentId"
      class="tool-fold-card"
    >
      <MessageBlock :block="tool" />
    </div>
    <div
      v-if="orchestration && expanded && result"
      :id="resultContentId"
      class="tool-fold-result"
      :class="{ 'is-error': result.is_error }"
    >
      <pre v-if="resultText" class="tool-fold-result-text">{{ resultText }}</pre>
      <BlockImage
        v-for="(image, index) in resultImages"
        :key="`inline:${index}`"
        :block="image"
        :record-uuid="result.recordUuid"
      />
      <EngineAssetImage
        v-for="attachment in result.attachments"
        :key="attachment.asset.nativeId"
        :attachment="attachment"
        auto-load
        compact
      />
    </div>
    <div
      v-if="!orchestration && result?.attachments?.length"
      class="tool-fold-assets"
    >
      <EngineAssetImage
        v-for="attachment in result.attachments"
        :key="attachment.asset.nativeId"
        :attachment="attachment"
        auto-load
        compact
      />
    </div>
  </div>
</template>

<style scoped>
.tool-fold-item { min-width: 0; }
.tool-fold-line {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  min-height: var(--tool-row-height);
  padding: 0;
  border: 0;
  color: var(--muted-foreground);
  background: transparent;
  text-align: left;
  cursor: pointer;
  line-height: var(--tool-row-line-height);
}
.tool-fold-line-static { cursor: default; }
.tool-fold-line:hover { color: var(--foreground); }
.tool-fold-chevron {
  width: 12px;
  height: 12px;
  flex: none;
  transition: transform 150ms;
}
.tool-fold-icon { width: 14px; height: 14px; flex: none; opacity: 0.78; }
.tool-fold-main {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  line-height: var(--tool-row-line-height);
  white-space: nowrap;
}
.tool-fold-main b { color: var(--foreground); font-weight: 600; }
.tool-fold-item.is-orchestration .tool-fold-line {
  color: color-mix(in srgb, var(--muted-foreground) 82%, transparent);
  font-size: 11px;
}
.tool-fold-item.is-orchestration .tool-fold-main b {
  color: inherit;
  font-weight: 450;
}
.tool-fold-item.is-orchestration .tool-fold-icon { width: 12px; height: 12px; opacity: 0.68; }
.tool-fold-line:focus-visible {
  border-radius: 4px;
  outline: 2px solid var(--ring);
  outline-offset: 2px;
}
.tool-fold-state { display: inline-flex; align-items: center; margin-left: auto; flex: none; font-size: 11px; line-height: var(--tool-row-line-height); }
.tool-fold-state.is-running { color: var(--claude); }
.tool-fold-state.is-permission { color: var(--warning, var(--accent)); }
.tool-fold-state.is-error,
.tool-fold-state.is-interrupted { color: var(--destructive); }
.tool-fold-state.is-background { color: var(--primary); }
.tool-fold-card { margin: 2px 0 6px 18px; }
.tool-fold-card > :deep(*) { margin-top: 0; }
.tool-fold-result {
  margin: 2px 0 6px 18px;
  padding: 2px 0 2px 9px;
  border-left: 1px solid var(--border);
  color: var(--muted-foreground);
}
.tool-fold-result.is-error { color: var(--destructive); border-left-color: color-mix(in srgb, var(--destructive) 30%, transparent); }
.tool-fold-result-text {
  margin: 0;
  font-family: inherit;
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.tool-fold-assets { margin: 2px 0 6px 32px; }
.tool-fold-dots { display: inline-flex; width: 17px; gap: 2px; margin-left: 4px; }
.tool-fold-dots i {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.22;
  animation: tool-dot-wave 1.05s infinite ease-in-out;
}
.tool-fold-dots i:nth-child(2) { animation-delay: 150ms; }
.tool-fold-dots i:nth-child(3) { animation-delay: 300ms; }
@keyframes tool-dot-wave {
  0%, 60%, 100% { opacity: 0.22; transform: translateY(0); }
  30% { opacity: 1; transform: translateY(-2px); }
}
@media (prefers-reduced-motion: reduce) {
  .tool-fold-dots i { animation: none; opacity: 0.65; }
}
</style>
