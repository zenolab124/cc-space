<script setup lang="ts">
import { computed, inject, type ComputedRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ToolResultData } from '@/utils/toolPair'
import type { ContentBlock } from '@/types'
import { summarizeToolProcess, type ToolProcessSummaryItem, type ToolUseBlock } from '@/utils/toolDisplay'
import {
  TOOL_EXECUTION_CONTEXT,
  TOOL_FOLD_INTERACTION,
  deriveToolVisualState,
  useToolFoldState,
  type ToolVisualState,
} from '@/composables/useToolDisplay'
import ToolProcessItems from './ToolProcessItems.vue'

const props = defineProps<{
  blocks: ContentBlock[]
  blockRecordUuids?: Array<string | null | undefined>
  tools: ToolUseBlock[]
  streaming?: boolean
}>()

const { t } = useI18n()
const foldState = useToolFoldState()
const onInteraction = inject(TOOL_FOLD_INTERACTION, () => {})
const context = inject(TOOL_EXECUTION_CONTEXT, null)
const legacyResults = inject<ComputedRef<Map<string, ToolResultData>>>('toolResultMap')

function stateOf(tool: ToolUseBlock): ToolVisualState {
  const request = context?.permissionRequest?.value
  return deriveToolVisualState({
    result: context?.results.value.get(tool.id) ?? legacyResults?.value.get(tool.id),
    asyncState: context?.asyncStates?.value.get(tool.id),
    waitingPermission: request?.toolUseId === tool.id,
    streaming: props.streaming,
    runInBackground: tool.name === 'Bash' && tool.input.run_in_background === true,
  })
}

const states = computed(() => props.tools.map(stateOf))
const groupState = computed<ToolVisualState>(() => {
  const priority: ToolVisualState[] = ['permission', 'error', 'interrupted', 'running', 'background', 'unknown', 'done']
  return priority.find(state => states.value.includes(state)) ?? 'unknown'
})
const failedCount = computed(() => states.value.filter(state => state === 'error').length)
const processSummary = computed(() => summarizeToolProcess(props.tools))
const visibleSummary = computed(() => processSummary.value.slice(0, 2))
const hiddenToolCount = computed(() =>
  processSummary.value.slice(2).reduce((total, item) => total + item.count, 0),
)

function actionLabel(item: ToolProcessSummaryItem): string {
  if (item.kind === 'other') return item.name
  return t(`block.toolFold.action.${item.kind}`)
}

function summaryLabel(item: ToolProcessSummaryItem): string {
  const action = actionLabel(item)
  if (item.count > 1) return `${action} ×${item.count}`
  if (!item.detail) return action
  return item.detail.toLocaleLowerCase().startsWith(action.toLocaleLowerCase())
    ? item.detail
    : `${action} ${item.detail}`
}

const summaryTitle = computed(() => [
  processSummary.value.map(summaryLabel).join(' · '),
  t('block.foldShiftHint'),
].filter(Boolean).join('\n'))
const stateLabel = computed(() => {
  if (groupState.value === 'permission' && failedCount.value) {
    return t('block.toolFold.failedAndPermission', { count: failedCount.value })
  }
  if (groupState.value === 'permission') return t('block.toolFold.permission')
  if (groupState.value === 'error') return t('block.toolFold.failedCount', { count: failedCount.value })
  if (groupState.value === 'interrupted') return t('block.toolFold.interrupted')
  if (groupState.value === 'running') return t('block.toolFold.runningProgress', {
    current: Math.max(1, states.value.findIndex(state => state === 'running') + 1),
    total: props.tools.length,
  })
  if (groupState.value === 'background') return t('block.toolFold.background')
  return ''
})

const groupKey = computed(() => props.tools[0]?.id ?? '')
const containsRequested = computed(() => props.tools.some(tool => tool.id === foldState.requestedToolId.value))
const autoExpanded = computed(() =>
  containsRequested.value || groupState.value === 'running' || groupState.value === 'permission',
)
const expanded = computed(() => {
  if (containsRequested.value) return true
  if (foldState.collapsedGroups.has(groupKey.value)) return false
  return foldState.expandedGroups.has(groupKey.value)
    || foldState.groupDefaultExpanded.value
    || autoExpanded.value
})

