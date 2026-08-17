<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification'
import { useNotifications } from '@/composables/useNotifications'

type Status = 'granted' | 'denied' | 'undetermined' | 'targetNotRunning' | 'unverified' | 'unknown'

interface PermRow {
  key: string
  icon: string
  /** 深链系统设置的面板锚点；null 表示无对应面板 */
  panel: string | null
  /** 可通过应用内动作触发系统授权弹窗 */
  requestable: boolean
}

const { t } = useI18n()
const { notifyTransient } = useNotifications()

// 主应用账本：前台会话、工作台任务、终端恢复的权限归因都挂在主 app 上
const appRows: PermRow[] = [
  { key: 'automationTerminal', icon: 'i-carbon-terminal', panel: 'automation', requestable: true },
  { key: 'notifications', icon: 'i-carbon-notification', panel: null, requestable: true },
  { key: 'fullDiskAccess', icon: 'i-carbon-data-base', panel: 'allFiles', requestable: false },
  { key: 'accessibility', icon: 'i-carbon-accessibility', panel: 'accessibility', requestable: true },
  { key: 'screenCapture', icon: 'i-carbon-screen', panel: 'screenRecording', requestable: true },
  { key: 'localNetwork', icon: 'i-carbon-network-3', panel: 'localNetwork', requestable: true },
]

// runner 账本：launchd 启动的定时任务（含其中 Claude 用到的能力）归因挂 runner
const runnerRows: PermRow[] = [
  { key: 'automationSystemEvents', icon: 'i-carbon-power', panel: 'automation', requestable: false },
  { key: 'fullDiskAccess', icon: 'i-carbon-data-base', panel: 'allFiles', requestable: false },
  { key: 'accessibility', icon: 'i-carbon-accessibility', panel: 'accessibility', requestable: false },
  { key: 'screenCapture', icon: 'i-carbon-screen', panel: 'screenRecording', requestable: false },
  { key: 'localNetwork', icon: 'i-carbon-network-3', panel: 'localNetwork', requestable: true },
]

const appPerms = ref<Record<string, Status>>({})
const runnerResult = ref<{ checkedAt: string; permissions: Record<string, Status> } | null>(null)
const checking = ref(false)
const runnerChecking = ref(false)
const requesting = ref<string | null>(null)
const slowRequest = ref<string | null>(null)

function appStatus(key: string): Status {
  return appPerms.value[key] ?? (key === 'localNetwork' ? 'unverified' : 'unknown')
}

function runnerStatus(key: string): Status {
  return runnerResult.value?.permissions[key] ?? (key === 'localNetwork' ? 'unverified' : 'unknown')
}

const STATUS_DOT: Record<Status, string> = {
  granted: 'bg-green-600',
  denied: 'bg-red-500',
  undetermined: 'bg-amber-500',
  targetNotRunning: 'bg-muted-foreground/50',
  unverified: 'bg-amber-500',
  unknown: 'bg-muted-foreground/50',
}

async function refresh(probeLocalNetwork = false) {
  checking.value = true
  try {
    const [perms, notif] = await Promise.all([
      invoke<Record<string, Status>>('check_system_permissions'),
      isPermissionGranted(),
    ])
    if (probeLocalNetwork) {
      perms.localNetwork = await invoke<Status>('check_local_network_permission')
    }
    appPerms.value = { ...perms, notifications: notif ? 'granted' : 'undetermined' }
  } catch (e) {
    notifyTransient(t('common.loadFailed'), String(e))
  } finally {
    checking.value = false
  }
}

function startSlowRequest(key: string): ReturnType<typeof setTimeout> {
  return setTimeout(() => {
    slowRequest.value = key
  }, 1200)
}

