import { inject, provide, reactive, ref, watch, type ComputedRef, type InjectionKey, type Ref } from 'vue'
import type { ToolResultData } from '@/utils/toolPair'
import { bridgeSetting, writeSetting } from '@/utils/settingBridge'
import { inferModel } from '@/utils/modelContext'
import { foldDefaultExpanded, foldDefaultRevision, setFoldDefault } from './useFoldDefaults'

export const TOOL_DISPLAY_MODES = ['cards', 'individual', 'grouped'] as const
export type ToolDisplayMode = typeof TOOL_DISPLAY_MODES[number]

const STORAGE_KEY = 'monet:tool-display-mode'
const OVERRIDES_STORAGE_KEY = 'monet:tool-display-mode-overrides'
const SETTING_KEY = 'toolDisplayMode'
const OVERRIDES_SETTING_KEY = 'toolDisplayModeOverrides'

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

function parseOverrides(value: unknown): Record<string, ToolDisplayMode> | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const parsed: Record<string, ToolDisplayMode> = {}
  for (const [key, mode] of Object.entries(value)) {
    const validMode = parseMode(mode)
    if (key && validMode) parsed[key] = validMode
  }
  return parsed
}

function loadOverrides(): Record<string, ToolDisplayMode> {
  try {
    const raw = localStorage.getItem(OVERRIDES_STORAGE_KEY)
    return raw ? parseOverrides(JSON.parse(raw)) ?? {} : {}
  } catch {
    return {}
  }
}

const toolDisplayModeOverrides = ref<Record<string, ToolDisplayMode>>(loadOverrides())
/** 展示方式变化会影响虚拟列表高度；统一 revision 供各会话表面触发重测。 */
const toolDisplayModeRevision = ref(0)

export function normalizeToolDisplayModel(model: string | null | undefined): string | null {
  const value = model?.trim()
  if (!value || value === '<synthetic>') return null
  return inferModel(value)?.id ?? value.toLowerCase()
}

export function toolDisplayModelKey(engineId: string, model: string | null | undefined): string | null {
  const normalizedModel = normalizeToolDisplayModel(model)
  return normalizedModel ? `${engineId.trim().toLowerCase()}:${normalizedModel}` : null
}

bridgeSetting({
  key: SETTING_KEY,
  uplift: () => localStorage.getItem(STORAGE_KEY) === null ? undefined : toolDisplayMode.value,
  apply: value => {
    const parsed = parseMode(value)
    if (parsed) toolDisplayMode.value = parsed
  },
})

bridgeSetting({
  key: OVERRIDES_SETTING_KEY,
  uplift: () => localStorage.getItem(OVERRIDES_STORAGE_KEY) === null
    ? undefined
    : toolDisplayModeOverrides.value,
  apply: value => {
    const parsed = parseOverrides(value)
    if (parsed) toolDisplayModeOverrides.value = parsed
  },
})

watch(toolDisplayMode, value => {
  try {
    localStorage.setItem(STORAGE_KEY, value)
  } catch {}
  writeSetting(SETTING_KEY, value)
  toolDisplayModeRevision.value += 1
})

watch(toolDisplayModeOverrides, value => {
  try {
    localStorage.setItem(OVERRIDES_STORAGE_KEY, JSON.stringify(value))
  } catch {}
  writeSetting(OVERRIDES_SETTING_KEY, value)
  toolDisplayModeRevision.value += 1
}, { deep: true })

export function useToolDisplayMode() {
  function setToolDisplayMode(mode: ToolDisplayMode) {
    toolDisplayMode.value = mode
  }

  function toolDisplayModeFor(engineId: string, model: string | null | undefined): ToolDisplayMode {
    const key = toolDisplayModelKey(engineId, model)
    return (key ? toolDisplayModeOverrides.value[key] : null) ?? toolDisplayMode.value
  }

  function toolDisplayModeOverrideFor(engineId: string, model: string | null | undefined): ToolDisplayMode | null {
    const key = toolDisplayModelKey(engineId, model)
    return key ? toolDisplayModeOverrides.value[key] ?? null : null
  }

  function setToolDisplayModeFor(
    engineId: string,
    model: string,
    mode: ToolDisplayMode | null,
  ) {
    const key = toolDisplayModelKey(engineId, model)
    if (!key) return
    const next = { ...toolDisplayModeOverrides.value }
    if (mode) next[key] = mode
    else delete next[key]
    toolDisplayModeOverrides.value = next
  }

  return {
    toolDisplayMode,
    toolDisplayModeOverrides,
    toolDisplayModeRevision,
    setToolDisplayMode,
    toolDisplayModeFor,
    toolDisplayModeOverrideFor,
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
