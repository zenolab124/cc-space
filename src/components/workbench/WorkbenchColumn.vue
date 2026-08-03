<script setup lang="ts">
import { computed, ref, provide } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { useProjects } from '@/composables/useProjects'
import { useWorkbench, type WorkbenchColumn } from '@/composables/useWorkbench'
import { useSessionStream } from '@/composables/useStreaming'
import { useSessionStatus } from '@/composables/useSessionStatus'
import { useSessionSettings, inheritRunSettings } from '@/composables/useSessionSettings'
import { useRunConfig } from '@/composables/useRunConfig'
import { refreshChannels } from '@/composables/useChannels'
import { refreshCliDefaults } from '@/composables/useCliDefaults'
import { useConfirm } from '@/composables/useConfirm'
import { useNotifications } from '@/composables/useNotifications'
import { displayTitle } from '@/types'
import { fileName } from '@/utils/path'
import { collectSessionCapabilities } from '@/features'
import { useSessionMeta } from '@/composables/useSessionMeta'
import { usesNativeSessionSurface } from '@/engines/integration'
import type { SessionSummary } from '@/types'

const { getMeta } = useSessionMeta()
import SessionDetail from '../SessionDetail.vue'
import EngineSessionDetail from '../engine/EngineSessionDetail.vue'

const props = defineProps<{
  column: WorkbenchColumn
  tabId: string
  index: number
  handleRef?: (el: any) => void
}>()

const emit = defineEmits<{
  (e: 'startRace'): void
}>()

const { t } = useI18n()
const { projects } = useProjects()
const { collapseColumn, removeSession, removeRaceLane, draftCwd, engineDraft, findLane, state, openSession, createDraftSession, registerFork, forkSourceOf } = useWorkbench()

provide('columnIndex', computed(() => props.index))
provide('tabId', computed(() => props.tabId))
const { confirm } = useConfirm()
const { notifyTransient } = useNotifications()

const tab = computed(() => state.value.tabs.find(t => t.id === props.tabId))
const lane = computed(() => tab.value ? findLane(tab.value, props.column.sessionId) : null)
const isRace = computed(() => !!lane.value)