function reportLocalNetworkResult(status: Status) {
  if (status === 'granted') {
    notifyTransient(t('settings.permCheck.localNetworkGranted'))
  } else if (status === 'denied') {
    notifyTransient(
      t('settings.permCheck.localNetworkDenied'),
      t('settings.permCheck.localNetworkDeniedHint'),
    )
  } else {
    notifyTransient(
      t('settings.permCheck.localNetworkUnknown'),
      t('settings.permCheck.localNetworkUnknownHint'),
    )
  }
}

async function requestApp(row: PermRow) {
  if (row.key === 'localNetwork' && appStatus(row.key) === 'denied') {
    notifyTransient(
      t('settings.permCheck.localNetworkDenied'),
      t('settings.permCheck.localNetworkDeniedHint'),
    )
    openPanel(row.panel)
    return
  }
  requesting.value = row.key
  const slowTimer = row.key === 'localNetwork' ? startSlowRequest(row.key) : null
  try {
    if (row.key === 'notifications') {
      const r = await requestPermission()
      appPerms.value = { ...appPerms.value, notifications: r === 'granted' ? 'granted' : 'denied' }
    } else {
      const status = await invoke<Status>('request_system_permission', { kind: row.key })
      appPerms.value = { ...appPerms.value, [row.key]: status }
      if (row.key === 'localNetwork') reportLocalNetworkResult(status)
      // 屏幕录制授权写入后，本进程要重启才能读到新状态
      if (row.key === 'screenCapture' && status !== 'granted') {
        notifyTransient(t('settings.permCheck.screenRestartHint'))
      }
    }
  } catch (e) {
    notifyTransient(t('settings.permCheck.requestFailed'), String(e))
  } finally {
    if (slowTimer) clearTimeout(slowTimer)
    slowRequest.value = null
    requesting.value = null
  }
}

// runner 行的授权请求：经 launchd 以 prompt 模式跑一次，系统弹窗归因给 runner
async function requestRunner(row: PermRow) {
  if (row.key === 'localNetwork' && runnerStatus(row.key) === 'denied') {
    notifyTransient(
      t('settings.permCheck.localNetworkDenied'),
      t('settings.permCheck.localNetworkDeniedHint'),
    )
    openPanel(row.panel)
    return
  }
  requesting.value = `runner:${row.key}`
  const slowTimer = row.key === 'localNetwork' ? startSlowRequest(`runner:${row.key}`) : null
  try {
    const before = runnerStatus(row.key)
    runnerResult.value = await invoke('run_runner_health_check', { promptKind: row.key })
    if (row.key === 'localNetwork') reportLocalNetworkResult(runnerStatus(row.key))
    // denied 记录系统不再弹窗，且 runner 是路径型记录无法程序化重置，
    // 只能引导用户去系统设置删除旧条目后重试
    if (runnerStatus(row.key) === 'denied' && before === 'denied') {
      notifyTransient(t('settings.permCheck.stillDenied'), t('settings.permCheck.stillDeniedHint'))
    }
  } catch (e) {
    notifyTransient(t('settings.permCheck.requestFailed'), String(e))
  } finally {
    if (slowTimer) clearTimeout(slowTimer)
    slowRequest.value = null
    requesting.value = null
  }
}

function openPanel(panel: string | null) {
  if (panel) invoke('open_privacy_settings', { panel })
}

async function runRunnerCheck() {
  runnerChecking.value = true
  try {
    runnerResult.value = await invoke('run_runner_health_check')
  } catch (e) {
    notifyTransient(t('settings.permCheck.runnerCheckFailed'), String(e))
  } finally {
    runnerChecking.value = false
  }
}

function formatTime(iso: string): string {
  const d = new Date(iso)
  return isNaN(d.getTime()) ? iso : d.toLocaleString()
}

// 用户去系统设置改完权限切回来时自动重新检测
function onWindowFocus() {
  if (!checking.value && !requesting.value) {
    refresh(appStatus('localNetwork') !== 'unverified')
  }
}

onMounted(async () => {
  refresh(false)
  window.addEventListener('focus', onWindowFocus)
  try {
    runnerResult.value = await invoke('get_runner_health_snapshot')
  } catch {
    /* 无历史结果 */
  }
})

