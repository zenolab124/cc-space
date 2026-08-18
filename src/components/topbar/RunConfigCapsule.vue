<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionSettings, EffortSetting, EffortLevel } from '@/composables/useSessionSettings'
import type { ResolvedRunConfig } from '@/composables/useRunConfig'
import {
  useChannels,
  refreshChannels,
  channelSupportsEngine,
  OFFICIAL_CHANNEL_ID,
  OFFICIAL_DIRECT_CHANNEL_ID,
  APPLE_FM_CHANNEL_ID,
} from '@/composables/useChannels'
import { useModelOptions } from '@/composables/useModelOptions'
import { useCliDefaults } from '@/composables/useCliDefaults'
import { useUiState } from '@/composables/useUiState'
import { inferModel, effortCapabilities, type ModelInfo } from '@/utils/modelContext'
import { ROLE_DISPLAY, resolveMappedRoles } from '@/utils/modelEnv'
import type { EngineCapsuleConfig } from '@/engines/runConfig'

/**
 * 运行配置胶囊(二期,原型冻结基准 docs/prototypes/run-config-capsule.html):
 * 渠道/模型/强度三段一枚胶囊，常显当前有效值；点哪段从哪层开渐进面板——
 * 强度一列 / 模型两列 / 渠道三列全景(窄列渠道段收起,点任意段直接开全景)。
 * 面板列表只放纯候选项:选中 = 解析值所在项;默认值右侧小字「默认」;
 * 不支持档右侧小字「不支持」(软提示不拦截,能力名单可能随 CLI 更新过时);
 * 覆盖态列头出「重置」清覆盖回跟随。
 */
const props = defineProps<{
  /** 会话覆盖原值(重置钮显隐/顾问开关状态判定) */
  settings?: Pick<SessionSettings, 'modelId' | 'effort' | 'fastMode' | 'channelId' | 'chrome' | 'extraArgs'>
  runConfig?: ResolvedRunConfig
  cwd?: string | null
  /** 标准引擎会话使用同一胶囊，仅由引擎 adapter 提供候选和值。 */
  engineConfig?: EngineCapsuleConfig
  /** 快速模式被供应商自动降级后的非阻断提示。 */
  fastModeNotice?: string | null
  /** 当前模型目录支持从上游重新同步。 */
  modelRefreshable?: boolean
  /** 上游模型目录正在同步。 */
  modelsRefreshing?: boolean
  /** 设置页的默认智能增强配置；仍复用会话三段式交互，但不显示高级参数。 */
  defaultConfig?: {
    engineId: 'claude-code' | 'codex'
    engineName: string
    channelId: string | null
    modelId: string | null
    effort: EffortSetting
    models?: EngineCapsuleConfig['models']
  }
  /** 窄列:胶囊收起渠道段,点任意段开全景 */
  narrow?: boolean
}>()

const emit = defineEmits<{
  (e: 'modelChange', modelId: string | null): void
  (e: 'effortChange', effort: EffortSetting): void
  (e: 'fastModeChange', fastMode: boolean): void
  (e: 'channelChange', channelId: string | null): void
  (e: 'chromeChange', chrome: boolean): void
  (e: 'extraArgsChange', extraArgs: string): void
  (e: 'refreshModels'): void
}>()

const { t } = useI18n()
const { channels, defaultSessionChannel, defaultSessionChannels } = useChannels()
const engineMode = computed(() => !!props.engineConfig)
const standaloneMode = computed(() => !!props.defaultConfig)
const standaloneEngineId = computed(() => props.defaultConfig?.engineId ?? 'claude-code')
const capsuleCwd = computed(() => props.cwd ?? null)
const { refreshCliDefaults } = useCliDefaults(capsuleCwd)

// ---- 面板开合(渐进层级) ----

type Layer = 'effort' | 'model' | 'channel'
/** 面板列:三段各自的列 + 仅全景显示的「高级」列(顾问/Chrome/自定义参数,无对应胶囊段) */
type Col = Layer | 'advanced'
const openLayer = ref<Layer | null>(null)
const containerRef = ref<HTMLElement>()
const modelListRef = ref<HTMLElement>()
const modelSearchRef = ref<HTMLInputElement>()
const modelSearchQuery = ref('')

/** 面板显示哪些列(自左向右);高级列只随全景出现 */
const visibleCols = computed<Col[]>(() => {
  switch (openLayer.value) {
    case 'channel': return engineMode.value || standaloneMode.value ? ['channel', 'model', 'effort'] : ['channel', 'model', 'effort', 'advanced']
    case 'model': return ['model', 'effort']
    case 'effort': return ['effort']
    default: return []
  }
})

/** 面板 fixed 定位(Teleport 到 body 逃出工作台列 overflow-hidden 裁剪) */
const panelRef = ref<HTMLElement>()
const panelPos = ref({ top: 0, left: 0 })

