import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export interface CodexEnvInfo {
  installedVersion: string | null
  latestVersion: string | null
  updateAvailable: boolean
  binaryPath: string | null
  desktopVersion: string | null
  versionMismatch: boolean
  activeRuntimeSource: CodexRuntimeSource
  configuredRuntimeSource: CodexRuntimeSource
  activeRuntimeVersion: string | null
  runtimeRestartRequired: boolean
  runtimeSelectionSuggested: boolean
  cacheVersion: string | null
  cacheVersionMismatch: boolean
}

export type CodexRuntimeSource = 'standalone' | 'desktop'

const codexInfo = ref<CodexEnvInfo | null>(null)
const checking = ref(false)
let initialized = false
let pendingCheck: Promise<CodexEnvInfo | null> | null = null
let readinessListenerStarted = false

interface CodexReadinessSnapshot {
  phase: 'warming' | 'ready' | 'degraded'
  error: string | null
}

async function refreshEngineNotices(): Promise<CodexEnvInfo | null> {
  if (pendingCheck) return pendingCheck
  checking.value = true
  pendingCheck = invoke<CodexEnvInfo>('codex_env_check')
    .then((info) => {
      codexInfo.value = info
      return info
    })
    .catch(() => codexInfo.value)
    .finally(() => {
      checking.value = false
      pendingCheck = null
    })
  return pendingCheck
}

function startReadinessListener() {
  if (readinessListenerStarted) return
  readinessListenerStarted = true
  void listen<CodexReadinessSnapshot>('codex-readiness-changed', (event) => {
    if (event.payload.phase !== 'warming') void refreshEngineNotices()
  }).then(() => refreshEngineNotices()).catch(() => {})
}

export function useEngineNotices() {
  if (!initialized) {
    initialized = true
    startReadinessListener()
    void refreshEngineNotices()
  }
  return {
    codexInfo,
    checking,
    codexCreateDelayRisk: computed(() => !!codexInfo.value
      && codexInfo.value.cacheVersionMismatch),
    refreshEngineNotices,
  }
}
