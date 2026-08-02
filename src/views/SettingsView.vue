<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { useI18n } from 'vue-i18n'
import {
  useChannels,
  refreshChannels,
  channelDisplayName,
  OFFICIAL_CHANNEL_ID,
  OFFICIAL_DIRECT_CHANNEL_ID,
  APPLE_FM_CHANNEL_ID,
  cliEnvTarget,
  refreshCliEnvTarget,
  type ChannelInfo,
  type CcSwitchProvider,
} from '@/composables/useChannels'
import { useUiState } from '@/composables/useUiState'
import { usePlatform } from '@/composables/usePlatform'
import { useConfirm } from '@/composables/useConfirm'
import { useNotifications } from '@/composables/useNotifications'
import { useLocale } from '@/composables/useLocale'
import ChannelForm from '@/components/settings/ChannelForm.vue'
import OfficialDefaultsForm from '@/components/settings/OfficialDefaultsForm.vue'
import AgentIframeDemo from '@/components/settings/AgentIframeDemo.vue'
import ClaudeCodeSettings from '@/components/settings/ClaudeCodeSettings.vue'
import PermissionsPanel from '@/components/settings/PermissionsPanel.vue'
import TurnSignalCard from '@/components/settings/TurnSignalCard.vue'
import SystemSessionViewer from '@/components/SystemSessionViewer.vue'
import { useWorkbench } from '@/composables/useWorkbench'
import { useZoom } from '@/composables/useZoom'
import { useHtmlVisual } from '@/features'
import { useVirtualizationSettings } from '@/composables/useVirtualizationSettings'
import { TOOL_DISPLAY_MODES, useToolDisplayMode, type ToolDisplayMode } from '@/composables/useToolDisplay'
import { useUpdater } from '@/composables/useUpdater'
import { MODELS } from '@/utils/modelContext'

const { t } = useI18n()
const {
  channels, defaultSessionChannel, defaultAgentChannel, defaultAgentModel,
  probeResults, probing,
  revealedTokens, revealToken, hideToken, agentPreferences,
  deleteChannel, setChannelEnabled, setDefaultSessionChannel, setDefaultAgentModel,
  setAgentFeatureModel, revealChannelsDir,
  probeChannel, probeAllChannels, loadAgentPreferences,
  scanCcSwitch, importCcSwitch,
} = useChannels()
const { activeSection } = useUiState()
const { confirm } = useConfirm()
const { notifyTransient } = useNotifications()
const {
  locale, availableLocales, switchLocale,
  translating, translateError, parseLanguageIntent, translateLocale, deleteLocale, isBuiltin,
} = useLocale()

const { minColumnWidth, setMinColumnWidth } = useWorkbench()
const { zoomLevel, setZoom, MIN_ZOOM, MAX_ZOOM, STEP } = useZoom()
const { enabled: htmlVisualEnabled } = useHtmlVisual()
const { threshold: virtualizationThreshold } = useVirtualizationSettings()
const { toolDisplayMode, setToolDisplayMode } = useToolDisplayMode()
const { status: updateStatus, newVersion: updateVersion, errorMessage: updateError, downloadProgress, checkForUpdate, downloadAndInstall, channel: updateChannel, loadChannel, setChannel } = useUpdater()
loadChannel()

// 切通道后立刻查一次：两个通道的版本线不同，不重查用户会以为切换没生效
async function switchUpdateChannel(next: 'stable' | 'nightly') {
  if (updateChannel.value === next) return
  await setChannel(next)
  await checkForUpdate()
}

// MCP Server 注册
const mcpRegistered = ref(false)
const mcpLoading = ref(false)

async function loadMcpStatus() {
  try {
    const status = await invoke<{ registered: boolean }>('get_mcp_status')
    mcpRegistered.value = status.registered
  } catch {}
}

async function toggleMcp() {
  mcpLoading.value = true
  try {
    if (mcpRegistered.value) {
      await invoke('unregister_mcp')
    } else {
      await invoke('register_mcp')
    }
    await loadMcpStatus()
  } catch {}
  finally { mcpLoading.value = false }
}

const agentToggles = ref<Record<string, boolean>>({})
const agentKeys = [
  { key: 'title', label: 'settings.agentTitle', desc: 'settings.agentTitleDesc', tag: 'recommended' as const },
  { key: 'permission_hint', label: 'settings.agentPermissionHint', desc: 'settings.agentPermissionHintDesc', tag: 'beginner' as const },
  { key: 'settings_explain', label: 'settings.agentSettingsExplain', desc: 'settings.agentSettingsExplainDesc', tag: 'asNeeded' as const },
  { key: 'cron_parse', label: 'settings.agentCronParse', desc: 'settings.agentCronParseDesc', tag: 'asNeeded' as const },
  { key: 'tags', label: 'settings.agentTags', desc: 'settings.agentTagsDesc', tag: 'recommended' as const },
  { key: 'summary', label: 'settings.agentSummary', desc: 'settings.agentSummaryDesc', tag: 'recommended' as const },
  { key: 'translate', label: 'settings.agentTranslate', desc: 'settings.agentTranslateDesc', tag: 'asNeeded' as const },
]

async function loadAgentToggles() {
  agentToggles.value = await invoke<Record<string, boolean>>('get_agent_toggles')
}

// Agent 会话落盘（官方 CLI 路径）：开 = 保留可追溯记录，关 = 不留痕
const agentSessionPersist = ref(true)

async function loadAgentSessionPersist() {
  agentSessionPersist.value = await invoke<boolean>('get_agent_session_persist')
}

async function toggleAgentSessionPersist() {
  const next = !agentSessionPersist.value
  agentSessionPersist.value = next
  await invoke('set_agent_session_persist', { enabled: next })
}

async function revealAgentSessionDir() {
  const dir = await invoke<{ dirName: string; path: string; exists: boolean }>('get_agent_session_dir')
  if (!dir.exists) {
    notifyTransient(t('settings.agentSessionDirMissing'))
    return
  }
  await invoke('open_in_finder', { path: dir.path })
}

// 完整会话浮层（Agent 日志带 sessionId 时可打开）
const viewingSession = ref<string | null>(null)

function isAgentEnabled(key: string) {
  return agentToggles.value[key] ?? false
}

async function toggleAgent(key: string) {
  const next = !isAgentEnabled(key)
  agentToggles.value = { ...agentToggles.value, [key]: next }
  await invoke('set_agent_toggle', { key, enabled: next })
}

const showTranslateForm = ref(false)
const customLangInput = ref('')

async function onCustomTranslate() {
  const input = customLangInput.value.trim()
  if (!input) return
  const intent = await parseLanguageIntent(input)
  if (!intent || intent.error) {
    translateError.value = intent?.error || t('settings.langNotRecognized')
    return
  }
  if (intent.code in availableLocales.value) {
    switchLocale(intent.code)
    return
  }
  const ok = await translateLocale(intent.code, intent.name, intent.native)
  if (ok) {
    customLangInput.value = ''
    notifyTransient(t('settings.translateSuccess'))
  }
}

// --- Agent 渠道测试 ---
interface AgentTestResult {
  success: boolean
  channelId: string
  model: string
  durationMs: number
  inputTokens: number
  outputTokens: number
  reply: string
  error?: string
}
const agentTesting = ref(false)
const agentTestResult = ref<AgentTestResult | null>(null)

async function onTestAgent() {
  agentTesting.value = true
  agentTestResult.value = null
  try {
    agentTestResult.value = await invoke<AgentTestResult>('test_agent_channel')
  } catch (e) {
    agentTestResult.value = { success: false, channelId: '', model: '', durationMs: 0, inputTokens: 0, outputTokens: 0, reply: '', error: String(e) }
  } finally {
    agentTesting.value = false
  }
}

// --- Agent 调用日志 ---
interface AgentLogEntry {
  timestamp: string
  feature: string
  channelId: string
  model: string
  durationMs: number
  inputTokens: number
  outputTokens: number
  success: boolean
  error?: string
  /** 官方 CLI 路径的落盘会话 ID（会话落盘开启时才有） */
  sessionId?: string
}
const agentLogs = ref<AgentLogEntry[]>([])
const agentLogsLoading = ref(false)
const showAgentLogs = ref(false)

async function loadAgentLogs() {
  agentLogsLoading.value = true
  try {
    agentLogs.value = await invoke<AgentLogEntry[]>('get_agent_logs')
  } finally {
    agentLogsLoading.value = false
  }
}

async function clearAgentLogs() {
  const ok = await confirm(t('settings.agentLogsClearConfirm'))
  if (!ok) return
  await invoke('clear_agent_logs')
  agentLogs.value = []
}

const agentLogsSorted = computed(() => [...agentLogs.value].reverse())

function agentLogChannelLabel(channelId: string): string {
  if (channelId === 'official(fallback)') {
    return t('settings.agentChannelFallback', { channel: channelDisplayName(OFFICIAL_CHANNEL_ID) })
  }
  if (!channelId) return t('common.unknown')
  return channelDisplayName(channelId)
}

const agentLogsStats = computed(() => {
  const logs = agentLogs.value
  const totalInput = logs.reduce((s, l) => s + l.inputTokens, 0)
  const totalOutput = logs.reduce((s, l) => s + l.outputTokens, 0)
  const successCount = logs.filter(l => l.success).length
  return { total: logs.length, totalInput, totalOutput, successCount }
})