function placePanel() {
  const rect = containerRef.value?.getBoundingClientRect()
  if (!rect) return
  panelPos.value = { top: rect.bottom + 4, left: rect.left }
  // 面板宽度随列数变化,渲染后测量并钳回视口(右溢/下溢)
  nextTick(() => {
    const p = panelRef.value?.getBoundingClientRect()
    if (!p) return
    const left = Math.max(8, Math.min(panelPos.value.left, window.innerWidth - p.width - 8))
    const top = Math.max(8, Math.min(panelPos.value.top, window.innerHeight - p.height - 8))
    panelPos.value = { top, left }
  })
}

function openFrom(layer: Layer) {
  // 窄列渠道段收起:点任意段直接开全景,规则简化
  const target = props.narrow ? 'channel' : layer
  openLayer.value = openLayer.value === target ? null : target
  if (openLayer.value) {
    placePanel()
    nextTick(() => {
      if (layer === 'model') modelSearchRef.value?.focus()
      modelListRef.value
        ?.querySelector<HTMLElement>('.rc-opt.sel')
        ?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
    })
    // 渠道文件/settings.json 都是活文件:开面板即重读,不显示过期值
    void refreshChannels()
    void refreshCliDefaults()
  }
}

function onDocumentClick(e: MouseEvent) {
  if (!openLayer.value) return
  const target = e.target as Node
  // 面板已 Teleport 到 body,点外判定须同时豁免胶囊与面板两棵子树
  if (
    containerRef.value && !containerRef.value.contains(target)
    && panelRef.value && !panelRef.value.contains(target)
  ) {
    openLayer.value = null
  }
}
onMounted(() => document.addEventListener('mousedown', onDocumentClick))
onUnmounted(() => document.removeEventListener('mousedown', onDocumentClick))
watch(openLayer, layer => {
  if (!layer) modelSearchQuery.value = ''
})

// ---- 渠道列 ----

const channelOptions = computed(() => {
  if (props.engineConfig) {
    const result = [{
      id: OFFICIAL_CHANNEL_ID,
      name: t('topbar.channelFollowEngine', { engine: props.engineConfig.engineName }),
    }]
    for (const channel of channels.value) {
      if (
        channel.id !== OFFICIAL_CHANNEL_ID
        && channel.id !== OFFICIAL_DIRECT_CHANNEL_ID
        && channel.enabled
        && (standaloneMode.value || channel.scope !== 'agent-only')
        && channelSupportsEngine(channel, props.engineConfig.engineId)
      ) {
        result.push({ id: channel.id, name: channel.name })
      }
    }
    return result
  }
  if (props.defaultConfig) {
    const engineId = props.defaultConfig.engineId
    const result = [{
      id: OFFICIAL_CHANNEL_ID,
      name: t('topbar.channelFollowEngine', { engine: props.defaultConfig.engineName }),
    }]
    if (engineId === 'claude-code') {
      result.push({ id: OFFICIAL_DIRECT_CHANNEL_ID, name: t('topbar.channelOfficialDirect') })
    }
    for (const channel of channels.value) {
      if (
        channel.id !== OFFICIAL_CHANNEL_ID
        && channel.id !== OFFICIAL_DIRECT_CHANNEL_ID
        && channel.id !== APPLE_FM_CHANNEL_ID
        && channel.enabled
        && channelSupportsEngine(channel, engineId)
      ) {
        result.push({ id: channel.id, name: channel.name })
      }
    }
    if (engineId === 'claude-code' && channels.value.some(channel => channel.id === APPLE_FM_CHANNEL_ID)) {
      result.push({ id: APPLE_FM_CHANNEL_ID, name: 'Apple FM' })
    }
    return result
  }
  const result: { id: string; name: string }[] = [
    { id: OFFICIAL_CHANNEL_ID, name: t('topbar.channelOfficial') },
    { id: OFFICIAL_DIRECT_CHANNEL_ID, name: t('topbar.channelOfficialDirect') },
  ]
  for (const ch of channels.value) {
    if (ch.id !== OFFICIAL_CHANNEL_ID && ch.id !== OFFICIAL_DIRECT_CHANNEL_ID && ch.enabled
      && (standaloneMode.value || channelSupportsEngine(ch, 'claude-code'))) {
      result.push({ id: ch.id, name: ch.name })
    }
  }
  if (standaloneMode.value && channels.value.some(ch => ch.id === APPLE_FM_CHANNEL_ID)) {
    result.push({ id: APPLE_FM_CHANNEL_ID, name: 'Apple FM' })
  }
  return result
})

/** 解析后的当前渠道(选中判定;null=官方) */
const resolvedChannelKey = computed(() => props.engineConfig?.channelId ?? props.defaultConfig?.channelId ?? props.runConfig?.channelId ?? OFFICIAL_CHANNEL_ID)
/** 应用默认渠道(「默认」hint 所在项) */
const defaultChannelKey = computed(() => {
  if (props.engineConfig) {
    const engineId = props.engineConfig.engineId
    if (engineId !== 'claude-code' && engineId !== 'codex') return OFFICIAL_CHANNEL_ID
    const id = defaultSessionChannels.value[engineId]
    const channel = id ? channels.value.find(item => item.id === id) : null
    return channel?.enabled && channelSupportsEngine(channel, engineId)
      ? id!
      : OFFICIAL_CHANNEL_ID
  }
  if (standaloneMode.value) return props.defaultConfig?.channelId ?? OFFICIAL_CHANNEL_ID
  const id = defaultSessionChannel.value
  if (!id) return OFFICIAL_CHANNEL_ID
  const ch = channels.value.find(c => c.id === id)
  return ch && ch.enabled ? id : OFFICIAL_CHANNEL_ID
})
const channelOverridden = computed(() => props.engineConfig
  ? props.engineConfig.channelOverridden
  : standaloneMode.value ? false
  : props.settings?.channelId !== null)

