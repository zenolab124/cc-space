<script setup lang="ts">
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { shortModel, relativeTime, type TokenUsage } from '@/types'
import { inferModel, getContextWindow, MODELS } from '@/utils/modelContext'
import { ROLE_DISPLAY, resolveMappedRoles } from '@/utils/modelEnv'
import { useChannels } from '@/composables/useChannels'
import { ADVISOR_MAIN_MODEL, type EffortSetting } from '@/composables/useSessionSettings'
import type { ResolvedRunConfig } from '@/composables/useRunConfig'
import { useConfirm } from '@/composables/useConfirm'
import { useNotifications } from '@/composables/useNotifications'
import { useRunners } from '@/composables/useRunners'
import RunConfigCapsule from './RunConfigCapsule.vue'
import PermissionModeDropdown from './PermissionModeDropdown.vue'
import SessionToolbar from './SessionToolbar.vue'
import SessionTokenBreakdown from './SessionTokenBreakdown.vue'
import type { PermissionMode } from '@/composables/useSessionSettings'

/**
 * 单行极简顶栏:常显区只留高频控件(模型/进度/努力),标题不再显示(列头已有),
 * 元数据(ID/分支/时间)与操作(刷新/终端/VSCode/删除)统一收进 ⋯ 菜单。
 * 窄列折叠的控件也进同一菜单——最窄退化为 [模型][⋯],保持整齐。
 */
const props = defineProps<{
  /** 完整 sessionId(菜单内复制用) */
  sessionId: string
  /** 已计算好的 shortId(菜单内展示) */
  shortIdValue: string
  /** projectId(删除会话用) */
  projectId: string
  /** session.cwd(终端/VSCode 打开用) */
  cwd: string | null
  /** git 分支 */
  gitBranch: string | null
  /** 真实跑过的最近 assistant 消息的 model 字符串(jsonl 里 message.model;含 [1m] 后缀如有) */
  modelString: string | null
  /** 已占用上下文 token 数(最近一次 assistant 响应的 input + cache_read_input_tokens) */
  usedContextTokens: number
  /** API 报告的真实上下文容量（来自 result 事件 modelUsage.contextWindow，null 时回退推断） */
  realContextWindow: number | null
  /** 最后修改时间(秒级时间戳) */
  lastModified: number
  /** 用户已选择的模型 ID(来自 useSessionSettings,可能为 null) */
  selectedModelId: string | null
  /** 用户已选择的努力等级(null = 跟随 CLI,'ultracode' = 超档) */
  selectedEffort: EffortSetting
  /** null = 跟随 CLI/项目配置；boolean = 本会话显式覆盖 */
  selectedFastMode: boolean | null
  /** 快速模式不可用并自动恢复标准速度后的提示。 */
  fastModeNotice?: string | null
  /** 渠道选择(null = 跟随应用默认;'official' = 强制官方;其他 = 渠道 id) */
  selectedChannelId: string | null
  /** 解析后的最终注入渠道 id(null = 官方):终端恢复带渠道用 */
  resolvedChannelId: string | null
  /** 运行配置同源解析结果（display 用于回显，launch 用于发送，见 useRunConfig） */
  runConfig: ResolvedRunConfig
  /** 顾问模式开关状态 */
  selectedAdvisor: boolean
  /** Chrome 集成开关状态 */
  selectedChrome: boolean
  /** 自定义 CLI 参数 */
  selectedExtraArgs: string
  /** null 表示跟随 CLI/项目配置 */
  selectedPermissionMode: PermissionMode | null
  /** 会话累计 token 用量(已含子 Agent/工作流) */
  totalTokens: TokenUsage
  /** 其中子 Agent/工作流分项(totalTokens 已含) */
  subagentTokens: TokenUsage
}>()

const emit = defineEmits<{
  (e: 'modelChange', modelId: string | null): void
  (e: 'effortChange', effort: EffortSetting): void
  (e: 'fastModeChange', fastMode: boolean): void
  (e: 'channelChange', channelId: string | null): void
  (e: 'chromeChange', chrome: boolean): void
  (e: 'extraArgsChange', extraArgs: string): void
  (e: 'permissionModeChange', mode: PermissionMode | null): void
  (e: 'reload'): void
  (e: 'deleted'): void
}>()

const { t } = useI18n()
const { confirm } = useConfirm()
const { notifyTransient } = useNotifications()

// --- 派生数据 ---