onUnmounted(() => {
  window.removeEventListener('focus', onWindowFocus)
})
</script>

<template>
  <div class="settings-permissions-panel">

    <!-- 主应用账本 -->
    <section class="permissions-card">
      <div class="permissions-card-header">
        <div class="permissions-card-title">
          <span class="permissions-card-icon"><span class="i-carbon-settings" /></span>
          <div>
            <div class="text-xs font-semibold">{{ t('settings.permCheck.appGroup') }}</div>
          </div>
        </div>
        <button
          class="perm-btn"
          :disabled="checking"
          @click="refresh(appStatus('localNetwork') !== 'unverified')"
        >
          <span :class="checking ? 'i-carbon-circle-dash animate-spin' : 'i-carbon-renew'" class="w-3 h-3" />
          {{ t('settings.permCheck.refresh') }}
        </button>
      </div>
      <div
        v-for="row in appRows"
        :key="row.key"
        class="permission-row"
      >
        <span :class="row.icon" class="w-4 h-4 shrink-0 opacity-70" />
        <div class="flex-1 min-w-0">
          <div class="text-xs">{{ t(`settings.permCheck.rows.${row.key}`) }}</div>
          <div class="text-[11px] text-muted-foreground truncate">{{ t(`settings.permCheck.rows.${row.key}Desc`) }}</div>
          <div v-if="slowRequest === row.key" class="permission-request-hint">
            {{ t('settings.permCheck.localNetworkWaitingHint') }}
          </div>
        </div>
        <span class="flex items-center gap-1.5 text-[11px] text-muted-foreground shrink-0">
          <i class="inline-block w-1.5 h-1.5 rounded-full" :class="STATUS_DOT[appStatus(row.key)]" />
          {{ t(`settings.permCheck.status.${appStatus(row.key)}`) }}
        </span>
        <button
          v-if="row.requestable && appStatus(row.key) !== 'granted'"
          class="perm-btn"
          :disabled="requesting === row.key"
          @click="requestApp(row)"
        >
          <span v-if="requesting === row.key" class="i-carbon-circle-dash animate-spin w-3 h-3" />
          {{ requesting === row.key && row.key === 'localNetwork'
            ? t('settings.permCheck.waitingForPermission')
            : row.key === 'localNetwork'
              ? t('settings.permCheck.testAndRequest')
              : t('settings.permCheck.request') }}
        </button>
        <button v-if="row.panel && appStatus(row.key) !== 'granted'" class="perm-btn" @click="openPanel(row.panel)">
          {{ t('settings.permCheck.openSettings') }}
        </button>
      </div>
    </section>

    <!-- runner 账本 -->
    <section class="permissions-card">
      <div class="permissions-card-header">
        <div class="permissions-card-title">
          <span class="permissions-card-icon"><span class="i-carbon-time" /></span>
          <div class="min-w-0">
            <div class="text-xs font-semibold">{{ t('settings.permCheck.runnerGroup') }}</div>
            <div class="permissions-card-hint">
            {{ runnerResult
              ? t('settings.permCheck.lastChecked', { time: formatTime(runnerResult.checkedAt) })
              : t('settings.permCheck.neverChecked') }}
            </div>
          </div>
        </div>
        <button class="perm-btn shrink-0" :disabled="runnerChecking" @click="runRunnerCheck">
          <span :class="runnerChecking ? 'i-carbon-circle-dash animate-spin' : 'i-carbon-play'" class="w-3 h-3" />
          {{ runnerChecking ? t('settings.permCheck.checking') : t('settings.permCheck.runCheck') }}
        </button>
      </div>
      <p class="permissions-card-description">
        {{ t('settings.permCheck.runnerGroupDesc') }}
      </p>
      <div
        v-for="row in runnerRows"
        :key="row.key"
        class="permission-row"
      >
        <span :class="row.icon" class="w-4 h-4 shrink-0 opacity-70" />
        <div class="flex-1 min-w-0">
          <div class="text-xs">{{ t(`settings.permCheck.rows.${row.key}`) }}</div>
          <div v-if="row.key === 'automationSystemEvents'" class="text-[11px] text-muted-foreground truncate">
            {{ t('settings.permCheck.rows.automationSystemEventsDesc') }}
          </div>
          <div v-if="slowRequest === `runner:${row.key}`" class="permission-request-hint">
            {{ t('settings.permCheck.localNetworkWaitingHint') }}
          </div>
        </div>
        <span class="flex items-center gap-1.5 text-[11px] text-muted-foreground shrink-0">
          <i class="inline-block w-1.5 h-1.5 rounded-full" :class="STATUS_DOT[runnerStatus(row.key)]" />
          {{ t(`settings.permCheck.status.${runnerStatus(row.key)}`) }}
        </span>
        <button
          v-if="row.key !== 'fullDiskAccess' && runnerStatus(row.key) !== 'granted'"
          class="perm-btn"
          :disabled="requesting === `runner:${row.key}` || runnerChecking"
          @click="requestRunner(row)"
        >
          <span v-if="requesting === `runner:${row.key}`" class="i-carbon-circle-dash animate-spin w-3 h-3" />
          {{ requesting === `runner:${row.key}` && row.key === 'localNetwork'
            ? t('settings.permCheck.waitingForPermission')
            : row.key === 'localNetwork'
              ? t('settings.permCheck.testAndRequest')
              : t('settings.permCheck.request') }}
        </button>
        <button v-if="row.panel && runnerStatus(row.key) !== 'granted'" class="perm-btn" @click="openPanel(row.panel)">
          {{ t('settings.permCheck.openSettings') }}
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.perm-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  font-size: 11px;
  border: 1px solid hsl(var(--border));
  border-radius: 5px;
  background: hsl(var(--card));
  color: hsl(var(--foreground));
  white-space: nowrap;
}
.perm-btn:hover:not(:disabled) {
  background: hsl(var(--muted));
}
.perm-btn:disabled {
  opacity: 0.5;
}