function pickChannel(id: string) {
  // 与 ChannelDropdown 语义一致:点选即会话指定(含 official 强制态)
  emit('channelChange', id)
}

// ---- 模型列 ----

const channelRef = computed<string | null>(() => props.defaultConfig?.channelId ?? props.runConfig?.channelId ?? null)
const { items: claudeModelItems } = useModelOptions(channelRef)
const standaloneModelItems = computed<ModelInfo[]>(() => {
  const id = props.defaultConfig?.channelId
  if (!id || id === OFFICIAL_CHANNEL_ID || id === OFFICIAL_DIRECT_CHANNEL_ID) {
    if (standaloneEngineId.value === 'codex') {
      return (props.defaultConfig?.models ?? [])
        .filter(model => !model.hidden)
        .map(model => ({ id: model.id, label: model.label, contextWindow: 0 }))
    }
    return claudeModelItems.value
  }
  const channel = channels.value.find(item => item.id === id)
  const preferredModel = standaloneEngineId.value === 'codex'
    ? channel?.codex?.defaultModel
    : channel?.agentModel
  const values = [...new Set([
    ...(channel?.availableModels ?? []),
    ...(preferredModel ? [preferredModel] : []),
  ].map(value => value.trim()).filter(Boolean))]
  return values.length
    ? values.map(value => ({ id: value, label: value, contextWindow: 0 }))
    : claudeModelItems.value
})
const modelItems = computed<ModelInfo[]>(() => props.engineConfig
  ? props.engineConfig.models
      .filter(model => !model.hidden)
      .map(model => ({ id: model.id, label: model.label, contextWindow: 0 }))
  : standaloneMode.value ? standaloneModelItems.value
  : claudeModelItems.value)

/** 当前渠道的 modelEnv(能力声明判定用) */
const activeModelEnv = computed<Record<string, string> | undefined>(() => {
  if (props.engineConfig) return undefined
  const id = props.defaultConfig?.channelId ?? props.runConfig?.channelId
  if (!id) return undefined
  return channels.value.find(c => c.id === id)?.modelEnv
})

/** 把模型字符串归位到候选项 id(选中/默认 hint 判定) */
function modelKeyOf(modelStr: string | null | undefined): string | null {
  if (!modelStr) return null
  const lower = modelStr.toLowerCase()
  const exact = modelItems.value.find(m => m.id === modelStr || m.id.toLowerCase() === lower)
  if (exact) return exact.id
  const inferred = inferModel(lower)
  if (inferred && modelItems.value.some(m => m.id === inferred.id)) return inferred.id
  return modelStr
}

const selectedModelKey = computed(() => modelKeyOf(props.engineConfig?.model ?? props.defaultConfig?.modelId ?? props.runConfig?.display.model))
const defaultModelKey = computed(() =>
  modelKeyOf(props.engineConfig
    ? props.engineConfig.defaultModel
    : props.defaultConfig
      ? channels.value.find(channel => channel.id === (props.defaultConfig?.channelId ?? OFFICIAL_CHANNEL_ID))?.defaultModel
    : props.runConfig?.channelDefaultModel ?? props.runConfig?.cliDefaultModel),
)
const modelOverridden = computed(() => props.engineConfig
  ? props.engineConfig.modelOverridden
  : standaloneMode.value
    ? !!props.defaultConfig?.modelId
    : props.runConfig?.display.modelSource === 'session')
const advisorLocked = computed(() => !props.engineConfig && props.runConfig?.display.modelSource === 'advisor')

/** 会话在用清单外模型时附加为候选(原名展示,与旧 ModelDropdown 行为一致) */
const modelListItems = computed<ModelInfo[]>(() => {
  const base = modelItems.value
  const sel = selectedModelKey.value
  if (sel && !base.some(m => m.id === sel)) {
    return [...base, { id: sel, label: props.engineConfig?.model ?? props.defaultConfig?.modelId ?? props.runConfig?.display.model ?? sel, contextWindow: 0 }]
  }
  return base
})

const filteredModelListItems = computed(() => {
  const query = modelSearchQuery.value.trim().toLocaleLowerCase()
  if (!query) return modelListItems.value
  return modelListItems.value.filter(model =>
    model.label.toLocaleLowerCase().includes(query)
    || model.id.toLocaleLowerCase().includes(query),
  )
})
const longestModelLabel = computed(() => modelListItems.value.reduce(
  (longest, model) => model.label.length > longest.length ? model.label : longest,
  '',
))

function clearModelSearch() {
  modelSearchQuery.value = ''
  nextTick(() => modelSearchRef.value?.focus())
}

function pickModel(id: string) {
  if (advisorLocked.value) return
  emit('modelChange', id)
}