type Tab = 'appearance' | 'channels' | 'agent' | 'claude-code' | 'permissions' | 'extensions' | 'lab' | 'system'
const { isMac } = usePlatform()
const activeTab = ref<Tab>('appearance')

const editing = ref<'new' | ChannelInfo | null>(null)
/** official 渠道轻量编辑(仅默认模型/思考强度两字段) */
const editingOfficial = ref<ChannelInfo | null>(null)

const ccSwitchProviders = ref<CcSwitchProvider[]>([])
const ccSwitchSelected = ref<Set<string>>(new Set())
const ccSwitchScanning = ref(false)
const ccSwitchOpen = ref(false)

async function onScanCcSwitch() {
  ccSwitchScanning.value = true
  try {
    const list = await scanCcSwitch()
    ccSwitchProviders.value = list
    ccSwitchSelected.value = new Set(list.filter(p => !p.alreadyImported).map(p => p.id))
    ccSwitchOpen.value = true
  } catch {
    notifyTransient(t('settings.ccSwitchNotFound'))
  } finally {
    ccSwitchScanning.value = false
  }
}

async function onImportCcSwitch() {
  const ids = [...ccSwitchSelected.value]
  if (!ids.length) return
  const count = await importCcSwitch(ids)
  notifyTransient(t('settings.ccSwitchImported', { count }))
  ccSwitchOpen.value = false
  ccSwitchProviders.value = []
}

function toggleCcSwitchAll() {
  const importable = ccSwitchProviders.value.filter(p => !p.alreadyImported)
  if (ccSwitchSelected.value.size === importable.length) {
    ccSwitchSelected.value = new Set()
  } else {
    ccSwitchSelected.value = new Set(importable.map(p => p.id))
  }
}

const appVersion = ref('')

const wakePolicy = ref('passive')
const widgetDayStart = ref(0)
const widgetMonthMode = ref('natural')

async function loadWidgetConfig() {
  try {
    const cfg = await invoke<{ dayStartHour: number, monthMode: string }>('get_widget_config')
    widgetDayStart.value = cfg.dayStartHour
    widgetMonthMode.value = cfg.monthMode || 'natural'
  } catch {}
}
async function setWidgetDayStart(hour: number) {
  widgetDayStart.value = hour
  await invoke('set_widget_config', { dayStartHour: hour, monthMode: widgetMonthMode.value }).catch(() => {})
}
async function setWidgetMonthMode(mode: string) {
  widgetMonthMode.value = mode
  await invoke('set_widget_config', { dayStartHour: widgetDayStart.value, monthMode: mode }).catch(() => {})
}

// --- 托盘标题配置 ---
interface TrayTitleSlot { provider: string; item: string }
interface TrayTitleConfig { version: number; slots: TrayTitleSlot[] }
interface QuotaItem {
  id: string
  label: string
  kind: 'fiveHour' | 'weekly' | 'other'
}
interface QuotaGroup { id: string; label: string; items: QuotaItem[] }
interface ProviderQuota { id: string; displayName: string; groups: QuotaGroup[] }
interface QuotaBundle { providers: ProviderQuota[] }

const traySlots = ref<TrayTitleSlot[]>([])
const quotaProviders = ref<ProviderQuota[]>([])

function traySlotKey(slot: TrayTitleSlot): string {
  return `${slot.provider}/${slot.item}`
}

const traySlotOptions = computed(() => {
  const options: { key: string; slot: TrayTitleSlot; label: string }[] = []
  for (const provider of quotaProviders.value) {
    for (const group of provider.groups) {
      for (const item of group.items) {
        const metric = item.kind === 'fiveHour'
          ? t('settings.traySlotFiveHour')
          : item.kind === 'weekly'
            ? t('settings.traySlotWeekly')
            : item.label
        options.push({
          key: traySlotKey({ provider: provider.id, item: item.id }),
          slot: { provider: provider.id, item: item.id },
          label: t('settings.traySlotProvider', { provider: provider.displayName, metric }),
        })
      }
    }
  }
  const known = new Set(options.map(option => option.key))
  for (const slot of traySlots.value) {
    const key = traySlotKey(slot)
    if (!known.has(key)) {
      options.push({
        key,
        slot,
        label: t('settings.traySlotUnavailable', { provider: slot.provider }),
      })
    }
  }
  return options
})

function isTraySlotActive(key: string): boolean {
  return traySlots.value.some(slot => traySlotKey(slot) === key)
}

async function toggleTraySlot(option: { key: string; slot: TrayTitleSlot }) {
  if (isTraySlotActive(option.key)) {
    traySlots.value = traySlots.value.filter(slot => traySlotKey(slot) !== option.key)
  } else {
    const order = new Map(traySlotOptions.value.map((item, index) => [item.key, index]))
    traySlots.value = [...traySlots.value, option.slot].sort((left, right) =>
      (order.get(traySlotKey(left)) ?? Number.MAX_SAFE_INTEGER)
      - (order.get(traySlotKey(right)) ?? Number.MAX_SAFE_INTEGER),
    )
  }
  await invoke('set_tray_title_config_v2', { slots: traySlots.value }).catch(() => {})
}

const trayEnabled = ref(true)
const trayToggleFailed = ref(false)
async function toggleTrayEnabled() {
  const next = !trayEnabled.value
  trayEnabled.value = next
  try {
    await invoke('set_tray_enabled', { enabled: next })
    trayToggleFailed.value = false
  } catch {
    // 失败回弹并短暂提示
    trayEnabled.value = !next
    trayToggleFailed.value = true
    setTimeout(() => { trayToggleFailed.value = false }, 2500)
  }
}

async function loadTrayTitleConfig() {
  try {
    trayEnabled.value = await invoke<boolean>('get_tray_enabled')
  } catch {}
  try {
    const cfg = await invoke<TrayTitleConfig>('get_tray_title_config_v2')
    traySlots.value = cfg.slots
  } catch {}
  try {
    const bundle = await invoke<QuotaBundle>('get_quota_bundle')
    quotaProviders.value = bundle.providers
  } catch {}
}

// 系统授权状态：/etc/sudoers.d 白名单是否在位（与 policy 独立——
// 切回被动后规则可保留，下次开启不再弹密码框）
const wakeAuthorized = ref(false)

async function loadWakePolicy() {
  try {
    wakePolicy.value = await invoke<string>('get_routine_wake_policy')
    wakeAuthorized.value = await invoke<boolean>('get_wake_authorization_status')
  } catch {}
}
async function setWakePolicy(policy: string) {
  if (policy === 'active') {
    // 乐观跟随 radio，取消/失败回弹（值变化驱动 DOM 复位）
    wakePolicy.value = 'active'
    const ok = await confirm(
      t('settings.routineWakeAuthBody'),
      t('settings.routineWakeAuthConfirm'),
    )
    if (ok) {
      try {
        await invoke('enable_wake_active')
        wakeAuthorized.value = true
        return
      } catch (e) {
        const msg = String(e)
        notifyTransient(
          t('settings.routineWake'),
          msg.includes('cancelled') ? t('settings.routineWakeAuthDenied') : msg,
        )
      }
    }
    wakePolicy.value = 'passive'
    return
  }
  wakePolicy.value = 'passive'
  try {
    await invoke('set_routine_wake_policy', { policy: 'passive' })
  } catch {}
}

async function removeWakeAuthorization() {
  try {
    await invoke('remove_wake_authorization')
    notifyTransient(t('settings.routineWake'), t('settings.routineWakeAuthRemoved'))
  } catch (e) {
    if (!String(e).includes('cancelled')) {
      notifyTransient(t('settings.routineWake'), String(e))
    }
  }
  // 提权删除可能被取消（规则仍在、策略已降级），以后端真实状态为准
  await loadWakePolicy()
}



onMounted(() => { refreshChannels(); refreshCliEnvTarget(); loadAgentToggles(); loadAgentSessionPersist(); loadAgentPreferences(); loadWakePolicy(); loadWidgetConfig(); loadTrayTitleConfig(); loadMcpStatus(); getVersion().then(v => appVersion.value = v) })

watch(activeSection, (s) => {
  if (s === 'settings') {
    refreshChannels().then(() => probeAllChannels())
  }
})

async function onDelete(ch: ChannelInfo) {
  const ok = await confirm(
    t('settings.deleteChannelConfirm', { name: ch.name, id: ch.id }),
    t('common.delete'),
  )
  if (!ok) return
  try {
    await deleteChannel(ch.id)
    if (editing.value !== 'new' && editing.value?.id === ch.id) editing.value = null
    notifyTransient(t('settings.channelDeleted'))
  } catch (e) {
    notifyTransient(t('settings.deleteFailed'), String(e))
  }
}

const sessionChannels = () => channels.value.filter(c => c.scope !== 'agent-only')

// 官方渠道 Agent 可选模型:从 modelContext 的 MODELS 派生(单源,消灭第二份清单)。
// 取具体版本 id 并剥 [1m] 后缀(Agent 用的是 API 模型名,1M 由 CLI/请求侧处理),去重。
const OFFICIAL_MODELS = [
  ...new Set(MODELS.map(m => m.id.replace(/\[1m\]$/i, ''))),
]

const agentChannelId = ref(defaultAgentChannel.value ?? OFFICIAL_CHANNEL_ID)
watch(defaultAgentChannel, (v) => { agentChannelId.value = v ?? OFFICIAL_CHANNEL_ID })