/** 容量推断用的模型字符串:历史真值优先(容量语境),与胶囊的"下次发送"解析分离 */
const effectiveModelStr = computed(() =>
  props.selectedAdvisor ? ADVISOR_MAIN_MODEL : (props.selectedModelId ?? props.modelString),
)

/** 胶囊消费的会话覆盖原值(重置钮显隐/顾问开关状态) */
const capsuleSettings = computed(() => ({
  modelId: props.selectedModelId,
  effort: props.selectedEffort,
  fastMode: props.selectedFastMode,
  channelId: props.selectedChannelId,
  chrome: props.selectedChrome,
  extraArgs: props.selectedExtraArgs,
}))

/** 解析后的 ModelInfo。API 模型名永远不带 [1m]，无法区分 200K/1M 变体——
 *  有真实容量时按真实值修正；无真实值时默认取 1M 变体（Claude Code CLI 默认 1M） */
const effectiveModel = computed(() => {
  const model = inferModel(effectiveModelStr.value)
  if (!model) return model
  if (model.id.endsWith('[1m]')) return model
  const oneMVariant = MODELS.find(m => m.id === `${model.id}[1m]`)
  if (!oneMVariant) return model
  if (props.realContextWindow) {
    return props.realContextWindow >= oneMVariant.contextWindow ? oneMVariant : model
  }
  return oneMVariant
})

const { channels } = useChannels()

/** 真实跑过模型的伪装等级(按当前解析渠道的映射反查;官方渠道/无映射恒 null) */
const realModelTier = computed<string | null>(() => {
  const id = props.resolvedChannelId
  if (!id) return null
  const env = channels.value.find(c => c.id === id)?.modelEnv
  const roles = resolveMappedRoles(props.modelString, env)
  return roles.length ? roles.map(r => ROLE_DISPLAY[r]).join('/') : null
})

/** 上下文容量:API 真值 → effectiveModel 容量 → 下次发送解析模型推断兜底 */
const capacity = computed(() =>
  props.realContextWindow
    ?? effectiveModel.value?.contextWindow
    ?? getContextWindow(props.runConfig.display.model ?? null),
)

// --- ⋯ 统一菜单(折叠控件 + 元数据 + 操作) ---

const menuOpen = ref(false)

// --- 菜单操作 ---

async function copySessionId() {
  try {
    await navigator.clipboard.writeText(props.sessionId)
    notifyTransient(t('topbar.sessionIdCopied'))
  } catch {
    notifyTransient(t('topbar.copyFailed'), t('topbar.checkClipboard'))
  }
  menuOpen.value = false
}

function onReload() {
  menuOpen.value = false
  emit('reload')
}

async function openInFinder() {
  menuOpen.value = false
  if (!props.cwd) return
  try {
    await invoke('open_in_finder', { path: props.cwd })
  } catch (e) {
    notifyTransient(t('topbar.openFailed'), String(e))
  }
}

async function openInTerminal() {
  menuOpen.value = false
  if (!props.cwd) return
  try {
    // 带上会话渠道(--settings <渠道文件>),终端恢复不静默回落官方
    await invoke('resume_in_terminal', {
      cwd: props.cwd,
      sessionId: props.sessionId,
      channel: props.resolvedChannelId,
    })
  } catch (e) {
    if (String(e).includes('AUTOMATION_DENIED')) {
      notifyTransient(t('topbar.automationDenied'), t('topbar.automationDeniedHint'))
    } else {
      notifyTransient(t('topbar.terminalFailed'), String(e))
    }
  }
}

async function openInVscode() {
  menuOpen.value = false
  if (!props.cwd) return
  try {
    await invoke('resume_in_vscode', { cwd: props.cwd })
  } catch (e) {
    notifyTransient(t('topbar.vscodeFailed'), String(e))
  }
}

async function onDelete() {
  menuOpen.value = false
  const ok = await confirm(t('archive.deleteSessionConfirm'), t('common.delete'))
  if (!ok) return
  try {
    // 删除会话前先停止该会话下所有 runner（FR-007 生命周期治理）
    await useRunners().stopAllForSession(props.sessionId)
    await invoke('delete_session', { projectId: props.projectId, sessionId: props.sessionId })
    emit('deleted')
  } catch (e) {
    notifyTransient(t('topbar.deleteFailed'), String(e))
  }
}

// --- 事件转发 ---