// ---- 强度列 ----

const EFFORT_LABELS: Record<EffortLevel, string> = {
  low: 'Low', medium: 'Medium', high: 'High', xhigh: 'xHigh', max: 'Max',
}
const EFFORT_OPTIONS: { value: NonNullable<EffortSetting>; label: string }[] = [
  ...(Object.entries(EFFORT_LABELS) as [EffortLevel, string][]).map(([value, label]) => ({ value, label })),
  { value: 'ultracode' as const, label: 'Ultracode' },
]
const effortOptionItems = computed(() => {
  if (standaloneMode.value) {
    if (standaloneEngineId.value !== 'codex') return EFFORT_OPTIONS
    const descriptor = props.defaultConfig?.models?.find(model => model.id === props.defaultConfig?.modelId)
    const ids = descriptor?.efforts.map(effort => effort.id) ?? ['low', 'medium', 'high', 'xhigh']
    return ids.map(value => ({
      value: value as NonNullable<EffortSetting>,
      label: value === 'xhigh' ? 'xHigh' : `${value.charAt(0).toUpperCase()}${value.slice(1)}`,
    }))
  }
  if (!props.engineConfig) return EFFORT_OPTIONS
  const descriptor = props.engineConfig.models.find(model => model.id === props.engineConfig?.model)
  const ids = descriptor?.efforts.map(effort => effort.id) ?? ['low', 'medium', 'high', 'xhigh']
  return ids.map(value => ({ value: value as NonNullable<EffortSetting>, label: value === 'xhigh' ? 'xHigh' : `${value.charAt(0).toUpperCase()}${value.slice(1)}` }))
})

const selectedEffort = computed<NonNullable<EffortSetting> | null>(
  () => (props.engineConfig?.effort ?? props.defaultConfig?.effort ?? props.runConfig?.display.effort ?? null) as NonNullable<EffortSetting> | null,
)
const defaultEffort = computed<NonNullable<EffortSetting> | null>(
  () => (props.engineConfig
    ? props.engineConfig.defaultEffort
    : props.defaultConfig
      ? channels.value.find(channel => channel.id === (props.defaultConfig?.channelId ?? OFFICIAL_CHANNEL_ID))?.defaultEffort ?? null
    : props.runConfig?.channelDefaultEffort ?? props.runConfig?.cliDefaultEffort) as NonNullable<EffortSetting> | null,
)
const effortOverridden = computed(() => props.engineConfig
  ? props.engineConfig.effortOverridden
  : standaloneMode.value
    ? !!props.defaultConfig?.effort
    : props.runConfig?.display.effortSource === 'session')

/** 强度能力标注:基于当前解析模型 + 渠道声明(软提示,不拦截) */
const effortCaps = computed(() =>
  effortCapabilities(props.runConfig?.display.model ?? null, activeModelEnv.value),
)
function effortUnsupported(value: NonNullable<EffortSetting>): boolean {
  if (props.engineConfig || standaloneMode.value) return false
  if (value === 'xhigh') return effortCaps.value.xhigh === false
  if (value === 'max') return effortCaps.value.max === false
  if (value === 'ultracode') return effortCaps.value.ultracode === false
  return false
}

function pickEffort(value: NonNullable<EffortSetting>) {
  emit('effortChange', value)
}

function onFastModeChange(event: Event) {
  emit('fastModeChange', (event.target as HTMLInputElement).checked)
}

const showFastMode = computed(() => !standaloneMode.value && (
  !engineMode.value || props.engineConfig?.showFastMode === true
))
const fastModeChecked = computed(() => props.engineConfig
  ? !!props.engineConfig.fastTier
    && props.engineConfig.serviceTier === props.engineConfig.fastTier.id
  : props.runConfig?.display.fastMode ?? false)
const fastModeDisabled = computed(() => !!props.engineConfig && !props.engineConfig.fastTier)
const fastModeTitle = computed(() => props.engineConfig
  ? props.engineConfig.fastModeUnavailableReason
    ?? props.engineConfig.fastTier?.description
    ?? t('topbar.fastModeTip')
  : t('topbar.fastModeTip'))

// ---- 胶囊段显示 ----

const channelSegLabel = computed(() => {
  const id = props.engineConfig?.channelId ?? props.defaultConfig?.channelId ?? props.runConfig?.channelId
  const base = !id || id === OFFICIAL_CHANNEL_ID
    ? t(props.engineConfig || props.defaultConfig ? 'topbar.channelFollowEngine' : 'topbar.channelOfficial', {
      engine: props.engineConfig?.engineName ?? props.defaultConfig?.engineName,
    })
    : channels.value.find(c => c.id === id)?.name ?? id
  if (!props.engineConfig) return base
  if (props.engineConfig.channelPending) return `${base} · ${t('topbar.channelPending')}`
  if (id === OFFICIAL_CHANNEL_ID && props.engineConfig.observedChannelLabel) {
    return `${base} · ${props.engineConfig.observedChannelLabel}`
  }
  return base
})