const hasAppleFm = computed(() => channels.value.some(c => c.id === APPLE_FM_CHANNEL_ID))
const agentChannelCards = computed(() =>
  // 已禁用的全功能渠道在左栏管理；Agent-only 渠道必须保留卡片，才能重新启用。
  channels.value.filter(c =>
    c.id !== OFFICIAL_CHANNEL_ID
    && c.id !== OFFICIAL_DIRECT_CHANNEL_ID
    && c.id !== APPLE_FM_CHANNEL_ID
    && (c.enabled || c.scope === 'agent-only'),
  )
)

/** Agent 卡片的实际生效模型标签:与 Rust 侧 fallback_agent_model 同序——
 *  显式 agentModel > 渠道默认模型 > 清单中含 haiku 者 > 清单首项 > 未配置 */
function agentModelTag(ch: ChannelInfo): string {
  if (ch.agentModel) return ch.agentModel
  if (ch.defaultModel) return ch.defaultModel
  const models = ch.availableModels
  return models.find(m => m.toLowerCase().includes('haiku')) ?? models[0] ?? '—'
}

/** 内置渠道由运行能力决定，不提供启停或删除。 */
const isBuiltinChannel = (id: string) =>
  id === OFFICIAL_CHANNEL_ID || id === OFFICIAL_DIRECT_CHANNEL_ID || id === APPLE_FM_CHANNEL_ID
/** 内置渠道显示名走 i18n(Rust 侧默认名为英文) */
const builtinChannelName = (ch: ChannelInfo) =>
  ch.id === OFFICIAL_CHANNEL_ID ? t('channel.official')
  : ch.id === OFFICIAL_DIRECT_CHANNEL_ID ? t('channel.officialDirect')
  : ch.name
const isAppleFmAgent = computed(() => agentChannelId.value === APPLE_FM_CHANNEL_ID)

function toggleAppleFmAgent() {
  if (isAppleFmAgent.value) {
    onAgentChannelChange(OFFICIAL_CHANNEL_ID)
  } else {
    agentChannelId.value = APPLE_FM_CHANNEL_ID
    setDefaultAgentModel(APPLE_FM_CHANNEL_ID, 'system')
  }
}

function onAgentChannelChange(id: string) {
  agentChannelId.value = id
  if (id === OFFICIAL_CHANNEL_ID) {
    setDefaultAgentModel(null, null)
    return
  }
  if (id === OFFICIAL_DIRECT_CHANNEL_ID) {
    setDefaultAgentModel(id, 'haiku')
    return
  }
  const channel = channels.value.find(ch => ch.id === id)
  setDefaultAgentModel(id, channel?.agentModel ?? null)
}

const agentModelOptions = () => {
  const opts: { channel: string; channelName: string; model: string }[] = []
  for (const m of OFFICIAL_MODELS) {
    opts.push({ channel: OFFICIAL_CHANNEL_ID, channelName: 'Official', model: m })
  }
  for (const ch of channels.value) {
    if (!ch.enabled || ch.id === OFFICIAL_CHANNEL_ID || ch.id === OFFICIAL_DIRECT_CHANNEL_ID) continue
    const models = new Set([...ch.availableModels, ...(ch.agentModel ? [ch.agentModel] : [])])
    for (const m of models) {
      opts.push({ channel: ch.id, channelName: ch.name, model: m })
    }
  }
  return opts
}

async function onReveal() {
  try {
    await revealChannelsDir()
  } catch (e) {
    notifyTransient(t('settings.openDirFailed'), String(e))
  }
}

function onSaved() {
  editing.value = null
  notifyTransient(t('settings.channelSaved'))
}
</script>

