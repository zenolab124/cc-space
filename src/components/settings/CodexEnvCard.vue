<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { relaunch } from '@tauri-apps/plugin-process'
import { useI18n } from 'vue-i18n'
import { isWindows } from '@/composables/usePlatform'
import { openExternalUrl } from '@/composables/useFileOpener'
import { useEngineNotices, type CodexRuntimeSource } from '@/composables/useEngineNotices'

const { t } = useI18n()

interface InstallResult {
  success: boolean
  newVersion: string | null
  command: string
  outputTail: string
  binaryPath?: string | null
}

type InstallPhase = 'installing' | 'verifying'

interface InstallProgress {
  engine: 'claude' | 'codex'
  phase: InstallPhase | 'completed' | 'failed'
}

const { codexInfo: info, checking, refreshEngineNotices: check } = useEngineNotices()
const installing = ref(false)
const installPhase = ref<InstallPhase | null>(null)
const installMsg = ref<{ kind: 'ok' | 'err'; text: string } | null>(null)
const installTail = ref('')
const copiedCmd = ref('')
const savingRuntime = ref(false)
const runtimeMsg = ref<{ kind: 'ok' | 'err'; text: string } | null>(null)
let copiedTimer: ReturnType<typeof setTimeout> | null = null

const showRuntimeSelector = computed(() => !!info.value
  && (info.value.runtimeSelectionSuggested || info.value.configuredRuntimeSource === 'desktop'))
const versionMismatchHintKey = computed(() => {
  if (info.value?.updateAvailable) return 'settings.codexEnv.versionMismatchUpgradeHint'
  if (showRuntimeSelector.value) return 'settings.codexEnv.versionMismatchHint'
  return 'settings.codexEnv.versionMismatchUnavailableHint'
})
const runtimeSourceHintKey = computed(() => info.value?.runtimeSelectionSuggested
  ? 'settings.codexEnv.runtimeSourceHint'
  : 'settings.codexEnv.runtimeSourceConfiguredHint')
const configuredRuntimeVersion = computed(() => {
  if (!info.value) return null
  return info.value.configuredRuntimeSource === 'desktop'
    ? info.value.desktopVersion
    : info.value.installedVersion
})
function normalizeVersion(version: string) {
  return version.trim().replace(/^v/, '')
}
const selectedCacheMismatch = computed(() => !!configuredRuntimeVersion.value
  && !!info.value?.cacheVersion
  && normalizeVersion(configuredRuntimeVersion.value) !== normalizeVersion(info.value.cacheVersion))

const installOptions = computed(() => {
  if (isWindows) {
    return [{ label: 'npm', cmd: 'npm install -g @openai/codex' }]
  }
  return [
    { label: t('settings.codexInstall.official'), cmd: 'curl -fsSL https://chatgpt.com/codex/install.sh | sh' },
    { label: 'npm', cmd: 'npm install -g @openai/codex' },
  ]
})

async function runInstall() {
  if (installing.value) return
  installing.value = true
  installPhase.value = 'installing'
  installMsg.value = null
  installTail.value = ''
  try {
    const result = await invoke<InstallResult>('codex_env_install')
    if (result.success) {
      installPhase.value = 'verifying'
      await check()
      installMsg.value = { kind: 'ok', text: t('settings.codexInstall.installOk', { version: result.newVersion ?? '?' }) }
    } else {
      installMsg.value = { kind: 'err', text: t('settings.codexInstall.installFail') }
      installTail.value = result.outputTail
    }
  } catch (error) {
    installMsg.value = { kind: 'err', text: String(error) }
  } finally {
    installing.value = false
    installPhase.value = null
  }
}

async function changeRuntimeSource(event: Event) {
  const source = (event.target as HTMLSelectElement).value as CodexRuntimeSource
  if (!info.value || source === info.value.configuredRuntimeSource) return
  savingRuntime.value = true
  runtimeMsg.value = null
  try {
    await invoke('codex_runtime_source_set', { source })
    await check()
    runtimeMsg.value = { kind: 'ok', text: t('settings.codexEnv.runtimeSaved') }
  } catch (cause) {
    runtimeMsg.value = { kind: 'err', text: String(cause) }
  } finally {
    savingRuntime.value = false
  }
}

async function restartForRuntime() {
  runtimeMsg.value = { kind: 'ok', text: t('engineSettings.restarting') }
  await relaunch()
}

async function copyCmd(command: string) {
  await navigator.clipboard.writeText(command)
  copiedCmd.value = command
  if (copiedTimer) clearTimeout(copiedTimer)
  copiedTimer = setTimeout(() => { copiedCmd.value = '' }, 1500)
}

function openInstallDocs() {
  openExternalUrl('https://developers.openai.com/codex/cli/').catch(() => {})
}

let unlistenInstallProgress: (() => void) | null = null

onMounted(async () => {
  check()
  unlistenInstallProgress = await listen<InstallProgress>('cli-install-progress', (event) => {
    if (event.payload.engine !== 'codex') return
    if (event.payload.phase === 'installing' || event.payload.phase === 'verifying') {
      installPhase.value = event.payload.phase
    }
  })
})