const modelSegLabel = computed(() => {
  const resolved = props.engineConfig?.model ?? props.defaultConfig?.modelId ?? props.runConfig?.display.model
  if (resolved) {
    const key = modelKeyOf(resolved)
    const hit = key ? modelListItems.value.find(m => m.id === key) : null
    return hit?.label ?? resolved
  }
  return t('topbar.modelDefault')
})

/** 等级徽章:第三方映射模型伪装的角色。选中项自带打标优先;清单外值兜底反查 modelEnv(多命中 join) */
const modelSegTier = computed<string | null>(() => {
  const key = selectedModelKey.value
  const hit = key ? modelListItems.value.find(m => m.id === key) : null
  if (hit?.mappedRole) return ROLE_DISPLAY[hit.mappedRole]
  if (props.engineConfig) return null
  const roles = resolveMappedRoles(props.runConfig?.display.model, activeModelEnv.value)
  return roles.length ? roles.map(r => ROLE_DISPLAY[r]).join('/') : null
})

/** 模型段 title:徽章随行(截断时悬停可见全文) */
const modelSegTitleName = computed(() =>
  modelSegTier.value
    ? `${modelSegLabel.value} · ${t('topbar.roleTier', { role: modelSegTier.value })}`
    : modelSegLabel.value,
)

const effortSegLabel = computed(() => {
  const v = selectedEffort.value
  if (!v) return 'High'
  return effortOptionItems.value.find(o => o.value === v)?.label ?? v
})

/** 来源列头文案 */
function srcLabel(source?: string): string {
  switch (source) {
    case 'session': return t('topbar.srcSession')
    case 'channel': return t('topbar.srcChannel')
    case 'advisor': return t('topbar.srcAdvisor')
    default: return t('topbar.srcCli')
  }
}
const channelSrcLabel = computed(() =>
  channelOverridden.value ? t('topbar.srcSession') : t(props.engineConfig ? 'topbar.srcEngine' : 'topbar.srcApp'),
)
const engineModelSrcLabel = computed(() => t(
  standaloneMode.value
    ? 'topbar.srcApp'
    : modelOverridden.value
    ? 'topbar.srcSession'
    : props.engineConfig?.defaultModel
      ? 'topbar.srcChannel'
      : 'topbar.srcEngine',
))
const engineEffortSrcLabel = computed(() => t(
  standaloneMode.value
    ? 'topbar.srcApp'
    : effortOverridden.value
    ? 'topbar.srcSession'
    : props.engineConfig?.defaultEffort
      ? 'topbar.srcChannel'
      : 'topbar.srcEngine',
))

// ---- 顾问 ----
// ---- 自定义 CLI 参数(逃生舱) ----

/** 输入草稿:失焦/回车才提交,面板重开时从 settings 同步 */
const extraArgsDraft = ref(props.settings?.extraArgs ?? '')
watch(() => props.settings?.extraArgs ?? '', (v) => { extraArgsDraft.value = v })

/** 协议级参数前端预警(与 Rust EXTRA_ARGS_DENYLIST 同源清单,Rust 端仍兜底剔除) */
const EXTRA_ARGS_DENYLIST = [
  '-p', '--print', '-c', '--continue',
  '--output-format', '--input-format', '--session-id', '--resume', '--fork-session',
  '--mcp-config', '--permission-prompt-tool', '--settings',
]
const extraArgsWarn = computed(() =>
  EXTRA_ARGS_DENYLIST.some(f => extraArgsDraft.value.split(/\s+/).some(t => t === f || t.startsWith(`${f}=`))),
)

function commitExtraArgs() {
  const v = extraArgsDraft.value.trim()
  if (v !== (props.settings?.extraArgs ?? '')) emit('extraArgsChange', v)
}

// ---- 管理渠道入口(原 ChannelDropdown 功能保留) ----
const { switchSection } = useUiState()
function openSettings() {
  openLayer.value = null
  switchSection('settings')
}
</script>