<template>
  <div class="h-full p-2.5">
    <div class="h-full flex bg-card border border-border rounded-lg shadow-paper overflow-hidden">
    <!-- 侧栏导航 -->
    <nav class="side-nav">
      <h1 class="side-title">
        <span class="i-carbon-settings w-4 h-4 opacity-70" />{{ $t('settings.title') }}
      </h1>
      <button :class="['side-item', { active: activeTab === 'appearance' }]" @click="activeTab = 'appearance'">
        <span class="i-carbon-paint-brush w-3.5 h-3.5" />{{ $t('settings.appearance') }}
      </button>
      <button :class="['side-item', { active: activeTab === 'channels' }]" @click="activeTab = 'channels'">
        <span class="i-carbon-connect w-3.5 h-3.5" />{{ $t('settings.channels') }}
      </button>
      <button :class="['side-item', { active: activeTab === 'agent' }]" @click="activeTab = 'agent'">
        <span class="i-carbon-machine-learning w-3.5 h-3.5" />{{ $t('settings.agent') }}
      </button>
      <button :class="['side-item', { active: activeTab === 'extensions' }]" @click="activeTab = 'extensions'">
        <span class="i-carbon-plug w-3.5 h-3.5" />{{ $t('settings.extensions') }}
      </button>
      <button :class="['side-item', { active: activeTab === 'claude-code' }]" @click="activeTab = 'claude-code'">
        <span class="i-carbon-json w-3.5 h-3.5" />Claude Code
      </button>
      <button v-if="isMac" :class="['side-item', { active: activeTab === 'permissions' }]" @click="activeTab = 'permissions'">
        <span class="i-carbon-security w-3.5 h-3.5" />{{ $t('settings.permissionsNav') }}
      </button>
      <button :class="['side-item', { active: activeTab === 'lab' }]" @click="activeTab = 'lab'">
        <span class="i-carbon-chemistry w-3.5 h-3.5" />{{ $t('settings.lab') }}
      </button>
      <button :class="['side-item', { active: activeTab === 'system' }]" @click="activeTab = 'system'">
        <span class="i-carbon-settings-adjust w-3.5 h-3.5" />{{ $t('settings.system') }}
        <span v-if="updateStatus === 'available'" class="side-dot" />
      </button>
    </nav>

    <!-- 内容区 -->
    <div :class="['flex-1 min-w-0', activeTab === 'claude-code' ? 'flex flex-col overflow-hidden' : 'overflow-y-auto']">
      <div :class="['settings-body', { 'flex-1 min-h-0 flex flex-col': activeTab === 'claude-code' }]">

        <!-- ====== 外观 ====== -->
        <section v-show="activeTab === 'appearance'">
          <h2 class="section-title">{{ $t('settings.appearance') }}</h2>
          <div class="settings-grid">
            <div class="setting-cell">
              <div class="setting-label">{{ $t('settings.themeLight') }}</div>
              <select class="form-select w-full" disabled>
                <option>Paper</option>
              </select>
            </div>
            <div class="setting-cell">
              <div class="setting-label">{{ $t('settings.themeDark') }}</div>
              <select class="form-select w-full" disabled>
                <option>Ink</option>
              </select>
            </div>
            <div class="setting-cell">
              <div class="setting-label">{{ $t('settings.language') }}</div>
              <select
                :value="locale"
                class="form-select w-full"
                @change="switchLocale(($event.target as HTMLSelectElement).value)"
              >
                <option
                  v-for="(meta, code) in availableLocales"
                  :key="code"
                  :value="code"
                >
                  {{ meta.nativeLabel }}
                </option>
              </select>
              <!-- AI 翻译 -->
              <div class="translate-zone">
                <div
                  v-for="(meta, code) in availableLocales"
                  :key="code"
                  class="flex items-center gap-2 text-xs"
                >
                  <template v-if="!isBuiltin(String(code))">
                    <span class="font-medium">{{ meta.nativeLabel }}</span>
                    <span class="text-muted-foreground">({{ code }})</span>
                    <button
                      class="ml-auto p-0.5 text-muted-foreground hover:text-destructive transition-colors"
                      v-tooltip="$t('common.delete')"
                      @click="deleteLocale(String(code))"
                    >
                      <span class="i-carbon-close w-3 h-3" />
                    </button>
                  </template>
                </div>
                <button
                  v-if="!showTranslateForm"
                  class="text-xs text-primary hover:underline"
                  @click="showTranslateForm = true"
                >
                  {{ $t('settings.addLanguage') }}
                </button>
                <div v-if="showTranslateForm" class="translate-form">
                  <div class="flex gap-2">
                    <input
                      v-model="customLangInput"
                      class="form-input flex-1"
                      :placeholder="$t('settings.customLangPlaceholder')"
                      :disabled="translating"
                      @keydown.enter="onCustomTranslate"
                    />
                    <button
                      class="px-2 py-1 text-xs rounded-md bg-primary text-primary-foreground hover:shadow-paper disabled:opacity-50 transition"
                      :disabled="translating || !customLangInput.trim()"
                      @click="onCustomTranslate"
                    >
                      {{ $t('settings.startTranslate') }}
                    </button>
                    <button
                      class="px-1.5 py-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
                      :disabled="translating"
                      @click="showTranslateForm = false"
                    >
                      {{ $t('common.cancel') }}
                    </button>
                  </div>
                  <p v-if="translating" class="text-[11px] text-muted-foreground mt-1.5">
                    <span class="i-carbon-rotate inline-block w-3 h-3 animate-spin mr-1" />{{ $t('settings.translating') }}
                  </p>
                  <p v-if="translateError" class="text-[11px] text-destructive mt-1">{{ translateError }}</p>
                </div>
              </div>
            </div>
            <div class="setting-cell setting-cell-wide">
              <div class="setting-label">{{ $t('settings.toolDisplayMode') }}</div>
              <div class="tool-display-options" role="radiogroup" :aria-label="$t('settings.toolDisplayMode')">
                <button
                  v-for="mode in TOOL_DISPLAY_MODES"
                  :key="mode"
                  type="button"
                  class="tool-display-option"
                  :class="{ active: toolDisplayMode === mode }"
                  role="radio"
                  :aria-checked="toolDisplayMode === mode"
                  @click="setToolDisplayMode(mode as ToolDisplayMode)"
                >
                  <span class="tool-display-option-title">{{ $t(`settings.toolDisplayMode_${mode}`) }}</span>
                  <span class="tool-display-option-hint">{{ $t(`settings.toolDisplayMode_${mode}Hint`) }}</span>
                </button>
              </div>
              <div class="setting-hint">{{ $t('settings.toolDisplayModeHint') }}</div>
            </div>
            <div class="setting-cell">
              <div class="setting-label">{{ $t('settings.zoomLevel') }}</div>
              <div class="flex items-center gap-2.5">
                <input
                  type="range"
                  :value="zoomLevel"
                  :min="MIN_ZOOM"
                  :max="MAX_ZOOM"
                  :step="STEP"
                  class="flex-1 accent-primary"
                  @input="setZoom(Number(($event.target as HTMLInputElement).value))"
                />
                <span class="text-xs tabular-nums text-muted-foreground w-9 text-right">{{ Math.round(zoomLevel * 100) }}%</span>
              </div>
              <div class="setting-hint">{{ $t('settings.zoomLevelHint') }}</div>
            </div>
            <div class="setting-cell">
              <div class="setting-label">{{ $t('settings.minColumnWidth') }}</div>
              <div class="flex items-center gap-2">
                <input
                  type="number"
                  :value="minColumnWidth"
                  min="200"
                  step="10"
                  class="form-input w-24 tabular-nums"
                  @change="setMinColumnWidth(Number(($event.target as HTMLInputElement).value))"
                />
                <span class="text-[11px] text-muted-foreground">px</span>
              </div>
              <div class="setting-hint">{{ $t('settings.minColumnWidthHint') }}</div>
            </div>
          </div>
        </section>

        <!-- ====== 渠道 ====== -->
        <section v-show="activeTab === 'channels'">
          <h2 class="section-title">{{ $t('settings.channels') }}</h2>
          <p class="text-xs text-muted-foreground mb-3 leading-relaxed">
            {{ $t('settings.channelDesc1') }}{{ $t('settings.channelDesc2') }}
          </p>

          <!-- 双列渠道 -->
          <div class="settings-grid">
            <!-- 左列：会话渠道 -->
            <div>
              <div class="chain-title">{{ $t('settings.defaultSessionChannel') }}</div>
              <div class="chain-list">
                <div
                  v-for="ch in sessionChannels()"
                  :key="ch.id"
                  class="chain-item"
                  :class="{
                    'chain-item-active': (defaultSessionChannel ?? OFFICIAL_CHANNEL_ID) === ch.id,
                    'opacity-50': !ch.enabled,
                  }"
                  @click="ch.enabled && setDefaultSessionChannel(ch.id === OFFICIAL_CHANNEL_ID ? null : ch.id)"
                >
                  <div class="chain-content">
                    <div class="chain-row-1">
                      <span class="truncate font-medium text-xs">{{ builtinChannelName(ch) }}</span>
                      <span v-if="isBuiltinChannel(ch.id)" class="text-[10px] text-muted-foreground/70 shrink-0" v-tooltip="$t(ch.id === OFFICIAL_CHANNEL_ID ? 'channel.followCliHint' : 'channel.officialDirectHint')">
                        <span class="i-carbon-information w-3 h-3 inline-block align-text-bottom" />
                      </span>
                      <div class="chain-actions">
                        <button v-if="!isBuiltinChannel(ch.id)" :class="['form-toggle-sm', { on: ch.enabled }]" @click.stop="setChannelEnabled(ch.id, !ch.enabled)"><span class="form-toggle-knob" /></button>
                        <template v-if="!isBuiltinChannel(ch.id)">
                          <button class="icon-btn icon-btn-sm icon-btn-ghost" v-tooltip="$t('common.edit')" @click.stop="editing = ch"><span class="i-carbon-edit w-3 h-3" /></button>
                          <button class="icon-btn icon-btn-sm icon-btn-ghost icon-btn-danger" v-tooltip="$t('common.delete')" @click.stop="onDelete(ch)"><span class="i-carbon-trash-can w-3 h-3" /></button>
                        </template>
                        <button v-else-if="ch.id === OFFICIAL_CHANNEL_ID" class="icon-btn icon-btn-sm icon-btn-ghost" v-tooltip="$t('settings.officialDefaults.edit')" @click.stop="editingOfficial = ch"><span class="i-carbon-edit w-3 h-3" /></button>
                      </div>
                    </div>
                    <div class="chain-row-2">
                      <template v-if="ch.id !== OFFICIAL_CHANNEL_ID">
                        <span v-if="ch.baseUrl" class="font-mono truncate">{{ ch.baseUrl }}</span>
                      </template>
                      <span v-else class="text-muted-foreground/60 italic">
                        {{ cliEnvTarget.kind === 'third-party' && cliEnvTarget.host ? `→ ${cliEnvTarget.host}` : $t('channel.cliTargetOfficial') }}
                      </span>
                      <span v-if="ch.defaultModel" class="chain-model-tag" v-tooltip="$t('settings.channelForm.defaultModelLabel')">{{ ch.defaultModel }}</span>
                      <span v-if="ch.defaultEffort" class="chain-model-tag" v-tooltip="$t('settings.channelForm.defaultEffortLabel')">{{ ch.defaultEffort }}</span>
                      <span v-if="ch.agentModel" class="chain-model-tag">{{ ch.agentModel }}</span>
                      <span class="ml-auto shrink-0 flex items-center gap-1.5">
                        <template v-if="probing[ch.id]"><span class="i-carbon-renew w-2.5 h-2.5 animate-spin" /></template>
                        <template v-else-if="probeResults[ch.id]">
                          <span class="inline-block w-1.5 h-1.5 rounded-full" :class="probeResults[ch.id].online ? 'bg-green-600' : 'bg-destructive'" />
                          <span v-if="probeResults[ch.id].online && probeResults[ch.id].models.length" v-tooltip="probeResults[ch.id].models.join('\n')">{{ probeResults[ch.id].models.length }} models</span>
                          <span v-else-if="!probeResults[ch.id].online">{{ probeResults[ch.id].status === 'auth_error' ? '401' : probeResults[ch.id].status }}</span>
                          <span v-if="probeResults[ch.id].latencyMs" class="text-muted-foreground/50">{{ probeResults[ch.id].latencyMs }}ms</span>
                        </template>
                        <button v-if="!isBuiltinChannel(ch.id)" class="icon-btn icon-btn-sm icon-btn-ghost" v-tooltip="$t('settings.probeChannel')" @click.stop="probeChannel(ch.id)"><span class="i-carbon-activity w-3 h-3" /></button>
                      </span>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <!-- 右列：Agent 渠道 -->
            <div>
              <div class="chain-title">{{ $t('settings.defaultAgentChannel') }}</div>
              <div class="chain-list">
                <!-- 跟随 CLI -->
                <div
                  class="chain-item"
                  :class="{ 'chain-item-active': agentChannelId === OFFICIAL_CHANNEL_ID }"
                  @click="onAgentChannelChange(OFFICIAL_CHANNEL_ID)"
                >
                  <div class="chain-content">
                    <div class="chain-row-1">
                      <span class="truncate font-medium text-xs">{{ $t('channel.official') }}</span>
                      <span class="text-[10px] text-muted-foreground/70 shrink-0" v-tooltip="$t('channel.followCliHint')"><span class="i-carbon-information w-3 h-3 inline-block align-text-bottom" /></span>
                    </div>
                    <div class="chain-row-2">
                      <span class="text-muted-foreground/60 italic">{{ cliEnvTarget.kind === 'third-party' && cliEnvTarget.host ? `→ ${cliEnvTarget.host}` : $t('channel.cliTargetOfficial') }}</span>
                      <span class="chain-model-tag">haiku</span>
                    </div>
                  </div>
                </div>
                <!-- 官方(直连) -->
                <div
                  class="chain-item"
                  :class="{ 'chain-item-active': agentChannelId === OFFICIAL_DIRECT_CHANNEL_ID }"
                  @click="onAgentChannelChange(OFFICIAL_DIRECT_CHANNEL_ID)"
                >
                  <div class="chain-content">
                    <div class="chain-row-1">
                      <span class="truncate font-medium text-xs">{{ $t('channel.officialDirect') }}</span>
                      <span class="text-[10px] text-muted-foreground/70 shrink-0" v-tooltip="$t('channel.officialDirectHint')"><span class="i-carbon-information w-3 h-3 inline-block align-text-bottom" /></span>
                    </div>
                    <div class="chain-row-2">
                      <span class="text-muted-foreground/60 italic">OAuth</span>
                      <span class="chain-model-tag">haiku</span>
                    </div>
                  </div>
                </div>
                <!-- 其他启用的渠道 -->
                <div
                  v-for="ch in agentChannelCards"
                  :key="ch.id"
                  class="chain-item"
                  :class="{
                    'chain-item-active': agentChannelId === ch.id,
                    'opacity-50': !ch.enabled,
                  }"
                  @click="ch.enabled && onAgentChannelChange(ch.id)"
                >
                  <div class="chain-content">
                    <div class="chain-row-1">
                      <span class="truncate font-medium text-xs">{{ ch.name }}</span>
                      <div class="chain-actions">
                        <button v-if="ch.scope === 'agent-only'" :class="['form-toggle-sm', { on: ch.enabled }]" @click.stop="setChannelEnabled(ch.id, !ch.enabled)"><span class="form-toggle-knob" /></button>
                        <button v-if="ch.scope === 'agent-only'" class="icon-btn icon-btn-sm icon-btn-ghost" v-tooltip="$t('common.edit')" @click.stop="editing = ch"><span class="i-carbon-edit w-3 h-3" /></button>
                      </div>
                    </div>
                    <div class="chain-row-2">
                      <span v-if="ch.baseUrl" class="font-mono truncate">{{ ch.baseUrl }}</span>
                      <span class="chain-model-tag">{{ agentModelTag(ch) }}</span>
                      <span class="ml-auto shrink-0 flex items-center gap-1.5">
                        <template v-if="probing[ch.id]"><span class="i-carbon-renew w-2.5 h-2.5 animate-spin" /></template>
                        <template v-else-if="probeResults[ch.id]">
                          <span class="inline-block w-1.5 h-1.5 rounded-full" :class="probeResults[ch.id].online ? 'bg-green-600' : 'bg-destructive'" />
                          <span v-if="probeResults[ch.id].latencyMs" class="text-muted-foreground/50">{{ probeResults[ch.id].latencyMs }}ms</span>
                        </template>
                      </span>
                    </div>
                  </div>
                </div>
              </div>
              <!-- Apple FM 分隔区 -->
              <template v-if="hasAppleFm">
                <div class="border-t border-border my-2" />
                <div class="chain-list">
                  <div
                    class="chain-item"
                    :class="{ 'chain-item-active': isAppleFmAgent }"
                    @click="toggleAppleFmAgent"
                  >
                    <div class="chain-content">
                      <div class="chain-row-1">
                        <span class="truncate font-medium text-xs">Apple FM</span>
                      </div>
                      <div class="chain-row-2">
                        <span class="text-muted-foreground/60 italic">{{ $t('settings.appleFmLocal') }}</span>
                        <span class="chain-model-tag">system</span>
                        <span class="ml-auto shrink-0 flex items-center gap-1.5">
                          <template v-if="probing[APPLE_FM_CHANNEL_ID]"><span class="i-carbon-renew w-2.5 h-2.5 animate-spin" /></template>
                          <template v-else-if="probeResults[APPLE_FM_CHANNEL_ID]">
                            <span class="inline-block w-1.5 h-1.5 rounded-full" :class="probeResults[APPLE_FM_CHANNEL_ID].online ? 'bg-green-600' : 'bg-destructive'" />
                            <span v-if="probeResults[APPLE_FM_CHANNEL_ID].latencyMs" class="text-muted-foreground/50">{{ probeResults[APPLE_FM_CHANNEL_ID].latencyMs }}ms</span>
                          </template>
                          <button class="icon-btn icon-btn-sm icon-btn-ghost" v-tooltip="$t('settings.probeChannel')" @click.stop="probeChannel(APPLE_FM_CHANNEL_ID)"><span class="i-carbon-activity w-3 h-3" /></button>
                        </span>
                      </div>
                    </div>
                  </div>
                </div>
              </template>
              <div class="text-[10.5px] text-muted-foreground mt-1.5">{{ $t('settings.defaultAgentChannelHint') }}</div>
            </div>
          </div>

          <ChannelForm
            v-if="editing"
            :key="editing === 'new' ? '__new__' : editing.id"
            :channel="editing === 'new' ? null : editing"
            class="mt-3"
            @saved="onSaved"
            @cancel="editing = null"
          />

          <OfficialDefaultsForm
            v-if="editingOfficial"
            :channel="editingOfficial"
            class="mt-3"
            @saved="editingOfficial = null"
            @cancel="editingOfficial = null"
          />

          <div class="flex items-center gap-2 mt-3">
            <button
              v-if="!editing"
              class="px-2.5 py-1 text-xs rounded-md bg-primary text-primary-foreground hover:shadow-paper transition-shadow"
              @click="editing = 'new'"
            >
              {{ $t('settings.addChannel') }}
            </button>
            <button
              class="px-2.5 py-1 text-xs rounded-md text-muted-foreground border border-border hover:text-foreground hover:bg-muted transition-colors"
              :disabled="ccSwitchScanning"
              @click="onScanCcSwitch"
            >
              <span v-if="ccSwitchScanning" class="i-carbon-renew w-3 h-3 animate-spin mr-1 inline-block align-[-2px]" />
              {{ ccSwitchScanning ? $t('settings.ccSwitchScanning') : $t('settings.ccSwitchImport') }}
            </button>
            <button
              class="px-2.5 py-1 text-xs rounded-md text-muted-foreground border border-border hover:text-foreground hover:bg-muted transition-colors"
              @click="onReveal"
            >
              {{ $t('common.openConfigDir') }}
            </button>
          </div>

          <!-- CC Switch 导入列表 -->
          <div v-if="ccSwitchOpen && ccSwitchProviders.length" class="mt-3 rounded-md border border-border bg-popover p-3">
            <div class="flex items-center justify-between mb-2">
              <span class="text-xs font-medium">CC Switch ({{ ccSwitchProviders.length }})</span>
              <div class="flex items-center gap-2">
                <button class="text-[10px] text-muted-foreground hover:text-foreground" @click="toggleCcSwitchAll">{{ $t('settings.ccSwitchSelectAll') }}</button>
                <button
                  class="px-2 py-0.5 text-[11px] rounded bg-primary text-primary-foreground disabled:opacity-40"
                  :disabled="ccSwitchSelected.size === 0"
                  @click="onImportCcSwitch"
                >
                  {{ $t('settings.ccSwitchImportSelected') }} ({{ ccSwitchSelected.size }})
                </button>
                <button class="icon-btn icon-btn-sm icon-btn-ghost" @click="ccSwitchOpen = false"><span class="i-carbon-close w-3 h-3" /></button>
              </div>
            </div>
            <div class="flex flex-col gap-1 max-h-48 overflow-y-auto">
              <label
                v-for="p in ccSwitchProviders"
                :key="p.id"
                class="flex items-center gap-2 px-2 py-1 rounded text-xs hover:bg-muted/50 cursor-pointer"
                :class="{ 'opacity-50': p.alreadyImported }"
              >
                <input
                  type="checkbox"
                  :checked="ccSwitchSelected.has(p.id)"
                  :disabled="p.alreadyImported"
                  class="accent-primary"
                  @change="p.alreadyImported ? null : (ccSwitchSelected.has(p.id) ? ccSwitchSelected.delete(p.id) : ccSwitchSelected.add(p.id))"
                />
                <span class="font-medium truncate">{{ p.name }}</span>
                <span v-if="p.isCurrent" class="text-[10px] text-green-600 shrink-0">{{ $t('settings.ccSwitchCurrent') }}</span>
                <span v-if="p.alreadyImported" class="text-[10px] text-muted-foreground shrink-0">{{ $t('settings.ccSwitchAlready') }}</span>
                <span v-if="p.baseUrl" class="ml-auto text-[10px] font-mono text-muted-foreground truncate max-w-48">{{ p.baseUrl }}</span>
              </label>
            </div>
          </div>
          <p v-else-if="ccSwitchOpen && !ccSwitchProviders.length && !ccSwitchScanning" class="mt-2 text-xs text-muted-foreground">{{ $t('settings.ccSwitchEmpty') }}</p>
        </section>

        <!-- ====== Agent ====== -->
        <section v-show="activeTab === 'agent'">
          <h2 class="section-title">{{ $t('settings.agent') }}</h2>
          <p class="text-xs text-muted-foreground mb-3 leading-relaxed">
            {{ $t('settings.agentDesc') }}
          </p>
          <div class="settings-grid">
            <div v-for="a in agentKeys" :key="a.key" class="agent-item">
              <div class="flex-1 min-w-0">
                <div class="text-xs font-medium">{{ $t(a.label) }}</div>
                <div class="ext-tags" style="margin-top:3px">
                  <span v-if="a.tag === 'recommended'" class="ext-tag recommended">{{ $t('settings.extTagRecommended') }}</span>
                  <span v-else-if="a.tag === 'beginner'" class="ext-tag recommended">{{ $t('settings.extTagBeginner') }}</span>
                  <span v-else-if="a.tag === 'asNeeded'" class="ext-tag neutral">{{ $t('settings.extTagAsNeeded') }}</span>
                </div>
                <div class="text-[11px] text-muted-foreground mt-0.5">{{ $t(a.desc) }}</div>
                <select
                  v-if="isAgentEnabled(a.key)"
                  class="form-input text-[11px] font-mono mt-1.5 w-auto max-w-56 h-6 py-0"
                  :value="agentPreferences[a.key]?.preferredChannel && agentPreferences[a.key]?.preferredModel ? `${agentPreferences[a.key].preferredChannel}:${agentPreferences[a.key].preferredModel}` : ''"
                  @change="{ const v = ($event.target as HTMLSelectElement).value; if (!v) { setAgentFeatureModel(a.key, null, null) } else { const [ch, ...rest] = v.split(':'); setAgentFeatureModel(a.key, ch, rest.join(':')) } }"
                >
                  <option value="">{{ $t('settings.agentAutoChannel') }}</option>
                  <option v-for="opt in agentModelOptions()" :key="`${opt.channel}:${opt.model}`" :value="`${opt.channel}:${opt.model}`">
                    {{ opt.channelName }} / {{ opt.model }}
                  </option>
                </select>
              </div>
              <button
                :class="['form-toggle', { on: isAgentEnabled(a.key) }]"
                @click="toggleAgent(a.key)"
              >
                <span class="form-toggle-knob" />
              </button>
            </div>
          </div>

          <!-- 会话落盘（全局行为，非单项能力） -->
          <div class="agent-item mt-3">
            <div class="flex-1 min-w-0">
              <div class="text-xs font-medium">{{ $t('settings.agentSessionPersist') }}</div>
              <div class="text-[11px] text-muted-foreground mt-0.5">{{ $t('settings.agentSessionPersistDesc') }}</div>
              <button
                class="text-[11px] text-primary hover:underline mt-1"
                @click="revealAgentSessionDir"
              >{{ $t('common.revealDir') }}</button>
            </div>
            <button
              :class="['form-toggle', { on: agentSessionPersist }]"
              @click="toggleAgentSessionPersist"
            >
              <span class="form-toggle-knob" />
            </button>
          </div>

          <!-- Agent 操作栏 -->
          <div class="mt-6 pt-4 border-t border-border flex items-center gap-4">
            <button
              class="px-2.5 py-1 text-xs rounded-md border border-border text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
              :disabled="agentTesting"
              @click="onTestAgent"
            >
              <span v-if="agentTesting" class="i-carbon-renew w-3 h-3 animate-spin mr-1 inline-block align-[-2px]" />
              {{ $t('settings.agentTest') }}
            </button>
            <button
              class="text-xs text-primary hover:underline"
              @click="showAgentLogs = true; loadAgentLogs()"
            >{{ $t('settings.agentLogs') }}</button>
          </div>
          <!-- 测试结果 -->
          <div v-if="agentTestResult" class="mt-2 px-3 py-2 rounded-md border text-[11px]"
            :class="agentTestResult.success ? 'border-emerald-500/30 bg-emerald-500/5' : 'border-destructive/30 bg-destructive/5'"
          >
            <div v-if="agentTestResult.success" class="flex items-center gap-3 text-foreground">
              <span class="text-emerald-600 dark:text-emerald-400 font-medium">OK</span>
              <span class="text-muted-foreground">{{ agentLogChannelLabel(agentTestResult.channelId) }}</span>
              <span class="font-mono text-muted-foreground">{{ agentTestResult.model }}</span>
              <span class="font-mono text-muted-foreground">{{ agentTestResult.durationMs >= 1000 ? `${(agentTestResult.durationMs / 1000).toFixed(1)}s` : `${agentTestResult.durationMs}ms` }}</span>
              <span v-if="agentTestResult.inputTokens" class="font-mono text-muted-foreground">↑{{ agentTestResult.inputTokens }} ↓{{ agentTestResult.outputTokens }}</span>
            </div>
            <div v-else class="text-destructive">{{ agentTestResult.error }}</div>
          </div>
        </section>

        <!-- ====== Claude Code 配置 ====== -->
        <section v-show="activeTab === 'claude-code'" class="cli-section">
          <ClaudeCodeSettings />
        </section>

        <!-- ====== 权限体检 ====== -->
        <section v-if="isMac" v-show="activeTab === 'permissions'">
          <h2 class="section-title">{{ $t('settings.permCheck.title') }}</h2>
          <PermissionsPanel />
        </section>

        <!-- ====== 扩展 ====== -->
        <section v-show="activeTab === 'extensions'">
          <h2 class="section-title">{{ $t('settings.extensions') }}</h2>
          <p class="text-xs text-muted-foreground mb-3 leading-relaxed">
            {{ $t('settings.extensionsDesc') }}
          </p>
          <div class="flex flex-col gap-3">
            <!-- MCP Server 注册 -->
            <div class="mcp-card">
              <div class="flex items-center gap-2">
                <span class="i-carbon-plug w-3.5 h-3.5 text-muted-foreground" />
                <span class="text-[11.5px] font-medium">{{ $t('settings.mcp.title') }}</span>
                <span :class="['mcp-status', { active: mcpRegistered }]">
                  {{ mcpRegistered ? $t('settings.mcp.registered') : $t('settings.mcp.notRegistered') }}
                </span>
                <button
                  :class="['form-toggle ml-auto', { on: mcpRegistered }]"
                  :disabled="mcpLoading"
                  @click="toggleMcp"
                >
                  <span class="form-toggle-knob" />
                </button>
              </div>
              <div class="ext-tags">
                <span class="ext-tag recommended">{{ $t('settings.extTagRecommended') }}</span>
              </div>
              <p class="text-[10.5px] text-muted-foreground mt-1 leading-snug">
                {{ $t('settings.mcp.description') }}
              </p>
              <div class="mt-1.5 flex flex-col gap-1">
                <div class="ext-example">
                  <span class="i-carbon-search w-3 h-3 shrink-0 opacity-50" />
                  <span>{{ $t('settings.mcp.exampleSearch') }}</span>
                </div>
                <div class="ext-example">
                  <span class="i-carbon-time w-3 h-3 shrink-0 opacity-50" />
                  <span>{{ $t('settings.mcp.exampleRoutine') }}</span>
                </div>
              </div>
            </div>
            <TurnSignalCard />
            <!-- HTML 增强渲染 -->
            <div class="mcp-card">
              <div class="flex items-center gap-2">
                <span class="i-carbon-code w-3.5 h-3.5 text-muted-foreground" />
                <span class="text-[11.5px] font-medium">{{ $t('settings.htmlVisual') }}</span>
                <button
                  :class="['form-toggle ml-auto', { on: htmlVisualEnabled }]"
                  @click="htmlVisualEnabled = !htmlVisualEnabled"
                >
                  <span class="form-toggle-knob" />
                </button>
              </div>
              <div class="ext-tags">
                <span class="ext-tag recommended">{{ $t('settings.extTagRecommended') }}</span>
                <span class="ext-tag warn">{{ $t('settings.extTagTokenCost') }}</span>
              </div>
              <p class="text-[10.5px] text-muted-foreground mt-1 leading-snug">
                {{ $t('settings.htmlVisualDesc') }}
              </p>
            </div>
          </div>
        </section>

        <!-- ====== 实验室 ====== -->
        <section v-show="activeTab === 'lab'">
          <h2 class="section-title">{{ $t('settings.lab') }}</h2>
          <p class="text-xs text-muted-foreground mb-3 leading-relaxed">
            {{ $t('settings.labDesc') }}
          </p>
          <!-- 消息虚拟化阈值 -->
          <div class="mcp-card mb-3">
            <div class="flex items-center gap-2">
              <span class="i-carbon-layers w-3.5 h-3.5 text-muted-foreground" />
              <span class="text-[11.5px] font-medium">{{ $t('settings.virtualizationThreshold') }}</span>
              <input
                type="number"
                min="0"
                class="ml-auto w-16 text-[11px] px-2 py-0.5 rounded border border-border bg-transparent text-right tabular-nums"
                v-model.number="virtualizationThreshold"
              />
            </div>
            <p class="text-[10.5px] text-muted-foreground mt-1 leading-snug">
              {{ $t('settings.virtualizationThresholdDesc') }}
            </p>
          </div>
          <div class="iframe-zone">
            <span class="iframe-badge">IFRAME</span>
            <AgentIframeDemo />
          </div>
        </section>

        <!-- ====== 系统 ====== -->
        <section v-show="activeTab === 'system'">
          <h2 class="section-title">{{ $t('settings.system') }}</h2>

          <!-- 更新 -->
          <div class="mcp-card mb-3">
            <div class="flex items-center gap-2">
              <span class="i-carbon-upgrade w-3.5 h-3.5 text-muted-foreground" />
              <span class="text-[11.5px] font-medium">{{ $t('settings.updateCurrent') }}</span>
              <span class="text-[11px] font-mono text-muted-foreground">v{{ appVersion }}</span>
              <span v-if="updateStatus === 'up-to-date'" class="text-[10px] text-emerald-600 dark:text-emerald-400">{{ $t('settings.updateUpToDate') }}</span>
              <span v-if="updateStatus === 'error'" class="text-[10px] text-destructive truncate max-w-48" :title="updateError">{{ $t('settings.updateError') }}</span>
              <div class="ml-auto flex items-center gap-2">
                <template v-if="updateStatus === 'available'">
                  <span class="text-[11px] text-primary font-medium">{{ $t('settings.updateAvailable', { version: updateVersion }) }}</span>
                  <button
                    class="px-2 py-0.5 text-[11px] rounded bg-primary text-primary-foreground hover:shadow-paper transition-shadow"
                    @click="downloadAndInstall"
                  >{{ $t('settings.updateInstall') }}</button>
                </template>
                <template v-else-if="updateStatus === 'downloading'">
                  <span class="text-[11px] text-muted-foreground font-mono">{{ $t('settings.updateDownloading', { progress: downloadProgress }) }}</span>
                </template>
                <template v-else-if="updateStatus === 'restarting'">
                  <span class="text-[11px] text-primary font-medium">{{ $t('settings.updateRestarting') }}</span>
                </template>
                <template v-else>
                  <button
                    class="px-2 py-0.5 text-[11px] rounded border border-border text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
                    :disabled="updateStatus === 'checking'"
                    @click="checkForUpdate"
                  >
                    <span v-if="updateStatus === 'checking'" class="i-carbon-renew w-3 h-3 animate-spin mr-1 inline-block align-[-2px]" />
                    {{ updateStatus === 'checking' ? $t('settings.updateChecking') : $t('settings.updateCheck') }}
                  </button>
                </template>
              </div>
            </div>

            <!-- 更新通道：Nightly 为每日构建，默认永远是稳定版 -->
            <div class="flex items-center gap-2 mt-2 pt-2 border-t border-border/60">
              <span class="i-carbon-branch w-3.5 h-3.5 text-muted-foreground" />
              <span class="text-[11.5px] font-medium">{{ $t('settings.updateChannel') }}</span>
              <span class="text-[10.5px] text-muted-foreground">{{ $t('settings.updateChannelDesc') }}</span>
              <div class="ml-auto flex items-center gap-1">
                <button
                  v-for="c in (['stable', 'nightly'] as const)"
                  :key="c"
                  class="px-2 py-0.5 text-[11px] rounded border transition-colors"
                  :class="updateChannel === c
                    ? 'border-primary bg-primary text-primary-foreground'
                    : 'border-border text-muted-foreground hover:text-foreground hover:bg-muted'"
                  @click="switchUpdateChannel(c)"
                >{{ $t(`settings.updateChannel_${c}`) }}</button>
              </div>
            </div>
          </div>

          <div class="settings-grid">
            <!-- 菜单栏（macOS 专属：launchd Helper 架构） -->
            <div v-if="isMac" class="setting-group">
              <div class="setting-group-header">
                <span class="i-carbon-menu w-3.5 h-3.5" />
                {{ $t('settings.groupTray') }}
              </div>
              <div class="setting-row">
                <div class="setting-row-main">
                  <div class="setting-label">{{ $t('settings.trayAutostart') }}</div>
                  <div class="setting-hint" :class="{ 'text-red-500': trayToggleFailed }">
                    {{ trayToggleFailed ? $t('settings.trayLaunchFail') : $t('settings.trayAutostartHint') }}
                  </div>
                </div>
                <button :class="['form-toggle', { on: trayEnabled }]" @click="toggleTrayEnabled">
                  <span class="form-toggle-knob" />
                </button>
              </div>
              <div class="setting-row" :class="{ 'opacity-40 pointer-events-none': !trayEnabled }">
                <div class="setting-row-main">
                  <div class="setting-label">{{ $t('settings.trayTitle') }}</div>
                  <div class="setting-hint">{{ $t('settings.trayTitleHint') }}</div>
                </div>
                <div class="flex items-center gap-1.5 flex-wrap justify-end">
                  <button
                    v-for="slot in traySlotOptions"
                    :key="slot.key"
                    :class="['setting-chip', { on: isTraySlotActive(slot.key) }]"
                    @click="toggleTraySlot(slot)"
                  >{{ slot.label }}</button>
                </div>
              </div>
            </div>
            <!-- 桌面小组件（macOS 专属：WidgetKit） -->
            <div v-if="isMac" class="setting-group">
              <div class="setting-group-header">
                <span class="i-carbon-apps w-3.5 h-3.5" />
                {{ $t('settings.groupWidget') }}
              </div>
              <div class="setting-row">
                <div class="setting-row-main">
                  <div class="setting-label">{{ $t('settings.widgetDayBoundary') }}</div>
                  <div class="setting-hint">{{ $t('settings.widgetDayBoundaryHint') }}</div>
                </div>
                <select
                  :value="widgetDayStart"
                  class="form-select w-44 shrink-0"
                  @change="setWidgetDayStart(Number(($event.target as HTMLSelectElement).value))"
                >
                  <option v-for="h in 24" :key="h - 1" :value="h - 1">{{ $t('settings.widgetHourOption', { h: h - 1 }) }}</option>
                  <option :value="-1">{{ $t('settings.widgetRolling24h') }}</option>
                </select>
              </div>
              <div class="setting-row">
                <div class="setting-row-main">
                  <div class="setting-label">{{ $t('settings.widgetMonthBoundary') }}</div>
                  <div class="setting-hint">{{ $t('settings.widgetMonthBoundaryHint') }}</div>
                </div>
                <select
                  :value="widgetMonthMode"
                  class="form-select w-44 shrink-0"
                  @change="setWidgetMonthMode(($event.target as HTMLSelectElement).value)"
                >
                  <option value="natural">{{ $t('settings.widgetNaturalMonth') }}</option>
                  <option value="rolling">{{ $t('settings.widgetRolling30') }}</option>
                </select>
              </div>
            </div>
            <!-- 定时任务（全宽收底） -->
            <div class="setting-group setting-group-wide">
              <div class="setting-group-header">
                <span class="i-carbon-alarm w-3.5 h-3.5" />
                {{ $t('settings.groupRoutine') }}
              </div>
              <div class="setting-row">
                <div class="setting-row-main">
                  <div class="setting-label">{{ $t('settings.routineWake') }}</div>
                  <div class="setting-hint">{{ $t('settings.routineWakeHint') }}</div>
                </div>
                <div class="flex flex-col items-end gap-1.5 shrink-0">
                  <label class="flex items-center gap-2 cursor-pointer text-[12px]">
                    <input
                      type="radio"
                      name="wake-policy"
                      value="passive"
                      :checked="wakePolicy === 'passive'"
                      class="accent-primary"
                      @change="setWakePolicy('passive')"
                    />
                    {{ $t('settings.routineWakePassive') }}
                  </label>
                  <label class="flex items-center gap-2 cursor-pointer text-[12px]">
                    <input
                      type="radio"
                      name="wake-policy"
                      value="active"
                      :checked="wakePolicy === 'active'"
                      class="accent-primary"
                      @change="setWakePolicy('active')"
                    />
                    {{ $t('settings.routineWakeActive') }}
                  </label>
                  <span v-if="wakePolicy === 'active'" class="text-[11px] text-muted-foreground">{{ $t('settings.routineWakeActiveSub') }}</span>
                </div>
              </div>
              <div v-if="wakeAuthorized" class="setting-row">
                <div class="setting-row-main">
                  <div class="setting-label">{{ $t('settings.routineWakeAuthTitle') }}</div>
                  <div class="setting-hint">{{ $t('settings.routineWakeAuthorized') }}</div>
                </div>
                <button
                  class="text-[11.5px] text-muted-foreground underline underline-offset-2 hover:text-foreground transition-colors shrink-0"
                  @click="removeWakeAuthorization"
                >
                  {{ $t('settings.routineWakeRemoveAuth') }}
                </button>
              </div>
            </div>
          </div>
        </section>

      </div>
    </div>
    </div>

    <!-- Agent 日志弹窗 -->
  <div
    v-if="showAgentLogs"
    class="fixed inset-0 z-70 grid place-items-center"
    style="background: rgba(70, 45, 20, 0.18)"
    @mousedown.self="showAgentLogs = false"
  >
    <div class="w-[720px] max-w-[90vw] max-h-[80vh] rounded-lg bg-popover border border-border shadow-paper-lifted flex flex-col">
      <div class="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
        <h3 class="text-sm font-medium">{{ $t('settings.agentLogs') }}</h3>
        <div class="flex items-center gap-3">
          <button
            v-if="agentLogs.length"
            class="text-[11px] text-muted-foreground hover:text-destructive transition-colors"
            @click="clearAgentLogs"
          >{{ $t('common.clear') }}</button>
          <button
            class="text-[11px] text-muted-foreground hover:text-foreground transition-colors"
            @click="loadAgentLogs"
          >↻</button>
          <button
            class="i-carbon-close w-4 h-4 text-muted-foreground hover:text-foreground transition-colors"
            @click="showAgentLogs = false"
          />
        </div>
      </div>

      <div class="flex-1 overflow-auto">
        <div v-if="agentLogsLoading" class="text-xs text-muted-foreground py-8 text-center">
          {{ $t('common.loading') }}
        </div>
        <template v-else-if="agentLogs.length">
          <div class="flex gap-4 px-4 py-2 text-[11px] text-muted-foreground border-b border-border bg-muted/30">
            <span>{{ $t('settings.agentLogsTotal', { n: agentLogsStats.total }) }}</span>
            <span>{{ $t('settings.agentLogsSuccess', { n: agentLogsStats.successCount }) }}</span>
            <span>↑{{ agentLogsStats.totalInput.toLocaleString() }} ↓{{ agentLogsStats.totalOutput.toLocaleString() }} tokens</span>
          </div>
          <table class="agent-logs-table">
            <thead>
              <tr>
                <th>{{ $t('settings.agentLogsTime') }}</th>
                <th>{{ $t('settings.agentLogsFeature') }}</th>
                <th>{{ $t('settings.agentLogsChannel') }}</th>
                <th>{{ $t('settings.agentLogsModel') }}</th>
                <th class="text-right">{{ $t('settings.agentLogsDuration') }}</th>
                <th class="text-right">Tokens</th>
                <th>{{ $t('settings.agentLogsStatus') }}</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(log, i) in agentLogsSorted" :key="i" :class="{ 'opacity-60': !log.success }">
                <td class="font-mono whitespace-nowrap">{{ new Date(log.timestamp).toLocaleString() }}</td>
                <td>{{ $t(`settings.agentFeature_${log.feature}`, log.feature) }}</td>
                <td>{{ agentLogChannelLabel(log.channelId) }}</td>
                <td class="font-mono truncate max-w-32" :title="log.model">{{ log.model }}</td>
                <td class="text-right font-mono">{{ log.durationMs >= 1000 ? `${(log.durationMs / 1000).toFixed(1)}s` : `${log.durationMs}ms` }}</td>
                <td class="text-right font-mono">
                  <template v-if="log.inputTokens || log.outputTokens">↑{{ log.inputTokens }} ↓{{ log.outputTokens }}</template>
                  <span v-else class="text-muted-foreground">—</span>
                </td>
                <td>
                  <span v-if="log.success" class="text-emerald-600 dark:text-emerald-400">OK</span>
                  <span v-else class="block max-w-72 truncate text-destructive" :title="log.error">
                    {{ $t('settings.agentLogsFailed') }}<template v-if="log.error"> · {{ log.error }}</template>
                  </span>
                </td>
                <td>
                  <button
                    v-if="log.sessionId"
                    class="text-primary hover:underline whitespace-nowrap"
                    @click="viewingSession = log.sessionId!"
                  >{{ $t('common.viewSession') }}</button>
                </td>
              </tr>
            </tbody>
          </table>
        </template>
        <div v-else class="text-xs text-muted-foreground py-8 text-center">
          {{ $t('settings.agentLogsEmpty') }}
        </div>
      </div>
    </div>
  </div>

  <!-- 完整会话浮层 -->
  <SystemSessionViewer
    v-if="viewingSession"
    :session-id="viewingSession"
    @close="viewingSession = null"
  />
  </div>
