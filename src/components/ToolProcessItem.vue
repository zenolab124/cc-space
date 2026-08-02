<script setup lang="ts">
import { computed, inject, watch, type ComputedRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ToolResultData } from '@/utils/toolPair'
import type { ToolUseBlock } from '@/utils/toolDisplay'
import { toolSummary } from '@/utils/toolDisplay'
import MessageBlock from './MessageBlock.vue'
import {
  TOOL_EXECUTION_CONTEXT,
  TOOL_FOLD_INTERACTION,
  deriveToolVisualState,
  useToolFoldState,
  type AsyncToolState,
} from '@/composables/useToolDisplay'

const props = defineProps<{
  tool: ToolUseBlock
  streaming?: boolean
}>()

const { t } = useI18n()
const foldState = useToolFoldState()
const onInteraction = inject(TOOL_FOLD_INTERACTION, () => {})
const context = inject(TOOL_EXECUTION_CONTEXT, null)
const legacyResults = inject<ComputedRef<Map<string, ToolResultData>>>('toolResultMap')

const result = computed(() => context?.results.value.get(props.tool.id) ?? legacyResults?.value.get(props.tool.id))
const asyncState = computed<AsyncToolState | null>(() => context?.asyncStates?.value.get(props.tool.id) ?? null)
const waitingPermission = computed(() => {
  const request = context?.permissionRequest?.value
  return !!request && request.toolUseId === props.tool.id
})

const state = computed(() => deriveToolVisualState({
  result: result.value,
  asyncState: asyncState.value,
  waitingPermission: waitingPermission.value,
  streaming: props.streaming,
  runInBackground: props.tool.name === 'Bash' && props.tool.input.run_in_background === true,
}))

const autoExpanded = computed(() => state.value === 'running' || state.value === 'permission')
const expanded = computed(() => {
  if (foldState.collapsedItems.has(props.tool.id)) return false
  return foldState.expandedItems.has(props.tool.id) || autoExpanded.value
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
  if (state.value === 'running') return t('block.toolFold.running')
  if (state.value === 'permission') return t('block.toolFold.permission')
  if (state.value === 'error') return t('block.toolFold.failed')
  if (state.value === 'background') return t('block.toolFold.background')
  if (state.value === 'interrupted') return t('block.toolFold.interrupted')
  if (state.value === 'done') return t('block.toolFold.done')
  return ''
})

function toggle() {
  onInteraction()
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
  <div class="tool-fold-item" :data-tool-use-id="tool.id">
    <button
      type="button"
      class="tool-fold-line"
      :aria-expanded="expanded"
      @click="toggle"
    >
      <span
        class="i-carbon-chevron-right tool-fold-chevron"
        :class="{ 'rotate-90': expanded }"
      />
      <span :class="[iconClass, 'tool-fold-icon']" />
      <span class="tool-fold-main">
        <b>{{ tool.name }}</b>
        <span v-if="toolSummary(tool) !== tool.name"> · {{ toolSummary(tool) }}</span>
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
    <div v-if="expanded" class="tool-fold-card">
      <MessageBlock :block="tool" />
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
  min-height: 26px;
  padding: 2px 0;
  border: 0;
  color: var(--muted-foreground);
  background: transparent;
  text-align: left;
  cursor: pointer;
}
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
  white-space: nowrap;
}
.tool-fold-main b { color: var(--foreground); font-weight: 600; }
.tool-fold-state { display: inline-flex; align-items: center; margin-left: auto; flex: none; font-size: 11px; }
.tool-fold-state.is-running { color: var(--claude); }
.tool-fold-state.is-permission { color: var(--warning, var(--accent)); }
.tool-fold-state.is-error,
.tool-fold-state.is-interrupted { color: var(--destructive); }
.tool-fold-state.is-background { color: var(--primary); }
.tool-fold-card { margin: 2px 0 5px 18px; }
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