function toggle(event: MouseEvent) {
  onInteraction()
  if (event.shiftKey) {
    foldState.setAllGroups(!expanded.value)
    return
  }
  if (expanded.value) {
    foldState.expandedGroups.delete(groupKey.value)
    foldState.collapsedGroups.add(groupKey.value)
  } else {
    foldState.collapsedGroups.delete(groupKey.value)
    foldState.expandedGroups.add(groupKey.value)
  }
}
</script>

<template>
  <div class="tool-process-group">
    <button
      type="button"
      class="tool-process-line"
      :class="[`is-${groupState}`, { 'is-expanded': expanded }]"
      :aria-expanded="expanded"
      :title="summaryTitle"
      @click="toggle"
    >
      <span class="i-carbon-chevron-right tool-process-chevron" :class="{ 'rotate-90': expanded }" />
      <span class="tool-process-summary">
        <template v-for="(item, index) in visibleSummary" :key="`${item.kind}:${item.name}`">
          <span v-if="index" class="tool-process-separator" aria-hidden="true">·</span>
          <span>{{ summaryLabel(item) }}</span>
        </template>
        <span v-if="hiddenToolCount" class="tool-process-more">+{{ hiddenToolCount }}</span>
      </span>
      <span
        v-if="stateLabel"
        class="tool-process-running"
        :class="`is-${groupState}`"
      >
        {{ stateLabel }}
        <span v-if="groupState === 'running'" class="tool-process-dots" aria-hidden="true"><i /><i /><i /></span>
      </span>
    </button>
    <div v-if="expanded" class="tool-process-items">
      <slot>
        <ToolProcessItems
          :blocks="blocks"
          :block-record-uuids="blockRecordUuids"
          :streaming="streaming"
        />
      </slot>
    </div>
  </div>
</template>

<style scoped>
.tool-process-group { min-width: 0; margin: 0; }
.tool-process-line {
  display: flex;
  align-items: center;
  gap: 4px;
  width: 100%;
  min-height: var(--tool-row-height);
  padding: 0;
  border: 0;
  color: color-mix(in srgb, var(--muted-foreground) 56%, transparent);
  background: transparent;
  text-align: left;
  cursor: pointer;
  line-height: var(--tool-row-line-height);
  transition: color 120ms ease;
}
.tool-process-line:hover,
.tool-process-line:focus-visible,
.tool-process-line.is-expanded,
.tool-process-line.is-running,
.tool-process-line.is-permission,
.tool-process-line.is-error,
.tool-process-line.is-interrupted { color: var(--foreground); }
.tool-process-line:focus-visible { outline: 1px solid var(--ring); outline-offset: 2px; }
.tool-process-chevron { width: 11px; height: 11px; flex: none; transition: transform 150ms; }
.tool-process-summary {
  display: flex;
  min-width: 0;
  align-items: baseline;
  gap: 4px;
  overflow: hidden;
  font-size: 10.5px;
  font-weight: 450;
  line-height: var(--tool-row-line-height);
  white-space: nowrap;
}
.tool-process-summary > span:not(.tool-process-separator, .tool-process-more) {
  overflow: hidden;
  text-overflow: ellipsis;
}
.tool-process-separator,
.tool-process-more {
  flex: none;
  color: color-mix(in srgb, currentColor 65%, transparent);
}
.tool-process-running { display: inline-flex; align-items: center; margin-left: auto; color: var(--muted-foreground); font-size: 11px; line-height: var(--tool-row-line-height); }
.tool-process-running.is-running { color: var(--claude); }
.tool-process-running.is-permission { color: var(--accent); }
.tool-process-running.is-error,
.tool-process-running.is-interrupted { color: var(--destructive); }
.tool-process-running.is-background { color: var(--primary); }
.tool-process-items { margin: 0 0 4px 15px; padding-left: 9px; border-left: 1px solid var(--border); }
.tool-process-dots { display: inline-flex; width: 17px; gap: 2px; margin-left: 4px; }
.tool-process-dots i {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.22;
  animation: tool-process-dot-wave 1.05s infinite ease-in-out;
}
.tool-process-dots i:nth-child(2) { animation-delay: 150ms; }
.tool-process-dots i:nth-child(3) { animation-delay: 300ms; }
@keyframes tool-process-dot-wave {
  0%, 60%, 100% { opacity: 0.22; transform: translateY(0); }
  30% { opacity: 1; transform: translateY(-2px); }
}
@media (prefers-reduced-motion: reduce) {
  .tool-process-dots i { animation: none; opacity: 0.65; }
}
</style>