</template>

<style scoped>
/* 侧栏 */
.side-nav {
  width: 140px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  padding: 14px 8px;
  background: var(--background);
}
.side-title {
  font-size: 14px;
  font-weight: 600;
  padding: 0 8px;
  margin-bottom: 14px;
  display: flex;
  align-items: center;
  gap: 6px;
}
.side-item {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  padding: 6px 10px;
  font-size: 12px;
  text-align: left;
  color: var(--muted-foreground);
  border-radius: var(--radius);
  transition: all 0.15s;
  margin-bottom: 2px;
  border: none;
  background: none;
  cursor: pointer;
}
.side-item:hover {
  color: var(--foreground);
  background: var(--muted);
}
.side-item.active {
  color: var(--primary);
  font-weight: 500;
  background: var(--card);
  box-shadow: var(--shadow-paper);
}
/* 更新可用提示点:与 ActivityBar 设置图标的绿点同源状态 */
.side-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--primary);
  margin-left: auto;
  flex-shrink: 0;
}

/* 内容体：限最大宽度居中，超宽窗口下不再无限拉伸 */
.settings-body {
  padding: 20px;
  max-width: 1040px;
  margin: 0 auto;
  width: 100%;
}

.cli-section {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

/* 分区标题 */
.section-title {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 14px;
}

/* 响应式网格：宽够双列，窄则自动落单列 */
.settings-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));
  gap: 12px;
  align-items: start;
}