.settings-permissions-panel {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.permissions-card {
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--card);
  box-shadow: var(--shadow-paper);
}
.permissions-card-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 16px;
  border-bottom: 1px solid var(--border);
  background: color-mix(in srgb, var(--primary) 4%, var(--card));
}
.permissions-card-title {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  min-width: 0;
}
.permissions-card-icon {
  display: grid;
  place-items: center;
  width: 27px;
  height: 27px;
  flex-shrink: 0;
  border: 1px solid color-mix(in srgb, var(--primary) 35%, var(--border));
  border-radius: var(--radius);
  color: var(--primary);
  background: color-mix(in srgb, var(--primary) 8%, transparent);
  font-size: 14px;
}
.permissions-card-hint,
.permissions-card-description {
  color: var(--muted-foreground);
  font-size: 11px;
  line-height: 1.55;
}
.permission-request-hint {
  margin-top: 3px;
  color: var(--accent);
  font-size: 11px;
  line-height: 1.45;
  white-space: normal;
}
.permissions-card-hint { margin-top: 3px; }
.permissions-card-description {
  margin: 0;
  padding: 10px 16px;
  border-bottom: 1px solid var(--border);
}
.permission-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 48px;
  padding: 9px 16px;
  border-bottom: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
}
.permission-row:last-child { border-bottom: 0; }
.permission-row:hover { background: color-mix(in srgb, var(--primary) 3%, transparent); }
@media (max-width: 680px) {
  .permissions-card-header { flex-direction: column; }
  .permission-row { align-items: flex-start; flex-wrap: wrap; }
  .permission-row > :nth-child(2) { min-width: calc(100% - 38px); }
  .permission-row > :nth-child(3) { margin-left: 26px; }
}
</style>