const rcLoading = ref(false)
const sessionSummary = computed<SessionSummary | null>(() => {
  const persisted = projects.value
    .flatMap(project => project.sessions)
    .find(session => session.id === props.column.sessionId)
  if (persisted) return persisted
  const draft = engineDraft(props.column.sessionId)
  if (!draft) return null
  return {
    id: props.column.sessionId,
    native_id: draft.reference.nativeId,
    reference: draft.reference,
    project_reference: draft.project,
    engine: draft.reference.engine,
    engine_name: draft.engineName,
    title: t('session.newSessionTitle'),
    first_user_message: null,
    model: null,
    git_branch: null,
    cwd: draft.cwd,
    version: null,
    timestamp: null,
    last_modified: 0,
    total_tokens: { input_tokens: 0, output_tokens: 0, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
    subagent_tokens: { input_tokens: 0, output_tokens: 0, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
    file_size: 0,
    message_count: 0,
    context_window: null,
  }
})
const useNativeDetail = computed(() => !sessionSummary.value?.engine || usesNativeSessionSurface(sessionSummary.value.engine))

async function onToggleRC() {
  const enabling = !stream.value.rcActive
  rcLoading.value = true
  try {
    const session = projects.value.flatMap(p => p.sessions).find(s => s.id === props.column.sessionId)
    // 与发消息同源解析渠道/模型/effort:进程未启动时本调用会用这套配置起进程,
    // 硬编码 null(=官方)会让 RC 判决对着错误的渠道,发消息时又因渠道不一致重启进程
    const cwd = session?.cwd ?? draftCwd(props.column.sessionId) ?? ''
    await Promise.all([refreshChannels(), refreshCliDefaults(cwd)])
    const rc = runConfig.value
    await invoke('toggle_remote_control', {
      sessionId: props.column.sessionId,
      cwd,
      model: rc.launch.model ?? null,
      effort: rc.launch.effort ?? null,
      channel: rc.channelId,
      advisor: settings.value.advisor,
      chrome: settings.value.chrome,
      forkSource: forkSourceOf(props.column.sessionId) ?? null,
      extraArgs: settings.value.extraArgs || null,
      enabled: enabling,
      permissionMode: rc.launch.permissionMode,
      sessionCapabilities: collectSessionCapabilities(),
    })
    // 按钮状态与成败 toast 均由 CLI 判决(rc-status 事件)驱动,此处只负责把请求发出去
  } catch (e) {
    // invoke 失败 = 进程级故障(判决不会再来),就地报错
    notifyTransient(t('workbench.column.rcFailed'), String(e))
  } finally {
    rcLoading.value = false
  }
}

/** 列头 Chrome 开关:与胶囊面板同源(settings.chrome),变更经 needs_restart 下一条消息生效 */
function onToggleChrome() {
  const next = !settings.value.chrome
  setChrome(next)
  notifyTransient(t(next ? 'session.chromeEnabled' : 'session.chromeDisabled'))
}

const sid = computed(() => props.column.sessionId)
const stream = useSessionStream(sid)
const status = useSessionStatus(sid)
// RC 开关与发消息同源的运行配置(渠道/模型/effort/advisor)
const { settings, setChrome } = useSessionSettings(sid)
const runConfigCwd = computed(() => {
  const session = projects.value.flatMap(project => project.sessions)
    .find(item => item.id === props.column.sessionId)
  return session?.cwd ?? draftCwd(props.column.sessionId) ?? null
})
const { runConfig } = useRunConfig(settings, runConfigCwd)

const projectName = computed(() => {
  for (const p of projects.value) {
    if (p.sessions.some(s => s.id === props.column.sessionId))
      return fileName(p.display_path)
  }
  const cwd = draftCwd(props.column.sessionId)
  return cwd ? fileName(cwd) : engineDraft(props.column.sessionId)?.cwd ? fileName(engineDraft(props.column.sessionId)!.cwd) : null
})

const title = computed(() => {
  for (const p of projects.value) {
    const s = p.sessions.find(s => s.id === props.column.sessionId)
    if (s) return displayTitle(s, getMeta(s.id)?.title)
  }
  if (draftCwd(props.column.sessionId) || engineDraft(props.column.sessionId)) return t('session.newSessionTitle')
  return props.column.sessionId.slice(0, 8)
})

function onFork() {
  const session = projects.value.flatMap(p => p.sessions).find(s => s.id === props.column.sessionId)
  if (!session?.cwd) return
  // 懒分叉:只登记意图,落盘由首条消息时 CLI 原生 --fork-session 完成
  const newSessionId = crypto.randomUUID()
  registerFork(newSessionId, props.column.sessionId, session.cwd)
  inheritRunSettings(props.column.sessionId, newSessionId)
  openSession(newSessionId)
  notifyTransient(t('workbench.column.forkCreated'))
}

function onNewSession() {
  const session = projects.value.flatMap(p => p.sessions).find(s => s.id === props.column.sessionId)
  const cwd = session?.cwd || draftCwd(props.column.sessionId)
  if (!cwd) return
  const newSessionId = createDraftSession(cwd)
  // 同项目新建延续当前会话的模型选择(渠道/模型/effort/顾问)
  inheritRunSettings(props.column.sessionId, newSessionId)
}

function onCollapse() {
  collapseColumn(props.tabId, props.column.sessionId)
}

async function onClose() {
  if (stream.value.streaming) {
    const ok = await confirm(t('workbench.monitor.removeConfirm'), t('common.removeBrief'))
    if (!ok) return
  }
  removeSession(props.column.sessionId)
}

async function onCloseLane() {
  if (stream.value.streaming) {
    const ok = await confirm(t('workbench.monitor.removeConfirm'), t('common.removeBrief'))
    if (!ok) return
  }
  removeRaceLane(props.tabId, props.column.sessionId)
}

const isDragging = defineModel<boolean>('dragging', { default: false })
</script>

<template>
  <div
    class="h-full flex flex-col bg-card border border-border rounded overflow-hidden"
    :class="isDragging ? 'shadow-paper-lifted opacity-50' : 'shadow-paper'"
  >
    <!-- 列头 -->
    <div
      :ref="handleRef"
      class="shrink-0 flex items-center gap-2 px-3 py-2 border-b border-border cursor-grab active:cursor-grabbing touch-none"
    >
      <span
        class="w-1.5 h-1.5 rounded-full shrink-0"
        :class="[status.dotClass, { 'col-dot-pulse': status.pulse }]"
      />
      <template v-if="isRace">
        <span class="flex-1 min-w-0 truncate text-xs font-semibold">{{ lane!.label }}</span>
      </template>
      <template v-else>
        <span v-if="projectName" class="shrink-0 text-[10px] px-1.5 py-0.5 rounded leading-tight" style="color: var(--tag-foreground); background: var(--tag)">{{ projectName }}</span>
        <span class="flex-1 min-w-0 truncate text-xs font-semibold">{{ title }}</span>
      </template>
      <button
        v-if="useNativeDetail"
        :disabled="rcLoading"
        class="icon-btn icon-btn-sm disabled:opacity-40"
        :class="stream.rcActive ? 'border-primary! text-primary!' : ''"
        v-tooltip="stream.rcActive ? $t('workbench.column.rcEnabled') : $t('workbench.column.rcEnable')"
        @pointerdown.stop
        @click.stop="onToggleRC"
      >
        <span class="i-carbon-remote-connection w-3 h-3" />
      </button>
      <button
        v-if="useNativeDetail"
        class="icon-btn icon-btn-sm"
        :class="settings.chrome ? 'border-primary! text-primary!' : ''"
        v-tooltip="settings.chrome ? $t('workbench.column.chromeEnabled') : $t('workbench.column.chromeEnable')"
        @pointerdown.stop
        @click.stop="onToggleChrome"
      >
        <span class="i-app-chrome w-3 h-3" />
      </button>
      <!-- 普通模式:赛马 + 分叉 + 新建 + 收起 + 关闭 -->
      <template v-if="!isRace && useNativeDetail">
        <button
          class="icon-btn icon-btn-sm"
          v-tooltip="$t('workbench.race.startRace')"
          @pointerdown.stop
          @click.stop="emit('startRace')"
        >
          <span class="i-app-horse w-3 h-3" />
        </button>
        <button
          class="icon-btn icon-btn-sm"
          v-tooltip="$t('workbench.column.fork')"
          @pointerdown.stop
          @click.stop="onFork"
        >
          <span class="i-carbon-branch w-3 h-3" />
        </button>
        <button
          class="icon-btn icon-btn-sm"
          v-tooltip="$t('workbench.column.newSession')"
          @pointerdown.stop
          @click.stop="onNewSession"
        >
          <span class="i-carbon-add w-3 h-3" />
        </button>
        <button
          class="icon-btn icon-btn-sm"
          v-tooltip="$t('workbench.column.collapseToRail')"
          @pointerdown.stop
          @click="onCollapse"
        >
          <span class="i-carbon-chevron-left w-3 h-3" />
        </button>
        <button
          class="icon-btn icon-btn-sm icon-btn-danger"
          v-tooltip="$t('workbench.column.closeExit')"
          @pointerdown.stop
          @click="onClose"
        >
          <span class="i-carbon-close w-3 h-3" />
        </button>
      </template>
      <template v-else-if="!isRace">
        <button
          class="icon-btn icon-btn-sm"
          v-tooltip="$t('workbench.column.collapseToRail')"
          @pointerdown.stop
          @click="onCollapse"
        >
          <span class="i-carbon-chevron-left w-3 h-3" />
        </button>
        <button
          class="icon-btn icon-btn-sm icon-btn-danger"
          v-tooltip="$t('workbench.column.closeExit')"
          @pointerdown.stop
          @click="onClose"
        >
          <span class="i-carbon-close w-3 h-3" />
        </button>
      </template>
      <!-- 赛马模式:关闭赛道 -->
      <button
        v-else
        class="icon-btn icon-btn-sm icon-btn-danger"
        v-tooltip="$t('workbench.race.closeLane')"
        @pointerdown.stop
        @click="onCloseLane"
      >
        <span class="i-carbon-close w-3 h-3" />
      </button>
    </div>

    <div class="flex-1 min-h-0">
      <SessionDetail v-if="useNativeDetail" mode="workbench" :session-id="column.sessionId" :hide-input="isRace" />
      <EngineSessionDetail v-else-if="sessionSummary" :session="sessionSummary" mode="workbench" :hide-input="isRace" />
    </div>
  </div>
</template>

<style scoped>
.col-dot-pulse {
  animation: col-pulse 1.6s ease-in-out infinite;
}
@keyframes col-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.35; }
}
@media (prefers-reduced-motion: reduce) {
  .col-dot-pulse { animation: none; }
}
</style>