onUnmounted(() => {
  unlistenInstallProgress?.()
})
</script>

<template>
  <div class="env-card">
    <div class="flex items-center gap-2">
      <span class="i-carbon-cloud-download w-3.5 h-3.5 text-muted-foreground" />
      <span class="text-[11.5px] font-medium">{{ t('settings.codexEnv.title') }}</span>

      <span v-if="checking" class="text-[10px] text-muted-foreground">{{ t('common.loading') }}</span>
      <template v-else-if="info">
        <span v-if="!info.binaryPath" class="env-badge bad">{{ t('settings.codexEnv.notFound') }}</span>
        <span v-else-if="info.updateAvailable" class="env-badge warn">{{ t('settings.codexEnv.updateAvailable') }}</span>
        <span v-else-if="info.installedVersion && info.latestVersion" class="env-badge ok">{{ t('settings.codexEnv.upToDate') }}</span>
        <span v-else-if="info.installedVersion" class="env-badge ok">{{ t('settings.codexEnv.detected') }}</span>
        <span v-else class="env-badge warn">{{ t('settings.codexEnv.versionUnknown') }}</span>
        <span v-if="info.versionMismatch" class="env-badge info">{{ t('settings.codexEnv.versionMismatchBadge') }}</span>
      </template>

      <span class="flex-1" />
      <button class="env-btn" :disabled="checking || installing" @click="check">
        {{ t('settings.codexEnv.refresh') }}
      </button>
    </div>

    <div v-if="info?.installedVersion" class="mt-1.5 flex items-baseline gap-1.5 font-mono text-[13px]">
      <span class="text-foreground">{{ info.installedVersion }}</span>
      <template v-if="info.updateAvailable && info.latestVersion">
        <span class="text-muted-foreground">&rarr;</span>
        <span class="font-semibold text-accent">{{ info.latestVersion }}</span>
      </template>
    </div>
    <p v-if="info?.binaryPath" class="env-path" :title="info.binaryPath">{{ info.binaryPath }}</p>
    <p v-if="info?.binaryPath && !info.latestVersion" class="mt-0.5 text-[10px] text-muted-foreground">
      {{ t('settings.codexEnv.latestUnknown') }}
    </p>

    <div
      v-if="info?.cacheVersionMismatch && !showRuntimeSelector"
      role="status"
      class="mt-2 rounded border border-accent/30 bg-accent/5 px-2.5 py-2 text-[10.5px] leading-relaxed"
    >
      <p class="flex items-start gap-1.5 font-medium text-accent">
        <span class="i-carbon-warning-alt mt-0.5 h-3 w-3 shrink-0" />
        <span>{{ t('settings.codexEnv.cacheMismatchTitle') }}</span>
      </p>
      <p class="mt-1 text-muted-foreground">
        {{ t('settings.codexEnv.cacheMismatchHint', {
          cache: info.cacheVersion ?? '?',
          runtime: info.activeRuntimeVersion ?? '?',
        }) }}
      </p>
    </div>

    <div
      v-if="info?.versionMismatch"
      role="status"
      class="mt-2 rounded border border-border bg-muted/35 px-2.5 py-2 text-[10.5px] leading-relaxed"
    >
      <p class="flex items-start gap-1.5 font-medium text-foreground">
        <span class="i-carbon-information mt-0.5 h-3 w-3 shrink-0 text-muted-foreground" />
        <span>{{ t('settings.codexEnv.versionMismatchTitle') }}</span>
      </p>
      <p class="mt-1 text-muted-foreground">
        {{ t(versionMismatchHintKey, {
          standalone: info.installedVersion ?? '?',
          desktop: info.desktopVersion ?? '?',
        }) }}
      </p>
    </div>

    <div
      v-if="showRuntimeSelector"
      class="mt-2 rounded border border-border bg-muted/25 px-2.5 py-2.5"
    >
      <div class="flex items-center gap-3">
        <div class="min-w-0 flex-1">
          <p class="text-[11px] font-medium text-foreground">{{ t('settings.codexEnv.runtimeSourceTitle') }}</p>
          <p class="mt-0.5 text-[10px] leading-relaxed text-muted-foreground">
            {{ t(runtimeSourceHintKey) }}
          </p>
        </div>
        <select
          class="form-select form-select-sm w-52 shrink-0"
          :value="info?.configuredRuntimeSource"
          :disabled="savingRuntime"
          :aria-label="t('settings.codexEnv.runtimeSourceTitle')"
          @change="changeRuntimeSource"
        >
          <option value="standalone" :disabled="!info?.installedVersion">
            {{ t('settings.codexEnv.runtimeStandalone', { version: info?.installedVersion ?? '?' }) }}
          </option>
          <option value="desktop" :disabled="!info?.desktopVersion">
            {{ t('settings.codexEnv.runtimeDesktop', { version: info?.desktopVersion ?? '?' }) }}
          </option>
        </select>
      </div>

      <div
        v-if="selectedCacheMismatch"
        role="status"
        class="mt-2 flex items-start gap-1.5 rounded border border-accent/30 bg-accent/5 px-2 py-1.5 text-[10px] leading-relaxed text-accent"
      >
        <span class="i-carbon-warning-alt mt-0.5 h-3 w-3 shrink-0" />
        <span>{{ t('settings.codexEnv.runtimeCacheWarning', {
          runtime: configuredRuntimeVersion ?? '?',
          cache: info?.cacheVersion ?? '?',
        }) }}</span>
      </div>

      <div v-if="info?.runtimeRestartRequired" class="mt-2 flex items-center justify-between gap-3 border-t border-border pt-2">
        <p class="text-[10px] text-muted-foreground">{{ t('settings.codexEnv.runtimeRestartHint') }}</p>
        <button type="button" class="env-btn shrink-0" @click="restartForRuntime">
          {{ t('engineSettings.restart') }}
        </button>
      </div>
      <p
        v-if="runtimeMsg"
        role="status"
        :class="['mt-1.5 text-[10px]', runtimeMsg.kind === 'ok' ? 'text-primary' : 'text-destructive']"
      >
        {{ runtimeMsg.text }}
      </p>
    </div>

    <div v-if="info && (!info.binaryPath || info.updateAvailable)" class="mt-2 px-2.5 py-2 rounded border border-border bg-muted/40">
      <p class="text-[11px] font-medium flex items-center gap-1">
        <span class="i-carbon-download w-3 h-3" />
        {{ t(info.updateAvailable ? 'settings.codexInstall.updateTitle' : 'settings.codexInstall.title') }}
      </p>
      <p class="text-[10.5px] text-muted-foreground mt-0.5">
        {{ t(info.updateAvailable ? 'settings.codexInstall.updateHint' : 'settings.codexInstall.hint') }}
      </p>
      <div class="mt-1.5 flex items-center gap-2">
        <button class="env-btn primary" :disabled="installing" @click="runInstall">
          <span v-if="installing" class="i-carbon-circle-dash w-3 h-3 animate-spin" />
          {{ installing
            ? t(installPhase === 'verifying' ? 'settings.codexInstall.verifying' : 'settings.codexInstall.installing')
            : t(info.updateAvailable ? 'settings.codexInstall.updateNow' : 'settings.codexInstall.installNow') }}
        </button>
        <code class="env-path flex-1 !mt-0 truncate text-muted-foreground" :title="installOptions[0].cmd">{{ installOptions[0].cmd }}</code>
      </div>
      <p v-if="installing" class="text-[10px] text-muted-foreground mt-1">
        {{ t(installPhase === 'verifying' ? 'settings.codexInstall.verifyingHint' : 'settings.codexInstall.installingHint') }}
      </p>
      <p v-if="installMsg" :class="['text-[10.5px] mt-1', installMsg.kind === 'ok' ? 'text-primary' : 'text-destructive']">
        {{ installMsg.text }}
      </p>
      <pre v-if="installMsg?.kind === 'err' && installTail" class="env-path !mt-1 max-h-24 overflow-y-auto whitespace-pre-wrap">{{ installTail }}</pre>
      <div v-if="installMsg?.kind === 'err'" class="mt-1.5 space-y-1">
        <p class="text-[10px] text-muted-foreground">{{ t('settings.codexInstall.manualFallback') }}</p>
        <div v-for="option in installOptions" :key="option.cmd" class="flex items-center gap-1.5">
          <span class="text-[10px] text-muted-foreground w-20 shrink-0">{{ option.label }}</span>
          <code class="env-path flex-1 !mt-0 truncate" :title="option.cmd">{{ option.cmd }}</code>
          <button class="env-btn shrink-0" @click="copyCmd(option.cmd)">
            {{ copiedCmd === option.cmd ? t('settings.codexInstall.copied') : t('settings.codexInstall.copy') }}
          </button>
        </div>
      </div>
      <button class="text-[10.5px] text-accent hover:underline mt-1.5" @click="openInstallDocs">
        {{ t('settings.codexInstall.docs') }} ↗
      </button>
    </div>
  </div>
</template>

<style scoped>
.env-card {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 8px 10px;
  margin-bottom: 8px;
  background: var(--card);
}
.env-badge {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 999px;
  line-height: 1.5;
  white-space: nowrap;
}
.env-badge.ok {
  background: color-mix(in srgb, var(--primary) 12%, transparent);
  color: var(--primary);
}
.env-badge.warn {
  background: color-mix(in srgb, var(--accent) 16%, transparent);
  color: var(--accent);
}
.env-badge.info {
  background: var(--muted);
  color: var(--muted-foreground);
}
.env-badge.bad {
  background: color-mix(in srgb, var(--destructive) 12%, transparent);
  color: var(--destructive);
}
.env-path {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10.5px;
  color: var(--muted-foreground);
  margin-top: 2px;
  word-break: break-all;
}
.env-btn {
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 3px 10px;
  font-size: 11px;
  background: var(--background);
  color: var(--foreground);
  cursor: pointer;
  white-space: nowrap;
}
.env-btn.primary {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--primary-foreground);
}
.env-btn:hover:not(:disabled) {
  filter: brightness(1.05);
}
.env-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