/* 设置分组卡片 */
.setting-group {
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card);
  overflow: hidden;
}
.setting-group-wide {
  /* 跨满整行：双列时占两列，单列时自然落一列（span 2 在单列会溢出） */
  grid-column: 1 / -1;
}
.setting-group-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 12px;
  border-bottom: 1px solid var(--border);
  background: var(--muted);
  font-size: 11px;
  font-weight: 600;
  color: var(--muted-foreground);
  letter-spacing: 0.4px;
}
.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  padding: 10px 12px;
  transition: opacity 0.15s;
}
.setting-row + .setting-row {
  border-top: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
}
.setting-row-main {
  flex: 1;
  min-width: 0;
}

/* 多选胶囊 */
.setting-chip {
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 2px 10px;
  font-size: 11.5px;
  color: var(--muted-foreground);
  background: transparent;
  transition: all 0.15s;
}
.setting-chip:hover {
  color: var(--foreground);
}
.setting-chip.on {
  background: color-mix(in srgb, var(--primary) 12%, transparent);
  border-color: var(--primary);
  color: var(--primary);
  font-weight: 500;
}

/* 设置单元：label 在上，控件在下 */
.setting-cell {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.setting-cell-wide { grid-column: 1 / -1; }
.tool-display-options {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}
.tool-display-option {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 3px;
  min-width: 0;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--muted-foreground);
  background: var(--card);
  text-align: left;
  transition: border-color 150ms, background 150ms, color 150ms, transform 150ms;
}
.tool-display-option:hover { color: var(--foreground); transform: translateY(-1px); }
.tool-display-option.active {
  border-color: var(--primary);
  color: var(--foreground);
  background: color-mix(in srgb, var(--primary) 8%, var(--card));
}
.tool-display-option-title { font-size: 11.5px; font-weight: 600; }
.tool-display-option-hint { font-size: 10.5px; line-height: 1.4; }
@media (max-width: 760px) {
  .tool-display-options { grid-template-columns: 1fr; }
}
.setting-label {
  font-size: 12px;
  font-weight: 500;
}
.setting-hint {
  font-size: 11px;
  color: var(--muted-foreground);
  font-weight: 400;
}

