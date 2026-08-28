<script setup lang="ts">
import { computed, inject, type ComputedRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ToolResultData } from '@/utils/toolPair'
import type { ContentBlock } from '@/types'
import { latestChronologicalItem, newestFirst } from '@/utils/chronological'
import {
  summarizeToolProcess,
  toolDisplayTitle,
  type ToolProcessSummaryItem,
  type ToolUseBlock,
} from '@/utils/toolDisplay'
import {
  TOOL_EXECUTION_CONTEXT,
  TOOL_FOLD_INTERACTION,
  deriveToolVisualState,
  useToolFoldState,
  type ToolVisualState,
} from '@/composables/useToolDisplay'
import ToolProcessItems from './ToolProcessItems.vue'
import SessionProcessDisclosure from './session/SessionProcessDisclosure.vue'

const props = defineProps<{
  blocks: ContentBlock[]
  blockRecordUuids?: Array<string | null | undefined>
  tools: ToolUseBlock[]
  streaming?: boolean
  /** 回合级执行轨道只让最新调用占据摘要行。 */
  latestOnly?: boolean
  /** 编排工具展开时只回放历史标题，不渲染输入、结果与卡片。 */
  titlesOnly?: boolean
  showImages?: boolean
}>()

const { t } = useI18n()
const foldState = useToolFoldState()
const onInteraction = inject(TOOL_FOLD_INTERACTION, () => {})
const context = inject(TOOL_EXECUTION_CONTEXT, null)
const legacyResults = inject<ComputedRef<Map<string, ToolResultData>>>('toolResultMap')

function stateOf(tool: ToolUseBlock): ToolVisualState {
  const projected = context?.visualStates?.value.get(tool.id)
  if (projected) return projected
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
const latestTool = computed(() => latestChronologicalItem(props.tools))
const historyTools = computed(() => props.latestOnly
  ? newestFirst(props.tools.slice(0, -1))
  : props.tools)
const groupState = computed<ToolVisualState>(() => {
  if (props.latestOnly) return latestChronologicalItem(states.value) ?? 'unknown'
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

function compactStateLabel(tool: ToolUseBlock): string {
  const state = stateOf(tool)
  if (state === 'permission') return t('block.toolFold.permission')
  if (state === 'error') return t('block.toolFold.failed')
  if (state === 'interrupted') return t('block.toolFold.interrupted')
  if (state === 'running') return t('block.toolFold.running')
  if (state === 'background') return t('block.toolFold.background')
  return ''
}

const summaryTitle = computed(() => [
  props.latestOnly && latestTool.value
    ? toolDisplayTitle(latestTool.value)
    : processSummary.value.map(summaryLabel).join(' · '),
  t('block.foldShiftHint'),
].filter(Boolean).join('\n'))
const stateLabel = computed(() => {
  if (props.latestOnly) {
    if (groupState.value === 'permission') return t('block.toolFold.permission')
    if (groupState.value === 'error') return t('block.toolFold.failed')
    if (groupState.value === 'interrupted') return t('block.toolFold.interrupted')
    if (groupState.value === 'running') return t('block.toolFold.running')
    if (groupState.value === 'background') return t('block.toolFold.background')
    return ''
  }
  if (groupState.value === 'permission' && failedCount.value) {
    return t('block.toolFold.failedAndPermission', { count: failedCount.value })
  }
  if (groupState.value === 'permission') return t('block.toolFold.permission')
  if (groupState.value === 'error') return t('block.toolFold.failedCount', { count: failedCount.value })
  if (groupState.value === 'interrupted') return t('block.toolFold.interrupted')
  if (groupState.value === 'running') {
    return t('block.toolFold.runningProgress', {
      current: Math.max(1, states.value.findIndex(state => state === 'running') + 1),
      total: props.tools.length,
    })
  }
  if (groupState.value === 'background') return t('block.toolFold.background')
  return ''
})

const groupKey = computed(() => props.tools[0]?.id ?? '')
const contentId = computed(() => `tool-process-${groupKey.value.replace(/[^a-zA-Z0-9_-]/g, '-')}`)
const containsRequested = computed(() => props.tools.some(tool => tool.id === foldState.requestedToolId.value))
const autoExpanded = computed(() =>
  containsRequested.value || groupState.value === 'permission',
)
const expanded = computed(() => {
  if (containsRequested.value) return true
  if (foldState.collapsedGroups.has(groupKey.value)) return false
  return foldState.expandedGroups.has(groupKey.value)
    || (!props.latestOnly && foldState.groupDefaultExpanded.value)
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
  <SessionProcessDisclosure
      :expanded="expanded"
      :state="groupState"
      :title="summaryTitle"
      :content-id="contentId"
      @toggle="toggle"
    >
    <template #summary>
      <span v-if="latestOnly && latestTool" class="tool-process-summary is-latest">
        <span class="i-carbon-tool-kit tool-process-icon" aria-hidden="true" />
        <span>{{ toolDisplayTitle(latestTool) }}</span>
      </span>
      <span v-else class="tool-process-summary">
        <template v-for="(item, index) in visibleSummary" :key="`${item.kind}:${item.name}`">
          <span v-if="index" class="tool-process-separator" aria-hidden="true">·</span>
          <span>{{ summaryLabel(item) }}</span>
        </template>
        <span v-if="hiddenToolCount" class="tool-process-more">+{{ hiddenToolCount }}</span>
      </span>
    </template>
    <template #status>
      <span
        v-if="stateLabel"
        class="tool-process-running"
        :class="`is-${groupState}`"
      >
        {{ stateLabel }}
        <span v-if="groupState === 'running'" class="tool-process-dots" aria-hidden="true"><i /><i /><i /></span>
      </span>
    </template>
    <slot>
      <div v-if="titlesOnly" class="tool-title-history">
        <div
          v-for="tool in historyTools"
          :key="tool.id"
          class="tool-title-history-row"
        >
          <span class="i-carbon-tool-kit tool-title-history-icon" aria-hidden="true" />
          <span class="tool-title-history-text">{{ toolDisplayTitle(tool) }}</span>
          <span
            v-if="compactStateLabel(tool)"
            class="tool-title-history-state"
            :class="`is-${stateOf(tool)}`"
          >
            {{ compactStateLabel(tool) }}
          </span>
        </div>
      </div>
      <ToolProcessItems
        v-else
        :blocks="blocks"
        :block-record-uuids="blockRecordUuids"
        :streaming="streaming"
        :show-images="showImages"
        nested
      />
    </slot>
  </SessionProcessDisclosure>
</template>

<style scoped>
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
.tool-process-summary.is-latest { align-items: center; font-size: 11px; }
.tool-process-summary.is-latest > span:last-child { min-width: 0; }
.tool-process-icon { width: 12px; height: 12px; flex: none; opacity: 0.68; }
.tool-title-history {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 1px;
  padding: 1px 0 2px;
}
.tool-title-history-row {
  display: flex;
  min-width: 0;
  min-height: var(--tool-row-height);
  align-items: center;
  gap: 6px;
  color: var(--muted-foreground);
  font-size: 11px;
  line-height: var(--tool-row-line-height);
}
.tool-title-history-icon { width: 12px; height: 12px; flex: none; opacity: 0.58; }
.tool-title-history-text {
  min-width: 0;
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tool-title-history-state { flex: none; font-size: 10px; font-weight: 500; }
.tool-title-history-state.is-error,
.tool-title-history-state.is-interrupted { color: var(--destructive); }
.tool-title-history-state.is-running { color: var(--claude); }
.tool-title-history-state.is-permission { color: var(--accent); }
.tool-title-history-state.is-background { color: var(--primary); }
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
