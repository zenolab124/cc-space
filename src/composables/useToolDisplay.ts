import { inject, provide, reactive, ref, watch, type ComputedRef, type InjectionKey, type Ref } from 'vue'
import type { ToolResultData } from '@/utils/toolPair'
import { bridgeSetting, writeSetting } from '@/utils/settingBridge'

export const TOOL_DISPLAY_MODES = ['cards', 'individual', 'grouped'] as const
export type ToolDisplayMode = typeof TOOL_DISPLAY_MODES[number]

const STORAGE_KEY = 'monet:tool-display-mode'
const SETTING_KEY = 'toolDisplayMode'

function parseMode(value: unknown): ToolDisplayMode | null {
  return typeof value === 'string' && TOOL_DISPLAY_MODES.includes(value as ToolDisplayMode)
    ? value as ToolDisplayMode
    : null
}

function loadMode(): ToolDisplayMode {
  try {
    return parseMode(localStorage.getItem(STORAGE_KEY)) ?? 'grouped'
  } catch {
    return 'grouped'
  }
}

const toolDisplayMode = ref<ToolDisplayMode>(loadMode())

bridgeSetting({
  key: SETTING_KEY,
  uplift: () => localStorage.getItem(STORAGE_KEY) === null ? undefined : toolDisplayMode.value,
  apply: value => {
    const parsed = parseMode(value)
    if (parsed) toolDisplayMode.value = parsed
  },
})

watch(toolDisplayMode, value => {
  try {
    localStorage.setItem(STORAGE_KEY, value)
  } catch {}
  writeSetting(SETTING_KEY, value)
})

export function useToolDisplayMode() {
  function setToolDisplayMode(mode: ToolDisplayMode) {
    toolDisplayMode.value = mode
  }

  return { toolDisplayMode, setToolDisplayMode }
}

export interface ToolFoldState {
  expandedItems: Set<string>
  collapsedItems: Set<string>
  expandedGroups: Set<string>
  collapsedGroups: Set<string>
  requestedToolId: Ref<string | null>
  reset: () => void
  requestReveal: (toolUseId: string) => void
  clearRevealRequest: (toolUseId: string) => void
}

export const TOOL_FOLD_STATE: InjectionKey<ToolFoldState> = Symbol('tool-fold-state')

export function createToolFoldState(): ToolFoldState {
  const expandedItems = reactive(new Set<string>())
  const collapsedItems = reactive(new Set<string>())
  const expandedGroups = reactive(new Set<string>())
  const collapsedGroups = reactive(new Set<string>())
  const requestedToolId = ref<string | null>(null)

  function reset() {
    expandedItems.clear()
    collapsedItems.clear()
    expandedGroups.clear()
    collapsedGroups.clear()
    requestedToolId.value = null
  }

  function requestReveal(toolUseId: string) {
    requestedToolId.value = toolUseId
    expandedItems.add(toolUseId)
    collapsedItems.delete(toolUseId)
  }

  function clearRevealRequest(toolUseId: string) {
    if (requestedToolId.value === toolUseId) requestedToolId.value = null
  }

  return {
    expandedItems,
    collapsedItems,
    expandedGroups,
    collapsedGroups,
    requestedToolId,
    reset,
    requestReveal,
    clearRevealRequest,
  }
}

const fallbackFoldState = createToolFoldState()

export function provideToolFoldState() {
  const state = createToolFoldState()
  provide(TOOL_FOLD_STATE, state)
  return state
}

export function useToolFoldState() {
  return inject(TOOL_FOLD_STATE, fallbackFoldState)
}

export type AsyncToolState = 'running' | 'waiting' | 'completed' | 'failed' | 'killed' | 'unknown'

export type ToolVisualState = 'done' | 'running' | 'permission' | 'error' | 'background' | 'interrupted' | 'unknown'

export function deriveToolVisualState(opts: {
  result?: ToolResultData
  asyncState?: AsyncToolState | null
  waitingPermission?: boolean
  streaming?: boolean
  runInBackground?: boolean
}): ToolVisualState {
  if (opts.waitingPermission) return 'permission'
  if (opts.asyncState === 'failed') return 'error'
  if (opts.asyncState === 'killed') return 'interrupted'
  if (opts.asyncState === 'running' || opts.asyncState === 'waiting') return 'background'
  if (opts.result?.is_error) return 'error'
  if (opts.result || opts.asyncState === 'completed') return 'done'
  if (opts.streaming) return 'running'
  if (opts.runInBackground) return 'background'
  return 'unknown'
}

export interface ToolExecutionContext {
  results: ComputedRef<Map<string, ToolResultData>>
  asyncStates?: ComputedRef<Map<string, AsyncToolState>>
  permissionRequest?: ComputedRef<{
    toolUseId: string | null
    toolName: string
    input: Record<string, unknown>
  } | null>
}

export const TOOL_EXECUTION_CONTEXT: InjectionKey<ToolExecutionContext> = Symbol('tool-execution-context')
export const TOOL_FOLD_INTERACTION: InjectionKey<() => void> = Symbol('tool-fold-interaction')