/* AI 翻译区 */
.translate-zone {
  padding: 8px 0 4px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.translate-form {
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 10px;
  background: var(--card);
}

/* 子卡片 */
.sub-card {
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 14px;
  margin-bottom: 12px;
  background: var(--card);
}
.sub-card-title {
  font-size: 12px;
  font-weight: 500;
  margin-bottom: 10px;
}

/* iframe 标识 */
.iframe-zone {
  border: 2px dashed var(--accent);
  border-radius: var(--radius);
  position: relative;
  overflow: hidden;
}
.iframe-badge {
  position: absolute;
  top: 0;
  right: 0;
  z-index: 2;
  padding: 2px 10px;
  font-size: 10px;
  font-weight: 600;
  background: var(--accent);
  color: var(--accent-foreground);
  border-radius: 0 0 0 var(--radius);
  letter-spacing: 0.04em;
}

/* 渠道链 */
.chain-title {
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 2px;
}
.chain-hint {
  font-size: 11px;
  color: var(--muted-foreground);
  margin-bottom: 6px;
}
.chain-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.chain-item {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 6px 8px;
  border-radius: var(--radius);
  border: 1px solid transparent;
  cursor: pointer;
  transition: all 0.15s;
  min-height: 46px;
  box-sizing: border-box;
}
.chain-item:hover {
  background: var(--muted);
}
.chain-item-active {
  border-color: var(--primary);
  background: color-mix(in srgb, var(--primary) 6%, transparent);
}
.chain-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.chain-row-1 {
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 22px;
}
.chain-row-2 {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 10px;
  color: var(--muted-foreground);
}
.chain-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-left: auto;
  flex-shrink: 0;
}
.chain-model-tag {
  font-size: 10px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  color: var(--muted-foreground);
  opacity: 0.7;
}
.agent-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--card);
}

