import { inject, provide, reactive, ref, watch, type ComputedRef, type InjectionKey, type Ref } from 'vue'
import type { ToolResultData } from '@/utils/toolPair'
import { bridgeSetting, writeSetting } from '@/utils/settingBridge'
import {
  SESSION_READING_ENGINE_IDS,
  isSessionReadingEngineId,
  type SessionReadingEngineId,
} from './sessionReadingEngines'
import { foldDefaultExpanded, foldDefaultRevision, setFoldDefault } from './useFoldDefaults'

export const TOOL_DISPLAY_MODES = ['cards', 'individual', 'grouped'] as const
export type ToolDisplayMode = typeof TOOL_DISPLAY_MODES[number]

const STORAGE_KEY = 'monet:tool-display-modes'
const LEGACY_STORAGE_KEY = 'monet:tool-display-mode'
const SETTING_KEY = 'toolDisplayModes'
const DEFAULT_MODE: ToolDisplayMode = 'grouped'

function parseMode(value: unknown): ToolDisplayMode | null {
  return typeof value === 'string' && TOOL_DISPLAY_MODES.includes(value as ToolDisplayMode)
    ? value as ToolDisplayMode
    : null
}

type ToolDisplayModes = Record<SessionReadingEngineId, ToolDisplayMode>

function defaultModes(mode: ToolDisplayMode = DEFAULT_MODE): ToolDisplayModes {
  return {
    'claude-code': mode,
    codex: mode,
  }
}

function parseModes(value: unknown, fallback = DEFAULT_MODE): ToolDisplayModes | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const source = value as Record<string, unknown>
  return SESSION_READING_ENGINE_IDS.reduce<ToolDisplayModes>((modes, engineId) => {
    modes[engineId] = parseMode(source[engineId]) ?? fallback
    return modes
  }, defaultModes(fallback))
}

function loadModes(): ToolDisplayModes {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw) return parseModes(JSON.parse(raw)) ?? defaultModes()
    return defaultModes(parseMode(localStorage.getItem(LEGACY_STORAGE_KEY)) ?? DEFAULT_MODE)
  } catch {
    return defaultModes()
  }
}

const toolDisplayModes = ref<ToolDisplayModes>(loadModes())
/** 展示方式变化会影响虚拟列表高度；统一 revision 供各会话表面触发重测。 */
const toolDisplayModeRevision = ref(0)

bridgeSetting({
  key: SETTING_KEY,
  uplift: () => {
    const hasLocalValue = localStorage.getItem(STORAGE_KEY) !== null
      || localStorage.getItem(LEGACY_STORAGE_KEY) !== null
    return hasLocalValue ? toolDisplayModes.value : undefined
  },
  apply: value => {
    const parsed = parseModes(value)
    if (parsed) toolDisplayModes.value = parsed
  },
})

watch(toolDisplayModes, value => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(value))
  } catch {}
  writeSetting(SETTING_KEY, value)
  toolDisplayModeRevision.value += 1
}, { deep: true })

export function useToolDisplayMode() {
  function toolDisplayModeFor(engineId: string): ToolDisplayMode {
    return isSessionReadingEngineId(engineId) ? toolDisplayModes.value[engineId] : DEFAULT_MODE
  }

  function setToolDisplayModeFor(engineId: SessionReadingEngineId, mode: ToolDisplayMode) {
    if (toolDisplayModes.value[engineId] === mode) return
    toolDisplayModes.value = { ...toolDisplayModes.value, [engineId]: mode }
  }

  return {
    toolDisplayModes,
    toolDisplayModeRevision,
    toolDisplayModeFor,
    setToolDisplayModeFor,
  }
}

export interface ToolFoldState {
  expandedItems: Set<string>
  collapsedItems: Set<string>
  expandedGroups: Set<string>
  collapsedGroups: Set<string>
  groupDefaultExpanded: Ref<boolean>
  itemDefaultExpanded: Ref<boolean>
  requestedToolId: Ref<string | null>
  reset: () => void
  setAllGroups: (expanded: boolean) => void
  setAllItems: (expanded: boolean) => void
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

  watch(foldDefaultRevision.toolGroup, () => {
    expandedGroups.clear()
    collapsedGroups.clear()
  }, { flush: 'sync' })
  watch(foldDefaultRevision.toolItem, () => {
    expandedItems.clear()
    collapsedItems.clear()
  }, { flush: 'sync' })

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

  function setAllGroups(expanded: boolean) {
    setFoldDefault('toolGroup', expanded)
  }

  function setAllItems(expanded: boolean) {
    setFoldDefault('toolItem', expanded)
  }

  function clearRevealRequest(toolUseId: string) {
    if (requestedToolId.value === toolUseId) requestedToolId.value = null
  }

  return {
    expandedItems,
    collapsedItems,
    expandedGroups,
    collapsedGroups,
    groupDefaultExpanded: foldDefaultExpanded.toolGroup,
    itemDefaultExpanded: foldDefaultExpanded.toolItem,
    requestedToolId,
    reset,
    setAllGroups,
    setAllItems,
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
  visualStates?: ComputedRef<Map<string, ToolVisualState>>
  permissionRequest?: ComputedRef<{
    toolUseId: string | null
    toolName: string
    input: Record<string, unknown>
  } | null>
}

export const TOOL_EXECUTION_CONTEXT: InjectionKey<ToolExecutionContext> = Symbol('tool-execution-context')
export const TOOL_FOLD_INTERACTION: InjectionKey<() => void> = Symbol('tool-fold-interaction')