<template>
  <div ref="containerRef" class="relative inline-flex min-w-0">
    <!-- 胶囊:三段一枚,hover 段高亮,点哪段开哪层 -->
    <div
      class="inline-flex items-center h-[22px] border border-border rounded-[5px] text-xs
             text-muted-foreground cursor-pointer select-none whitespace-nowrap overflow-hidden min-w-0"
    >
      <button
        v-if="!narrow"
        type="button"
        class="capsule-seg"
        :class="channelOverridden ? 'seg-overridden' : 'seg-inherited'"
        :title="$t('topbar.channelTitle', { name: channelSegLabel })"
        @click="openFrom('channel')"
      >{{ channelSegLabel }}</button>
      <button
        type="button"
        class="capsule-seg seg-sep"
        :class="[
          modelOverridden || advisorLocked ? 'seg-overridden' : 'seg-inherited',
          narrow ? 'seg-first' : '',
        ]"
        :title="$t('topbar.modelTitle', { name: modelSegTitleName })"
        @click="openFrom('model')"
      >
        <span class="truncate">{{ modelSegLabel }}</span>
        <span v-if="modelSegTier" class="seg-tier">{{ $t('topbar.roleTier', { role: modelSegTier }) }}</span>
      </button>
      <button
        type="button"
        class="capsule-seg seg-sep"
        :class="effortOverridden ? 'seg-overridden' : 'seg-inherited'"
        :title="$t('topbar.effortTitle', { name: effortSegLabel })"
        @click="openFrom('effort')"
      >{{ effortSegLabel }}</button>
    </div>

    <!-- 渐进面板:列式扩展。Teleport 到 body:工作台列容器 overflow-hidden,
         列过窄时容器内 absolute 面板会被裁剪,fixed 悬浮层不受限 -->
    <Teleport to="body">
    <div
      v-if="openLayer"
      ref="panelRef"
      class="fixed z-50 inline-flex rounded-md border border-border
             shadow-paper-lifted bg-popover"
      :style="{ top: `${panelPos.top}px`, left: `${panelPos.left}px` }"
    >
      <!-- 渠道列 -->
      <div v-if="visibleCols.includes('channel')" class="rc-col">
        <div class="rc-head">
          <span class="rc-label">{{ $t('topbar.channelLabel') }}</span>
          <span class="rc-src">{{ channelSrcLabel }}</span>
          <button v-if="channelOverridden" class="rc-reset" @click="emit('channelChange', null)">{{ $t('topbar.resetInherit') }}</button>
        </div>
        <div class="rc-list">
          <button
            v-for="ch in channelOptions"
            :key="ch.id"
            class="rc-opt"
            :class="{ sel: ch.id === resolvedChannelKey }"
            @click="pickChannel(ch.id)"
          >
            <span class="truncate">{{ ch.name }}</span>
            <span v-if="ch.id === defaultChannelKey" class="rc-hint">{{ $t('topbar.hintDefault') }}</span>
          </button>
        </div>
        <button class="rc-opt rc-manage" @click="openSettings">
          <span class="i-carbon-settings w-3 h-3 shrink-0" />
          <span>{{ $t('topbar.manageChannels') }}</span>
        </button>
      </div>

      <!-- 模型列 -->
      <div v-if="visibleCols.includes('model')" class="rc-col rc-model-col">
        <div class="rc-head">
          <span class="rc-label">{{ $t('topbar.modelLabel') }}</span>
          <button
            v-if="modelRefreshable"
            type="button"
            class="rc-model-refresh"
            :disabled="modelsRefreshing"
            :title="$t(modelsRefreshing ? 'topbar.modelRefreshing' : 'topbar.modelRefresh')"
            :aria-label="$t(modelsRefreshing ? 'topbar.modelRefreshing' : 'topbar.modelRefresh')"
            @mousedown.prevent
            @click.stop="emit('refreshModels')"
          >
            <span class="i-carbon-renew" :class="{ 'animate-spin': modelsRefreshing }" aria-hidden="true" />
          </button>
          <span class="rc-src">{{ engineConfig ? engineModelSrcLabel : standaloneMode ? $t('topbar.srcApp') : advisorLocked ? $t('topbar.srcAdvisor') : srcLabel(runConfig?.display.modelSource) }}</span>
          <button v-if="modelOverridden" class="rc-reset" @click="emit('modelChange', null)">{{ $t('topbar.resetInherit') }}</button>
        </div>
        <div class="rc-model-search">
          <span class="i-carbon-search rc-model-search-icon" aria-hidden="true" />
          <input
            ref="modelSearchRef"
            v-model="modelSearchQuery"
            type="text"
            :placeholder="$t('topbar.modelSearchPlaceholder')"
            :aria-label="$t('topbar.modelSearchPlaceholder')"
            :disabled="advisorLocked"
            autocomplete="off"
            spellcheck="false"
            @keydown.esc.stop.prevent="clearModelSearch"
          />
          <button
            v-if="modelSearchQuery"
            type="button"
            class="rc-model-search-clear"
            :aria-label="$t('common.clear')"
            @mousedown.prevent
            @click.stop="clearModelSearch"
          >
            <span class="i-carbon-close" aria-hidden="true" />
          </button>
        </div>
        <span class="rc-model-width-probe" aria-hidden="true">{{ longestModelLabel }}</span>
        <div
          ref="modelListRef"
          class="rc-list rc-model-list"
          :class="{ 'opacity-45 pointer-events-none': advisorLocked }"
          :title="advisorLocked ? $t('topbar.modelAdvisorLocked') : ''"
        >
          <template v-for="(m, i) in filteredModelListItems" :key="m.id">
            <div v-if="i > 0 && !!m.legacy !== !!filteredModelListItems[i - 1].legacy" class="rc-divider" />
            <button
              class="rc-opt"
              :title="m.label"
              :class="{ sel: m.id === selectedModelKey, 'opacity-55': m.legacy }"
              @click="pickModel(m.id)"
            >
              <span class="rc-model-name">{{ m.label }}</span>
              <span v-if="m.mappedRole" class="rc-hint">{{ $t('topbar.roleTier', { role: ROLE_DISPLAY[m.mappedRole] }) }}</span>
              <span v-if="m.id === defaultModelKey" class="rc-hint">{{ $t('topbar.hintDefault') }}</span>
            </button>
          </template>
          <div v-if="!filteredModelListItems.length" class="rc-model-empty">
            {{ $t('topbar.modelSearchEmpty') }}
          </div>
        </div>
      </div>

      <!-- 强度列 -->
      <div v-if="visibleCols.includes('effort')" class="rc-col">
        <div v-if="showFastMode" class="rc-fast-block">
          <label
            class="rc-fast-option"
            :class="{ 'is-disabled': fastModeDisabled }"
            :title="fastModeTitle"
          >
            <input
              type="checkbox"
              :checked="fastModeChecked"
              :disabled="fastModeDisabled"
              :aria-label="$t('topbar.fastMode')"
              @change="onFastModeChange"
            />
            <span>{{ $t('topbar.fastMode') }}</span>
          </label>
          <p v-if="fastModeNotice" class="rc-fast-notice" role="status">{{ fastModeNotice }}</p>
        </div>
        <div class="rc-head">
          <span class="rc-label">{{ $t('topbar.effortLabel') }}</span>
          <span class="rc-src">{{ engineConfig ? engineEffortSrcLabel : standaloneMode ? $t('topbar.srcApp') : srcLabel(runConfig?.display.effortSource) }}</span>
          <button v-if="effortOverridden" class="rc-reset" @click="emit('effortChange', null)">{{ $t('topbar.resetInherit') }}</button>
        </div>
        <div class="rc-list">
          <button
            v-for="o in effortOptionItems"
            :key="o.value"
            class="rc-opt"
            :title="effortUnsupported(o.value) ? $t('topbar.effortUnsupportedTip') : ''"
            :class="{ sel: o.value === selectedEffort }"
            @click="pickEffort(o.value)"
          >
            <span>{{ o.label }}</span>
            <!-- 「默认」与「不支持」可同时成立(渠道默认档在当前模型上会被 CLI 静默降级),并列显示 -->
            <span v-if="o.value === defaultEffort" class="rc-hint">{{ $t('topbar.hintDefault') }}</span>
            <span v-if="effortUnsupported(o.value)" class="rc-hint rc-warn">{{ $t('topbar.hintUnsupported') }}</span>
          </button>
        </div>
      </div>

      <!-- 高级列(仅全景):顾问/Chrome/自定义参数——跨领域运行配置,不属于任何单一段 -->
      <div v-if="visibleCols.includes('advanced')" class="rc-col">
        <div class="rc-head">
          <span class="rc-label">{{ $t('topbar.advancedLabel') }}</span>
        </div>
        <div class="rc-adv-item">
          <button
            type="button"
            :class="['form-toggle-sm', { on: settings?.chrome }]"
            @click="emit('chromeChange', !settings?.chrome)"
          ><span class="form-toggle-knob" /></button>
          <span>{{ $t('topbar.chromeMode') }}</span>
        </div>
        <div class="rc-foot">{{ $t('topbar.chromeFoot') }}</div>
        <div class="rc-extra-args">
          <input
            v-model="extraArgsDraft"
            type="text"
            class="rc-args-input"
            :class="{ 'rc-args-warn': extraArgsWarn }"
            :placeholder="$t('topbar.extraArgsPlaceholder')"
            spellcheck="false"
            @blur="commitExtraArgs"
            @keydown.enter.prevent="commitExtraArgs"
          />
        </div>
        <div class="rc-foot" :class="{ 'rc-foot-warn': extraArgsWarn }">
          {{ extraArgsWarn ? $t('topbar.extraArgsDenied') : $t('topbar.extraArgsFoot') }}
        </div>
      </div>
    </div>
    </Teleport>
  </div>
