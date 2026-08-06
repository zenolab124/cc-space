<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import { isWindows } from '@/composables/usePlatform'

const { t } = useI18n()

interface CodexEnvInfo {
  installedVersion: string | null
  binaryPath: string | null
}

interface InstallResult {
  success: boolean
  newVersion: string | null
  command: string
  outputTail: string
}

const info = ref<CodexEnvInfo | null>(null)
const checking = ref(false)
const installing = ref(false)
const installMsg = ref<{ kind: 'ok' | 'err'; text: string } | null>(null)
const installTail = ref('')
const copiedCmd = ref('')
let copiedTimer: ReturnType<typeof setTimeout> | null = null

const installOptions = computed(() => {
  if (isWindows) {
    return [{ label: 'npm', cmd: 'npm install -g @openai/codex' }]
  }
  return [
    { label: t('settings.codexInstall.official'), cmd: 'curl -fsSL https://chatgpt.com/codex/install.sh | sh' },
    { label: 'npm', cmd: 'npm install -g @openai/codex' },
  ]
})

async function check() {
  checking.value = true
  try {
    info.value = await invoke<CodexEnvInfo>('codex_env_check')
  } catch { /* ignore */ }
  finally { checking.value = false }
}

async function runInstall() {
  if (installing.value) return
  installing.value = true
  installMsg.value = null
  installTail.value = ''
  try {
    const result = await invoke<InstallResult>('codex_env_install')
    if (result.success) {
      installMsg.value = { kind: 'ok', text: t('settings.codexInstall.installOk', { version: result.newVersion ?? '?' }) }
      await check()
    } else {
      installMsg.value = { kind: 'err', text: t('settings.codexInstall.installFail') }
      installTail.value = result.outputTail
    }
  } catch (error) {
    installMsg.value = { kind: 'err', text: String(error) }
  } finally {
    installing.value = false
  }
}

async function copyCmd(command: string) {
  await navigator.clipboard.writeText(command)
  copiedCmd.value = command
  if (copiedTimer) clearTimeout(copiedTimer)
  copiedTimer = setTimeout(() => { copiedCmd.value = '' }, 1500)
}

function openInstallDocs() {
  invoke('open_in_default_app', { path: 'https://developers.openai.com/codex/cli/' }).catch(() => {})
}

onMounted(check)
</script>

<template>
  <div class="env-card">
    <div class="flex items-center gap-2">
      <span class="i-carbon-cloud-download w-3.5 h-3.5 text-muted-foreground" />
      <span class="text-[11.5px] font-medium">{{ t('settings.codexEnv.title') }}</span>

      <span v-if="checking" class="text-[10px] text-muted-foreground">{{ t('common.loading') }}</span>
      <template v-else-if="info">
        <span v-if="!info.binaryPath" class="env-badge bad">{{ t('settings.codexEnv.notFound') }}</span>
        <span v-else-if="info.installedVersion" class="env-badge ok">{{ t('settings.codexEnv.detected') }}</span>
        <span v-else class="env-badge warn">{{ t('settings.codexEnv.versionUnknown') }}</span>
      </template>

      <span class="flex-1" />
      <button class="env-btn" :disabled="checking || installing" @click="check">
        {{ t('settings.codexEnv.refresh') }}
      </button>
    </div>

    <div v-if="info?.installedVersion" class="mt-1.5 font-mono text-[13px]">
      <span class="text-foreground">{{ info.installedVersion }}</span>
    </div>
    <p v-if="info?.binaryPath" class="env-path" :title="info.binaryPath">{{ info.binaryPath }}</p>

    <div v-if="info && !info.binaryPath" class="mt-2 px-2.5 py-2 rounded border border-border bg-muted/40">
      <p class="text-[11px] font-medium flex items-center gap-1">
        <span class="i-carbon-download w-3 h-3" />
        {{ t('settings.codexInstall.title') }}
      </p>
      <p class="text-[10.5px] text-muted-foreground mt-0.5">{{ t('settings.codexInstall.hint') }}</p>
      <div class="mt-1.5 flex items-center gap-2">
        <button class="env-btn primary" :disabled="installing" @click="runInstall">
          <span v-if="installing" class="i-carbon-circle-dash w-3 h-3 animate-spin" />
          {{ installing ? t('settings.codexInstall.installing') : t('settings.codexInstall.installNow') }}
        </button>
        <code class="env-path flex-1 !mt-0 truncate text-muted-foreground" :title="installOptions[0].cmd">{{ installOptions[0].cmd }}</code>
      </div>
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
