import { ref, watch } from 'vue'
import { bridgeSetting, writeSetting } from '@/utils/settingBridge'
import {
  SESSION_READING_ENGINE_IDS,
  isSessionReadingEngineId,
  type SessionReadingEngineId,
} from './sessionReadingEngines'

const STORAGE_KEY = 'monet:sticky-user-prompt-by-engine'
const LEGACY_STORAGE_KEY = 'monet:sticky-user-prompt'
const SETTING_KEY = 'stickyUserPromptByEngine'
const DEFAULT_ENABLED = true

type StickyUserPromptByEngine = Record<SessionReadingEngineId, boolean>

function defaultValues(enabled = DEFAULT_ENABLED): StickyUserPromptByEngine {
  return {
    'claude-code': enabled,
    codex: enabled,
  }
}

function parseValues(value: unknown): StickyUserPromptByEngine | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const source = value as Record<string, unknown>
  return SESSION_READING_ENGINE_IDS.reduce<StickyUserPromptByEngine>((values, engineId) => {
    values[engineId] = typeof source[engineId] === 'boolean' ? source[engineId] : DEFAULT_ENABLED
    return values
  }, defaultValues())
}

function loadValues(): StickyUserPromptByEngine {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw) return parseValues(JSON.parse(raw)) ?? defaultValues()
    const legacy = localStorage.getItem(LEGACY_STORAGE_KEY)
    return defaultValues(legacy === null ? DEFAULT_ENABLED : legacy === 'true')
  } catch {
    return defaultValues()
  }
}

const stickyUserPromptByEngine = ref<StickyUserPromptByEngine>(loadValues())

// settings.json 为权威源:文件有值以文件为准,无值则上迁镜像现值
bridgeSetting({
  key: SETTING_KEY,
  uplift: () => {
    const hasLocalValue = localStorage.getItem(STORAGE_KEY) !== null
      || localStorage.getItem(LEGACY_STORAGE_KEY) !== null
    return hasLocalValue ? stickyUserPromptByEngine.value : undefined
  },
  apply: value => {
    const parsed = parseValues(value)
    if (parsed) stickyUserPromptByEngine.value = parsed
  },
})

watch(stickyUserPromptByEngine, value => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(value))
  } catch {}
  writeSetting(SETTING_KEY, value)
}, { deep: true })

export function useStickyUserPrompt() {
  function stickyUserPromptFor(engineId: string): boolean {
    return isSessionReadingEngineId(engineId)
      ? stickyUserPromptByEngine.value[engineId]
      : DEFAULT_ENABLED
  }

  function setStickyUserPromptFor(engineId: SessionReadingEngineId, enabled: boolean) {
    if (stickyUserPromptByEngine.value[engineId] === enabled) return
    stickyUserPromptByEngine.value = { ...stickyUserPromptByEngine.value, [engineId]: enabled }
  }

  return { stickyUserPromptByEngine, stickyUserPromptFor, setStickyUserPromptFor }
}