/* 渠道标签 */
.channel-chip {
  padding: 0 4px;
  font-size: 9.5px;
  line-height: 16px;
  border: 1px solid var(--primary);
  color: var(--primary);
  border-radius: calc(var(--radius) - 2px);
  flex-shrink: 0;
}

/* Agent 日志表格 */
.agent-logs-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 11px;
}
.agent-logs-table th {
  position: sticky;
  top: 0;
  background: var(--muted);
  padding: 4px 8px;
  text-align: left;
  font-weight: 500;
  color: var(--muted-foreground);
  border-bottom: 1px solid var(--border);
  white-space: nowrap;
}
.agent-logs-table td {
  padding: 3px 8px;
  border-bottom: 1px solid var(--border);
  color: var(--foreground);
}
.agent-logs-table tbody tr:hover {
  background: var(--muted);
}

/* MCP 卡片 */
.mcp-card {
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--card);
}
.mcp-status {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 100px;
  color: var(--muted-foreground);
  border: 1px solid var(--border);
}
.mcp-status.active {
  color: var(--primary);
  border-color: var(--primary);
  background: hsl(var(--primary) / 0.08);
}
.ext-example {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 10.5px;
  color: var(--muted-foreground);
  font-style: italic;
}
.ext-tags {
  display: flex;
  gap: 4px;
  margin-top: 5px;
  flex-wrap: wrap;
}
.ext-tag {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 3px;
  line-height: 1.5;
}
.ext-tag.recommended {
  background: color-mix(in srgb, var(--primary) 12%, transparent);
  color: var(--primary);
}
.ext-tag.neutral {
  background: color-mix(in srgb, var(--muted-foreground) 10%, transparent);
  color: var(--muted-foreground);
}
.ext-tag.warn {
  background: color-mix(in srgb, var(--destructive) 10%, transparent);
  color: var(--destructive);
}
</style>