function onModelChange(id: string | null) {
  emit('modelChange', id)
}
function onEffortChange(level: EffortSetting) {
  emit('effortChange', level)
}
function onChannelChange(channelId: string | null) {
  emit('channelChange', channelId)
}
function onPermissionModeChange(mode: PermissionMode | null) {
  emit('permissionModeChange', mode)
  menuOpen.value = false
}
</script>

<template>
  <SessionToolbar
    v-model:menu-open="menuOpen"
    :used-context-tokens="usedContextTokens"
    :context-capacity="capacity"
  >
    <template #controls="{ containerWidth }">
      <!-- 运行配置胶囊:渠道·模型·强度三段一枚(点哪段开哪层渐进面板);窄列收渠道段 -->
      <RunConfigCapsule
        :settings="capsuleSettings"
        :run-config="runConfig"
        :cwd="cwd"
        :fast-mode-notice="fastModeNotice"
        :narrow="containerWidth < 280"
        @model-change="onModelChange"
        @effort-change="onEffortChange"
        @fast-mode-change="(v: boolean) => emit('fastModeChange', v)"
        @channel-change="onChannelChange"
        @chrome-change="(v: boolean) => emit('chromeChange', v)"
        @extra-args-change="(v: string) => emit('extraArgsChange', v)"
      />

      <PermissionModeDropdown
        v-if="containerWidth >= 520"
        :selected="selectedPermissionMode"
        :effective="runConfig.display.permissionMode"
        @select="onPermissionModeChange"
      />
    </template>

    <template #actions>
      <!-- 外部注入控件(异步任务按钮等) -->
      <slot />
    </template>

    <template #menu="{ containerWidth }">
      <!-- 窄列收纳项 -->
      <div v-if="containerWidth < 520" class="py-0.5 border-b border-border">
          <PermissionModeDropdown
            variant="submenu"
            :selected="selectedPermissionMode"
            :effective="runConfig.display.permissionMode"
            @select="onPermissionModeChange"
          />
        </div>

        <!-- 元数据 -->
        <div class="px-3 py-1.5 text-xs text-muted-foreground flex flex-col gap-1 border-b border-border">
          <button
            class="flex items-center gap-1.5 hover:text-foreground transition-colors text-left"
            :title="$t('topbar.copySessionId')"
            @click="copySessionId"
          >
            <span class="font-mono">{{ shortIdValue }}</span>
            <span class="i-carbon-copy w-3 h-3 shrink-0" />
          </button>
          <span v-if="gitBranch" class="flex items-center gap-1.5">
            <span class="i-carbon-branch w-3 h-3 shrink-0" />{{ gitBranch }}
          </span>
          <span v-if="modelString && !effectiveModel" class="truncate">
            {{ shortModel(modelString) }}<template v-if="realModelTier"> · {{ $t('topbar.roleTier', { role: realModelTier }) }}</template>
          </span>
          <span>{{ relativeTime(lastModified) }}</span>
          <!-- 会话 token 统计 -->
          <SessionTokenBreakdown :total-tokens="totalTokens" :subagent-tokens="subagentTokens" />
        </div>

        <!-- 操作 -->
        <div class="py-1 flex flex-col">
          <button class="menu-item" @click="onReload">
            <span class="i-carbon-renew w-3.5 h-3.5" />{{ $t('topbar.refreshSession') }}
          </button>
          <button v-if="cwd" class="menu-item" @click="openInFinder">
            <span class="i-carbon-folder w-3.5 h-3.5" />{{ $t('topbar.openInFinder') }}
          </button>
          <button v-if="cwd" class="menu-item" @click="openInTerminal">
            <span class="i-carbon-terminal w-3.5 h-3.5" />{{ $t('topbar.openInTerminal') }}
          </button>
          <button v-if="cwd" class="menu-item" @click="openInVscode">
            <span class="i-carbon-code w-3.5 h-3.5" />{{ $t('topbar.openInVscode') }}
          </button>
          <button class="menu-item text-destructive! hover:bg-destructive/10" @click="onDelete">
            <span class="i-carbon-trash-can w-3.5 h-3.5" />{{ $t('topbar.deleteSession') }}
          </button>
        </div>
    </template>
  </SessionToolbar>
</template>

<style scoped>
.menu-item {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 5px 12px;
  font-size: 12px;
  text-align: left;
  color: var(--foreground);
  transition: background-color 0.15s;
}
.menu-item:hover {
  background: var(--muted);
}
</style>