</template>

<style scoped>
.capsule-seg {
  padding: 0 7px;
  height: 100%;
  display: inline-flex;
  align-items: center;
  position: relative;
  transition: background .12s, color .12s;
  max-width: 9rem;
  overflow: hidden;
  text-overflow: ellipsis;
}
.capsule-seg:hover { background: var(--muted); color: var(--foreground); }
.seg-sep::before {
  content: '·';
  position: absolute;
  left: -2.5px;
  opacity: .4;
  pointer-events: none;
}
.seg-first::before { content: none; }
.seg-inherited { opacity: .68; }
.seg-overridden { color: var(--foreground); font-weight: 500; }
.seg-tier { margin-left: 4px; font-size: 9px; opacity: .65; flex-shrink: 0; font-weight: 400; }

.rc-col { width: 152px; padding: 8px 8px 6px; display: flex; flex-direction: column; }
.rc-model-col { width: max-content; min-width: 152px; max-width: 280px; }
.rc-col + .rc-col { border-left: 1px solid var(--border); }
.rc-fast-block {
  margin: -2px 0 6px; padding-bottom: 5px; border-bottom: 1px solid var(--border);
}
.rc-fast-option {
  display: flex; align-items: center; gap: 7px; min-height: 28px;
  padding: 3px 8px;
  color: var(--muted-foreground); font-size: 12px; cursor: pointer;
}
.rc-fast-option:hover { color: var(--foreground); }
.rc-fast-option:focus-within { color: var(--foreground); }
.rc-fast-option input { accent-color: var(--primary); cursor: pointer; }
.rc-fast-option input:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
.rc-fast-option.is-disabled { opacity: .5; cursor: not-allowed; }
.rc-fast-option.is-disabled:hover { color: var(--muted-foreground); }
.rc-fast-option input:disabled { cursor: not-allowed; }
.rc-fast-notice {
  margin: 1px 8px 0; color: var(--muted-foreground); font-size: 9px; line-height: 1.35;
}
.rc-head { display: flex; align-items: baseline; gap: 6px; margin-bottom: 5px; padding: 0 2px; }
.rc-label { font-size: 11px; color: var(--muted-foreground); font-weight: 500; }
.rc-src { font-size: 9px; color: var(--muted-foreground); opacity: .7; margin-left: auto; }
.rc-reset { font-size: 9px; color: var(--primary); cursor: pointer; }
.rc-model-refresh {
  width: 20px; height: 20px; margin: -4px 0 -4px -2px;
  display: inline-flex; align-items: center; justify-content: center;
  border-radius: 4px; color: var(--muted-foreground); cursor: pointer;
  transition: color 150ms ease, background-color 150ms ease;
}
.rc-model-refresh:hover:not(:disabled) { background: var(--muted); color: var(--foreground); }
.rc-model-refresh:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
.rc-model-refresh:disabled { opacity: .5; cursor: wait; }
.rc-model-refresh span { width: 12px; height: 12px; }
.rc-list { display: flex; flex-direction: column; }
.rc-model-search {
  min-height: 26px; margin-bottom: 5px; padding: 0 6px;
  display: flex; align-items: center; gap: 5px;
  border: 1px solid var(--border); border-radius: 4px;
  background: var(--background); color: var(--muted-foreground);
}
.rc-model-search:focus-within { border-color: var(--primary); color: var(--foreground); }
.rc-model-search-icon { width: 12px; height: 12px; flex-shrink: 0; opacity: .65; }
.rc-model-search input {
  min-width: 0; width: 100%; border: 0; outline: 0; padding: 0;
  background: transparent; color: var(--foreground); font-size: 11px;
}
.rc-model-search input::placeholder { color: var(--muted-foreground); opacity: .65; }
.rc-model-search input:disabled { cursor: not-allowed; }
.rc-model-search-clear {
  width: 18px; height: 18px; margin-right: -3px; flex-shrink: 0;
  display: inline-flex; align-items: center; justify-content: center;
  border-radius: 3px; cursor: pointer;
}
.rc-model-search-clear:hover { background: var(--muted); color: var(--foreground); }
.rc-model-search-clear:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
.rc-model-search-clear span { width: 11px; height: 11px; }
.rc-model-width-probe {
  height: 0; padding: 0 8px; overflow: hidden;
  font-size: 12px; line-height: 0; white-space: nowrap; visibility: hidden;
}
.rc-model-list {
  max-height: min(280px, calc(100vh - 96px));
  overflow-y: auto;
  overscroll-behavior: contain;
}
.rc-opt {
  font-size: 12px; padding: 3px 8px; border-radius: 4px; color: var(--muted-foreground);
  cursor: pointer; display: flex; align-items: center; gap: 5px; margin-bottom: 1px;
  text-align: left; width: 100%;
}
.rc-opt:hover { background: var(--muted); color: var(--foreground); }
.rc-opt.sel { background: var(--primary); color: var(--primary-foreground); }
.rc-model-name {
  min-width: 0; overflow: hidden; overflow-wrap: anywhere;
  display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2;
  line-height: 1.35;
}
.rc-model-empty {
  padding: 9px 8px; color: var(--muted-foreground);
  font-size: 11px; line-height: 1.4; text-align: center;
}
.rc-hint { opacity: .6; font-size: 10px; margin-left: auto; flex-shrink: 0; }
/* 双标注并列(默认+不支持):第二个不再 auto 推移,紧随首个 */
.rc-hint + .rc-hint { margin-left: 0; }
.rc-opt.sel .rc-hint { opacity: .8; }
.rc-warn { color: var(--destructive); opacity: .75; }
.rc-opt.sel .rc-warn { color: var(--primary-foreground); }
.rc-divider { height: 1px; background: var(--border); margin: 4px 2px; opacity: .7; }
.rc-manage { margin-top: 3px; font-size: 11px; opacity: .85; }
.rc-adv-item {
  display: flex; align-items: center; gap: 6px; font-size: 11px;
  color: var(--muted-foreground); padding: 5px 2px 1px;
}
.rc-adv-sep { border-top: 1px solid var(--border); margin-top: 5px; }
.rc-foot { font-size: 9px; color: var(--muted-foreground); opacity: .75; padding: 4px 2px 0; line-height: 1.4; }
.rc-foot-warn { color: var(--destructive); opacity: .9; }
.rc-extra-args { padding: 8px 2px 0; border-top: 1px solid var(--border); margin-top: 5px; }
.rc-args-input {
  width: 100%; font-size: 11px; font-family: ui-monospace, monospace;
  padding: 3px 6px; border-radius: 4px; border: 1px solid var(--border);
  background: var(--background); color: var(--foreground);
}
.rc-args-input:focus { outline: none; border-color: var(--primary); }
.rc-args-input::placeholder { color: var(--muted-foreground); opacity: .6; }
.rc-args-warn { border-color: var(--destructive); }
</style>
