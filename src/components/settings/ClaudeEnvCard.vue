<script setup lang="ts">
import { onMounted, onUnmounted, ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useI18n } from 'vue-i18n'
import { isWindows } from '@/composables/usePlatform'

const { t } = useI18n()

// --- 环境检测（原 ClaudeEnvCard） ---

interface ClaudeEnvInfo {
  installedVersion: string | null
  latestVersion: string | null
  updateAvailable: boolean
  binaryPath: string | null
  installMethod: string
}

interface UpgradeResult {
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

interface DiagEntry {
  path: string
  version: string | null
  method: string
  isDefault: boolean
}

const info = ref<ClaudeEnvInfo | null>(null)
const checking = ref(false)
const upgrading = ref(false)
const upgradeMsg = ref<{ kind: 'ok' | 'warn' | 'err'; text: string } | null>(null)
const diag = ref<{ entries: DiagEntry[]; multiple: boolean } | null>(null)
const diagging = ref(false)

const METHOD_KEYS: Record<string, string> = {
  official: 'settings.claudeEnv.methodOfficial',
  npm: 'settings.claudeEnv.methodNpm',
  homebrew: 'settings.claudeEnv.methodHomebrew',
  unknown: 'settings.claudeEnv.methodUnknown',
}

async function check() {
  checking.value = true
  try {
    info.value = await invoke<ClaudeEnvInfo>('claude_env_check')
  } catch { /* ignore */ }
  finally { checking.value = false }
}

async function upgrade() {
  if (upgrading.value) return
  upgrading.value = true
  upgradeMsg.value = null
  try {
    const r = await invoke<UpgradeResult>('claude_env_upgrade')
    const prev = info.value?.installedVersion
    if (r.success && r.newVersion && prev && r.newVersion !== prev) {
      upgradeMsg.value = { kind: 'ok', text: t('settings.claudeEnv.upgradeOk', { version: r.newVersion }) }
    } else if (r.success) {
      upgradeMsg.value = { kind: 'warn', text: t('settings.claudeEnv.upgradeUnchanged') }
    } else {
      upgradeMsg.value = { kind: 'err', text: `${t('settings.claudeEnv.upgradeFail')}\n$ ${r.command}\n${r.outputTail}` }
    }
    await check()
  } catch (e) {
    upgradeMsg.value = { kind: 'err', text: String(e) }
  } finally { upgrading.value = false }
}

async function diagnose() {
  if (diag.value) {
    diag.value = null
    return
  }
  diagging.value = true
  try {
    diag.value = await invoke<{ entries: DiagEntry[]; multiple: boolean }>('claude_env_diagnose')
  } catch { /* ignore */ }
  finally { diagging.value = false }
}

// --- 二进制路径定位（原 ClaudeBinaryCard） ---

interface LocateInfo {
  path: string | null
  source: 'manual' | 'cached' | 'scan' | 'loginShell' | null
  manualPath: string | null
  manualValid: boolean
  attempted: string[]
}

const binInfo = ref<LocateInfo | null>(null)
const binLoading = ref(false)
const manualInput = ref('')
const binError = ref('')

const SOURCE_KEYS: Record<string, string> = {
  manual: 'settings.claudeBin.sourceManual',
  cached: 'settings.claudeBin.sourceCached',
  scan: 'settings.claudeBin.sourceScan',
  loginShell: 'settings.claudeBin.sourceLoginShell',
}

/** 优先取 binInfo（locator 更权威），回退 info.binaryPath */
const displayPath = computed(() => binInfo.value?.path ?? info.value?.binaryPath ?? null)

// --- 未检测到时的安装引导 ---

const installOptions = computed(() => {
  if (isWindows) {
    return [
      { label: 'PowerShell', cmd: 'irm https://claude.ai/install.ps1 | iex' },
      { label: 'npm', cmd: 'npm install -g @anthropic-ai/claude-code' },
    ]
  }
  return [
    { label: t('settings.claudeInstall.official'), cmd: 'curl -fsSL https://claude.ai/install.sh | bash' },
    { label: 'npm', cmd: 'npm install -g @anthropic-ai/claude-code' },
  ]
})

const copiedCmd = ref('')
let copiedTimer: ReturnType<typeof setTimeout> | null = null

async function copyCmd(cmd: string) {
  await navigator.clipboard.writeText(cmd)
  copiedCmd.value = cmd
  if (copiedTimer) clearTimeout(copiedTimer)
  copiedTimer = setTimeout(() => { copiedCmd.value = '' }, 1500)
}

const installing = ref(false)
const installPhase = ref<InstallPhase | null>(null)
const installMsg = ref<{ kind: 'ok' | 'err'; text: string } | null>(null)
const installTail = ref('')

async function runInstall() {
  installing.value = true
  installPhase.value = 'installing'
  installMsg.value = null
  installTail.value = ''
  try {
    const r = await invoke<UpgradeResult>('claude_env_install')
    if (r.success) {
      installPhase.value = 'verifying'
      if (r.newVersion && r.binaryPath) {
        info.value = {
          installedVersion: r.newVersion,
          latestVersion: info.value?.latestVersion ?? null,
          updateAvailable: false,
          binaryPath: r.binaryPath,
          installMethod: 'official',
        }
      }
      // 刷新探测状态与版本信息，卡片自动切回已安装形态
      await loadBin()
      installMsg.value = { kind: 'ok', text: t('settings.claudeInstall.installOk', { version: r.newVersion ?? '?' }) }
    } else {
      installMsg.value = { kind: 'err', text: t('settings.claudeInstall.installFail') }
      installTail.value = r.outputTail
    }
  } catch (e) {
    installMsg.value = { kind: 'err', text: String(e) }
  } finally {
    installing.value = false
    installPhase.value = null
  }
}

function openInstallDocs() {
  invoke('open_in_default_app', { path: 'https://code.claude.com/docs' }).catch(() => {})
}

function applyBinInfo(next: LocateInfo) {
  binInfo.value = next
  manualInput.value = next.manualPath ?? ''
}

async function loadBin() {
  binLoading.value = true
  try {
    applyBinInfo(await invoke<LocateInfo>('get_claude_binary_info'))
  } catch { /* ignore */ }
  finally { binLoading.value = false }
}

async function saveManual() {
  binError.value = ''
  binLoading.value = true
  try {
    applyBinInfo(await invoke<LocateInfo>('set_claude_binary_path', {
      path: manualInput.value.trim() || null,
    }))
  } catch (e) {
    binError.value = String(e)
  } finally { binLoading.value = false }
}

async function clearManual() {
  manualInput.value = ''
  await saveManual()
}

async function redetect() {
  binError.value = ''
  binLoading.value = true
  try {
    applyBinInfo(await invoke<LocateInfo>('redetect_claude_binary'))
  } catch { /* ignore */ }
  finally { binLoading.value = false }
}

// --- 初始化 ---

let unlistenInstallProgress: (() => void) | null = null

onMounted(async () => {
  check()
  loadBin()
  unlistenInstallProgress = await listen<InstallProgress>('cli-install-progress', (event) => {
    if (event.payload.engine !== 'claude') return
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
    <!-- 第一行：标题 + 状态 badges + 按钮 -->
    <div class="flex items-center gap-2">
      <span class="i-carbon-cloud-download w-3.5 h-3.5 text-muted-foreground" />
      <span class="text-[11.5px] font-medium">{{ t('settings.claudeEnv.title') }}</span>

      <span v-if="checking" class="text-[10px] text-muted-foreground">{{ t('common.loading') }}</span>
      <template v-else-if="info">
        <span v-if="!info.installedVersion" class="env-badge bad">{{ t('settings.claudeEnv.notFound') }}</span>
        <span v-else-if="info.updateAvailable" class="env-badge warn">{{ t('settings.claudeEnv.updateAvailable') }}</span>
        <span v-else class="env-badge ok">{{ t('settings.claudeEnv.upToDate') }}</span>
        <span v-if="info.installedVersion" class="env-badge off">{{ t(METHOD_KEYS[info.installMethod] ?? METHOD_KEYS.unknown) }}</span>
      </template>
      <!-- 二进制来源 badge -->
      <span
        v-if="binInfo?.path && binInfo.source"
        class="env-badge off"
      >{{ t(SOURCE_KEYS[binInfo.source] ?? '') }}</span>

      <span class="flex-1" />
      <button class="env-btn" :disabled="checking || upgrading || binLoading" @click="() => { check(); loadBin() }">
        {{ t('settings.claudeEnv.refresh') }}
      </button>
      <button class="env-btn" :disabled="diagging" @click="diagnose">
        {{ diag ? t('settings.claudeEnv.diagHide') : t('settings.claudeEnv.diagnose') }}
      </button>
    </div>

    <!-- 第二行：版本号 + 升级按钮 -->
    <div v-if="info?.installedVersion" class="mt-1.5 flex items-baseline gap-1.5 font-mono text-[13px]">
      <span class="text-foreground">{{ info.installedVersion }}</span>
      <template v-if="info.updateAvailable && info.latestVersion">
        <span class="text-muted-foreground">&rarr;</span>
        <span class="text-accent font-semibold">{{ info.latestVersion }}</span>
      </template>
      <button
        v-if="info.updateAvailable"
        class="env-btn primary ml-2 flex items-center gap-1"
        :disabled="upgrading"
        @click="upgrade"
      >
        <span v-if="upgrading" class="i-carbon-circle-dash w-3 h-3 animate-spin shrink-0" />
        {{ upgrading ? t('settings.claudeEnv.upgrading') : t('settings.claudeEnv.upgrade') }}
      </button>
    </div>

    <!-- 第三行：二进制路径（统一显示，不重复） -->
    <p v-if="displayPath" class="env-path" :title="displayPath">{{ displayPath }}</p>
    <p v-if="info && !info.latestVersion" class="text-[10px] text-muted-foreground mt-0.5">
      {{ t('settings.claudeEnv.latestUnknown') }}
    </p>

    <!-- 手动路径无效提示 -->
    <p v-if="binInfo?.manualPath && !binInfo.manualValid" class="text-[10.5px] text-destructive mt-1">
      {{ t('settings.claudeBin.manualInvalid') }}
    </p>

    <!-- 探测失败：展示尝试清单 -->
    <div v-if="binInfo && !binInfo.path" class="mt-1.5">
      <p class="text-[10.5px] text-destructive">{{ t('settings.claudeBin.attemptedTitle') }}</p>
      <ul class="mt-0.5 space-y-0.5">
        <li v-for="a in binInfo.attempted" :key="a" class="env-path !mt-0">{{ a }}</li>
      </ul>
    </div>

    <!-- 未检测到：安装引导（一键执行官方脚本，失败回退手动命令） -->
    <div v-if="binInfo && !binInfo.path" class="mt-2 px-2.5 py-2 rounded border border-border bg-muted/40">
      <p class="text-[11px] font-medium flex items-center gap-1">
        <span class="i-carbon-download w-3 h-3" />
        {{ t('settings.claudeInstall.title') }}
      </p>
      <p class="text-[10.5px] text-muted-foreground mt-0.5">{{ t('settings.claudeInstall.hint') }}</p>
      <div class="mt-1.5 flex items-center gap-2">
        <button class="env-btn primary" :disabled="installing" @click="runInstall">
          <span v-if="installing" class="i-carbon-circle-dash w-3 h-3 animate-spin" />
          {{ installing
            ? t(installPhase === 'verifying' ? 'settings.claudeInstall.verifying' : 'settings.claudeInstall.installing')
            : t('settings.claudeInstall.installNow') }}
        </button>
        <code class="env-path flex-1 !mt-0 truncate text-muted-foreground" :title="installOptions[0].cmd">{{ installOptions[0].cmd }}</code>
      </div>
      <p v-if="installing" class="text-[10px] text-muted-foreground mt-1">
        {{ t(installPhase === 'verifying' ? 'settings.claudeInstall.verifyingHint' : 'settings.claudeInstall.installingHint') }}
      </p>
      <p v-if="installMsg" :class="['text-[10.5px] mt-1', installMsg.kind === 'ok' ? 'text-primary' : 'text-destructive']">
        {{ installMsg.text }}
      </p>
      <pre v-if="installMsg?.kind === 'err' && installTail" class="env-path !mt-1 max-h-24 overflow-y-auto whitespace-pre-wrap">{{ installTail }}</pre>
      <div v-if="installMsg?.kind === 'err'" class="mt-1.5 space-y-1">
        <p class="text-[10px] text-muted-foreground">{{ t('settings.claudeInstall.manualFallback') }}</p>
        <div v-for="opt in installOptions" :key="opt.cmd" class="flex items-center gap-1.5">
          <span class="text-[10px] text-muted-foreground w-20 shrink-0">{{ opt.label }}</span>
          <code class="env-path flex-1 !mt-0 truncate" :title="opt.cmd">{{ opt.cmd }}</code>
          <button class="env-btn shrink-0" @click="copyCmd(opt.cmd)">
            {{ copiedCmd === opt.cmd ? t('settings.claudeInstall.copied') : t('settings.claudeInstall.copy') }}
          </button>
        </div>
      </div>
      <button class="text-[10.5px] text-accent hover:underline mt-1.5" @click="openInstallDocs">
        {{ t('settings.claudeInstall.docs') }} ↗
      </button>
    </div>

    <!-- 第四行：手动路径输入 + 保存/清除/重检测 -->
    <div class="flex gap-1.5 items-center mt-2">
      <input
        v-model="manualInput"
        class="env-input flex-1"
        :placeholder="t('settings.claudeBin.manualPlaceholder')"
        spellcheck="false"
        @keydown.enter="saveManual"
      >
      <button class="env-btn" :disabled="binLoading" @click="saveManual">
        {{ t('common.save') }}
      </button>
      <button
        v-if="binInfo?.manualPath"
        class="env-btn"
        :disabled="binLoading"
        @click="clearManual"
      >
        {{ t('common.clear') }}
      </button>
      <button class="env-btn" :disabled="binLoading" @click="redetect">
        {{ t('settings.claudeBin.redetect') }}
      </button>
    </div>

    <p v-if="binError" class="text-[10.5px] text-destructive mt-1">{{ binError }}</p>

    <!-- 升级结果 -->
    <p
      v-if="upgradeMsg"
      class="mt-1.5 text-[10.5px] whitespace-pre-wrap break-all"
      :class="upgradeMsg.kind === 'ok' ? 'text-primary' : upgradeMsg.kind === 'warn' ? 'text-accent' : 'text-destructive'"
    >{{ upgradeMsg.text }}</p>

    <!-- 冲突诊断 -->
    <div v-if="diag" class="mt-2 border-t border-border pt-1.5">
      <p v-if="diag.multiple" class="text-[10.5px] text-accent mb-1">
        {{ t('settings.claudeEnv.multiWarning', { count: diag.entries.length }) }}
      </p>
      <p v-else class="text-[10.5px] text-muted-foreground mb-1">{{ t('settings.claudeEnv.singleOk') }}</p>
      <div v-for="e in diag.entries" :key="e.path" class="flex items-center gap-1.5 py-0.5">
        <span class="font-mono text-[10px] text-muted-foreground truncate flex-1" :title="e.path">{{ e.path }}</span>
        <span class="font-mono text-[10px] shrink-0">{{ e.version ?? t('settings.claudeEnv.verUnknown') }}</span>
        <span class="env-badge off shrink-0">{{ t(METHOD_KEYS[e.method] ?? METHOD_KEYS.unknown) }}</span>
        <span v-if="e.isDefault" class="env-badge ok shrink-0">{{ t('settings.claudeEnv.default') }}</span>
      </div>
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
.env-badge.off {
  background: color-mix(in srgb, var(--muted-foreground) 12%, transparent);
  color: var(--muted-foreground);
}
.env-path {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10.5px;
  color: var(--muted-foreground);
  margin-top: 2px;
  word-break: break-all;
}
.env-input {
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 3px 8px;
  font-size: 11px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  background: var(--background);
  color: var(--foreground);
  min-width: 0;
}
.env-input:focus {
  outline: none;
  border-color: var(--ring);
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
