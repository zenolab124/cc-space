<script setup lang="ts">
import { ref, shallowRef, computed, watch, nextTick, onMounted, onUnmounted, provide } from 'vue'
import { useI18n } from 'vue-i18n'
import { useConfirm } from '@/composables/useConfirm'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useProjects } from '@/composables/useProjects'
import { useSessions } from '@/composables/useSessions'
import { useSearch } from '@/composables/useSearch'
import { createSessionDetail } from '@/composables/useSessionDetail'
import {
  useStreaming,
  useSessionStream,
  finishedDirty,
  syncProcessAlive,
  autoTurnLanded,
  autoLandedSessions,
  setSessionRenderSurface,
  removeSessionRenderSurface,
  markSessionRenderActive,
  sessionRenderCadence,
} from '@/composables/useStreaming'
import { useSessionSettings, type ChannelMark } from '@/composables/useSessionSettings'
import { useStickyUserPrompt } from '@/composables/useStickyUserPrompt'
import { useRunConfig } from '@/composables/useRunConfig'
import {
  useChannels,
  refreshChannels,
  channelDisplayName,
  OFFICIAL_CHANNEL_ID,
} from '@/composables/useChannels'
import { useWorkbench } from '@/composables/useWorkbench'
import { useNotifications } from '@/composables/useNotifications'
import {
  shouldTriggerPanel,
  parseCommand,
  getAllCommands,
  type SlashCommand,
} from '@/composables/useSlashCommands'
import { useWorkshop } from '@/composables/useWorkshop'
import { useSessionMeta } from '@/composables/useSessionMeta'
import { displayTitle, shortId, hasReportedUsage, shouldReplaceUsage } from '@/types'
import { inferModel } from '@/utils/modelContext'
import {
  summarizeAssistantResponse,
  type AssistantResponseMeta,
} from '@/utils/assistantResponse'
import { cwdToProjectId, samePath } from '@/utils/path'
import { ROLE_DISPLAY, resolveMappedRoles } from '@/utils/modelEnv'
import { filterConsumedResults, type ToolResultData } from '@/utils/toolPair'
import { findPendingPermissionToolUseId } from '@/utils/toolDisplay'
import type { SessionRecord, SessionSummary, ContentBlock } from '@/types'
import ContentBlockList from './ContentBlockList.vue'
import AssistantResponseFrame from './AssistantResponseFrame.vue'
import MessageGroup from './MessageGroup.vue'
import SystemEventRow from './SystemEventRow.vue'
import DividerMark from './DividerMark.vue'
import SessionTopBar from './topbar/SessionTopBar.vue'
import SlashCommandPanel from './SlashCommandPanel.vue'
import SlashHelpCard from './SlashHelpCard.vue'
import PermissionCard from './PermissionCard.vue'
import QuestionCard from './QuestionCard.vue'
import PlanApprovalCard from './PlanApprovalCard.vue'
import UserMsgContent from './UserMsgContent.vue'
import SessionSurface from './session/SessionSurface.vue'
import SessionComposer from './session/SessionComposer.vue'
import SessionComposerField from './session/SessionComposerField.vue'
import SessionComposerAttachments from './session/SessionComposerAttachments.vue'
import SessionComposerQueue, { type ComposerQueueItem } from './session/SessionComposerQueue.vue'
import SessionSidePanel from './session/SessionSidePanel.vue'
import { shouldSubmitComposer } from './session/composerAction'
import SessionViewport from './session/SessionViewport.vue'
import SessionContentState from './session/SessionContentState.vue'
import SessionBackToBottom from './session/SessionBackToBottom.vue'
import SessionTypingIndicator from './session/SessionTypingIndicator.vue'
import SessionInteractionPanel from './session/SessionInteractionPanel.vue'
import SessionReadonlyBar from './session/SessionReadonlyBar.vue'
import SessionIdentityBar from './session/SessionIdentityBar.vue'
import ConversationUserMessage from './session/ConversationUserMessage.vue'
import { useImageInput } from '@/composables/useImageInput'
import { useSessionSidePanelHost } from '@/composables/useSessionSidePanelHost'
import { SESSION_FILE_ROOT } from '@/composables/useSessionFileLinks'
import { useHtmlVisual } from '@/features'
import SessionBannerOverlay from './session/SessionBannerOverlay.vue'
import SessionAnchorNav, { type AnchorItem } from './SessionAnchorNav.vue'
import {
  usePermissionRequests,
  currentForSession,
  type RespondExtra,
} from '@/composables/usePermissionRequests'
import { createSubAgentContext } from '@/composables/useSubAgents'
import { asyncTaskStopId, buildAsyncLedger, isActive, type AsyncTaskItem } from '@/composables/useAsyncTasks'
import type { SubAgentMeta } from '@/types'
import AsyncTaskPanel from './AsyncTaskPanel.vue'
import { IMAGE_LOCATOR, type ImageLocator } from '@/utils/ccimg'
import { renderMarkdownDeferred } from '@/composables/useMarkdown'
import { persistKeyOf } from '@/lib/stream-markdown/constants'
import { useFileLedger } from '@/composables/useFileLedger'
import FileLedgerPanel from './FileLedgerPanel.vue'
import ArtifactPreviewList from './artifacts/ArtifactPreviewList.vue'
import { detectContentBlockArtifacts } from '@/features/artifact-preview/detectArtifacts'
import RunnerPanel from './runner/RunnerPanel.vue'
import {
  elementScroll,
  measureElement as measureVirtualElement,
  useVirtualizer,
  type Virtualizer,
} from '@tanstack/vue-virtual'
import { useVirtualizationSettings } from '@/composables/useVirtualizationSettings'
import {
  canApplyScrollFollowToken,
  captureScrollFollowToken,
  createScrollFollowState,
  shouldCompensateVirtualItemSizeChange,
  stableMessageGroupKey,
  transitionScrollFollow,
  type ScrollFollowToken,
} from '@/lib/sessionScrollPolicy'
import { useRunners } from '@/composables/useRunners'
import {
  TOOL_EXECUTION_CONTEXT,
  TOOL_FOLD_INTERACTION,
  provideToolFoldState,
  useToolDisplayMode,
} from '@/composables/useToolDisplay'

/**
 * 会话详情。两种宿主形态(v2.1.0 FR-004/009,档案馆分屏已下线):
 * - mode='archive'(默认):档案馆只读——无输入区/权限交互,底部常驻只读条;流式中会话可只读跟看
 * - mode='workbench':工作台右区列——完整交互(输入/斜杠/权限卡)
 */
const props = defineProps<{
  /** 直接指定会话(工作台列);优先于全局选中 */
  sessionId?: string | null
  mode?: 'archive' | 'workbench'
  /** 赛马模式:隐藏列内输入框(共享输入在底部) */
  hideInput?: boolean
}>()

const { t, locale } = useI18n()
const { confirm: confirmDialog } = useConfirm()

/** 是否可交互(输入/权限决策只存在于工作台,FR-009 档案馆移除渲染而非隐藏) */
const interactive = computed(() => props.mode === 'workbench')

const { projects, loadProjects } = useProjects()
const { selectedSessionId, selectSession } = useSessions()
const { pendingScrollTarget } = useSearch()
const { findSession, removeSession, draftCwd, forkSourceOf } = useWorkbench()
const { goToSession, notifyTransient } = useNotifications()

// 每个实例独立的 detail 数据
const detail = createSessionDetail()
const {
  records,
  loading,
  error,
  recordsReleased,
  loadRecords,
  reloadRecords,
  clearRecords,
  releaseRecords,
} = detail

const { sendMessage, stopStreaming, stopAsyncTask, clearStreamingTurns, clearPendingUserMessage, getStream, removePendingQueueItem, consumePendingQueue, removeLandedTurns, demoteUnlandedTurns } = useStreaming()

const { enabled: htmlVisualEnabled } = useHtmlVisual()
const featureBannerShown = ref(false)
const bannerResumed = ref(false)
const bannerCwd = ref('')

// 横幅自动消失:悬浮通知语义——出现后固定停留 BANNER_MS 淡出,与回合进度无关
const BANNER_MS = 5000
let bannerHideTimer = 0
onUnmounted(() => clearTimeout(bannerHideTimer))
interface HookEvent {
  subtype: 'hook_started' | 'hook_response'
  hook_name: string
  hook_event: string
  output?: string
  exit_code?: number
}
const bannerHookEvents = ref<HookEvent[]>([])

const inputText = ref('')
const scrollContainer = ref<HTMLElement>()
function bindScrollContainer(element: HTMLElement | null) {
  scrollContainer.value = element ?? undefined
}
/** 滚动内容包裹层:布局层滚动跟随的 RO 观察对象(内容总高度的单一载体) */
const scrollContentEl = ref<HTMLElement>()
const composerFieldRef = ref<InstanceType<typeof SessionComposerField>>()
const textareaRef = computed(() => composerFieldRef.value?.element ?? null)

interface SessionConnectedPayload {
  session_id: string
  resumed: boolean
  cwd: string
}

let unlistenConnected: (() => void) | null = null
let unlistenHook: (() => void) | null = null

listen<SessionConnectedPayload>('session-connected', (e) => {
  const p = e.payload
  if (p.session_id === effectiveSessionId.value) {
    bannerResumed.value = p.resumed
    bannerCwd.value = p.cwd
    bannerHookEvents.value = []
    featureBannerShown.value = true
    clearTimeout(bannerHideTimer)
    bannerHideTimer = window.setTimeout(() => { featureBannerShown.value = false }, BANNER_MS)
  }
}).then(fn => { unlistenConnected = fn })

listen<HookEvent & { session_id: string }>('session-hook', (e) => {
  const p = e.payload
  if (p.session_id === effectiveSessionId.value) {
    bannerHookEvents.value.push(p)
  }
}).then(fn => { unlistenHook = fn })

onUnmounted(() => {
  unlistenConnected?.()
  unlistenHook?.()
})

// --- 会话 ID 来源 ---

const effectiveSessionId = computed(() => {
  if (props.sessionId !== undefined) return props.sessionId
  return selectedSessionId.value
})

// per-session 流式状态(v2.1.0:多会话并行,各列独立)
const stream = useSessionStream(effectiveSessionId)
// 异步任务账本在 setup 早期就会被 watch 求值，外部运行态必须先完成初始化。
const externalRunning = ref(false)

// --- tool_result 全局查找表(跨消息配对:tool_use 在 assistant、tool_result 在 user) ---
const toolResultMap = computed(() => {
  const map = new Map<string, ToolResultData>()
  for (const r of records.value) {
    if (r.type !== 'user' || !r.message) continue
    const content = r.message.content
    if (typeof content === 'string') continue
    for (const b of content) {
      if (b.type === 'tool_result') {
        const tr = b as Extract<ContentBlock, { type: 'tool_result' }>
        // recordUuid = tool_result 所在 user record 的 uuid;嵌套图片按此拼协议 URL
        map.set(tr.tool_use_id, { content: tr.content, is_error: tr.is_error, recordUuid: r.uuid })
      }
    }
  }
  for (const turn of stream.value.streamingTurns) {
    for (const tr of turn.toolResults ?? []) {
      // 流式结果经 typed 反序列化,图片 data 已剥离;流式期图片由 records
      // 重载走协议 URL。recordUuid 置 null 仅为类型完整。
      map.set(tr.tool_use_id, { content: tr.content, is_error: tr.is_error, recordUuid: null })
    }
  }
  return map
})
provide('toolResultMap', toolResultMap)

const toolFoldState = provideToolFoldState()
const { toolDisplayMode } = useToolDisplayMode()
const { stickyUserPromptEnabled } = useStickyUserPrompt()

// --- 异步任务面板（后台 Bash / Agent / Workflow / Monitor / Wakeup）---
const {
  allAgents: subAgentList,
  openAgents: subAgentTabs,
  activeTabId: subAgentActiveTabId,
  sidebarOpen: asyncSidebarOpen,
  openSidebar: openAsyncPanel,
  loadSubAgentList,
  findByToolUseId,
  toggleSubAgent,
  closeSidebar: closeAsyncPanel,
  closeTab: closeSubAgentTab,
  closeAllTabs: closeAllSubAgents,
  isAgentOpen,
} = createSubAgentContext()

// 账本：从 records + 流式增量实时推导所有异步任务（发现不再依赖磁盘轮询）；
// workflow 子 agent 清单由磁盘扫描按 runId 关联进条目。
// live = 自有流式 或 自持长活进程存活 或 外部 claude 进程在跑（跟看）。
// processAlive 那条腿兜住「turn 已结束但进程还在跑后台任务（Workflow/子 agent）」——
// 缺它时这类条目会被误判 unknown 掉进"已结束"区
const asyncTasks = computed<AsyncTaskItem[]>(() => {
  const live = stream.value.streaming || stream.value.processAlive || externalRunning.value
  const ledger = buildAsyncLedger(records.value ?? [], stream.value.streamingTurns, live, {
    // 进程代际锚点：live 由自有进程撑起时，前代进程的无终态任务不再恒判"进行中"
    ownProcessStartMs: stream.value.processStartedAtMs,
    externalRunning: externalRunning.value,
  })
  return ledger.map(item =>
    item.species === 'workflow' && item.runId
      ? { ...item, children: subAgentList.value.filter(a => a.workflow_id === item.runId) }
      : item,
  )
})
const asyncActiveCount = computed(() => asyncTasks.value.filter(isActive).length)
const asyncToolStates = computed(() => {
  const states = new Map<string, AsyncTaskItem['state']>()
  for (const item of asyncTasks.value) {
    if (item.toolUseId) states.set(item.toolUseId, item.state)
  }
  return states
})
const stoppingAsyncTaskIds = ref<ReadonlySet<string>>(new Set())
const asyncStopTimers = new Map<string, number>()

function setAsyncTaskStopping(taskId: string, value: boolean) {
  const next = new Set(stoppingAsyncTaskIds.value)
  if (value) next.add(taskId)
  else next.delete(taskId)
  stoppingAsyncTaskIds.value = next
  const timer = asyncStopTimers.get(taskId)
  if (timer !== undefined) window.clearTimeout(timer)
  asyncStopTimers.delete(taskId)
}

watch(asyncTasks, (tasks) => {
  for (const taskId of stoppingAsyncTaskIds.value) {
    const stillActive = tasks.some(task =>
      isActive(task) && (task.taskId === taskId || task.agentId === taskId))
    if (!stillActive) setAsyncTaskStopping(taskId, false)
  }
})

async function onStopAsyncTask(task: AsyncTaskItem) {
  const sessionId = effectiveSessionId.value
  const taskId = asyncTaskStopId(task)
  if (!sessionId || !taskId || stoppingAsyncTaskIds.value.has(taskId)) return
  const rawName = task.title || task.detail || taskId
  const name = rawName.length > 80 ? `${rawName.slice(0, 80)}…` : rawName
  if (!(await confirmDialog(t('asyncTask.stopConfirm', { name }), t('asyncTask.stopConfirmOk')))) return
  if (!asyncTasks.value.some(item => asyncTaskStopId(item) === taskId)) return

  setAsyncTaskStopping(taskId, true)
  try {
    await stopAsyncTask(sessionId, taskId)
    // 正常由终态通知解锁；兜底避免异常 CLI 永久留下 loading 状态。
    asyncStopTimers.set(taskId, window.setTimeout(() => {
      setAsyncTaskStopping(taskId, false)
    }, 10_000))
  } catch (error) {
    setAsyncTaskStopping(taskId, false)
    notifyTransient(t('asyncTask.stopFailed'), String(error))
  }
}

onUnmounted(() => {
  for (const timer of asyncStopTimers.values()) window.clearTimeout(timer)
  asyncStopTimers.clear()
})

/** 自持长活进程忙 = 自发轮在途(live turn 在播):此窗口发消息应排队而非直发——
 *  CLI 串行,直发会打断在播轮渲染且排队"没有回应"(审计遗留①)。
 *  判据必须是「真正占用 CLI 主循环」的在途轮次,不能用账本 active 计数——
 *  armed wakeup 恒 waiting、常驻 Monitor 恒 running,都与主循环空闲无关,
 *  误当忙态会让消息滞留数小时甚至永久(回归审查 R1,已证实) */
const ownProcessBusy = computed(() =>
  stream.value.processAlive && stream.value.streamingTurns.some(t => t.live),
)
const asyncPanelVisible = computed(() => asyncSidebarOpen.value && asyncTasks.value.length > 0)
const asyncPanelRef = ref<InstanceType<typeof AsyncTaskPanel> | null>(null)
const detailRootRef = ref<HTMLElement>()

// ---- 文件账本(v2.6.0):纯前端推导,与异步面板互斥占用右侧手风琴 ----
const { entries: ledgerEntries, modifiedEntries: ledgerModified, readOnlyEntries: ledgerReadOnly } =
  useFileLedger(records, computed(() => stream.value.streamingTurns))
const ledgerPanelOpen = ref(false)
function toggleLedgerPanel() {
  ledgerPanelOpen.value = !ledgerPanelOpen.value
  if (ledgerPanelOpen.value && asyncSidebarOpen.value) closeAsyncPanel()
  if (ledgerPanelOpen.value) runnerDockOpen.value = false
}
// 反向互斥:异步面板打开时收起账本和 runner
watch(asyncSidebarOpen, open => {
  if (open) {
    ledgerPanelOpen.value = false
    runnerDockOpen.value = false
  }
})
const ledgerPanelRef = ref<InstanceType<typeof FileLedgerPanel> | null>(null)
// tool_use 锚点索引:文件工具卡按钮可见性 O(1) 判定(子代理转录的卡不入账,自然隐藏)
const ledgerAnchorSet = computed(() => {
  const s = new Set<string>()
  for (const e of ledgerEntries.value) for (const op of e.ops) s.add(op.anchorId)
  return s
})

provide('findSubAgent', (toolUseId: string) => findByToolUseId(toolUseId))
provide('toggleSubAgent', (meta: SubAgentMeta) => toggleSubAgent(meta))
provide('isSubAgentOpen', (agentId: string) => isAgentOpen(agentId))
// 主对话工具卡（Workflow/后台 Bash 等）直达面板条目：按 toolUseId 打开
provide('openAsyncTask', (toolUseId: string) => {
  if (!asyncTasks.value.some(x => x.toolUseId === toolUseId)) return false
  openAsyncPanel()
  nextTick(() => asyncPanelRef.value?.openByToolUse(toolUseId))
  return true
})
// 文件工具卡(Edit/Write/Read/NotebookEdit)直达账本:推开面板并下钻到该文件时间线
provide('hasLedgerAnchor', (toolUseId: string) => ledgerAnchorSet.value.has(toolUseId))
provide('openFileLedger', (toolUseId: string) => {
  if (!ledgerAnchorSet.value.has(toolUseId)) return false
  if (!ledgerPanelOpen.value) {
    ledgerPanelOpen.value = true
    if (asyncSidebarOpen.value) closeAsyncPanel()
  }
  nextTick(() => ledgerPanelRef.value?.openByAnchor(toolUseId))
  return true
})

// ---- Runner 跑单面板(v2.11.0):与异步/账本三方互斥 ----
const {
  runnerPinned,
  runningCount: runnerRunningCount,
  hasCrashed: runnerHasCrashed,
  setCurrentProject: setRunnerProject,
  stopAllForSession: runnerStopAll,
  loadRunners,
  dialogOpen: runnerDialogOpen,
} = useRunners()
// 停靠形态开关（三方互斥的第三方）
const runnerDockOpen = ref(false)
// 悬浮形态开关
const runnerFloatOpen = ref(false)

/** 切换 runner 面板：当前哪种形态开着就关哪种，全关时默认开悬浮 */
function toggleRunnerPanel() {
  if (runnerDockOpen.value) {
    runnerDockOpen.value = false
  } else if (runnerFloatOpen.value) {
    runnerFloatOpen.value = false
  } else {
    runnerFloatOpen.value = true
  }
  // 面板打开时增量水合
  const sid = currentSession.value?.summary?.id
  if (sid && (runnerDockOpen.value || runnerFloatOpen.value)) {
    loadRunners(sid)
  }
}

function closeRunnerPanel() {
  runnerDockOpen.value = false
  runnerFloatOpen.value = false
}

/** 钉住 = 悬浮面板常驻原位：点外/Esc 不再收起，仅显式点关闭按钮可关 */
function toggleRunnerPin() {
  runnerPinned.value = !runnerPinned.value
}

/** 侧边栏 = 悬浮 ↔ 停靠手风琴切换（与账本/异步侧栏互斥） */
function toggleRunnerDock() {
  if (runnerDockOpen.value) {
    // 停靠→悬浮
    runnerDockOpen.value = false
    runnerFloatOpen.value = true
  } else {
    // 悬浮→停靠
    runnerFloatOpen.value = false
    runnerDockOpen.value = true
    ledgerPanelOpen.value = false
    if (asyncSidebarOpen.value) closeAsyncPanel()
  }
}

// 当前会话 runner 数量（用于顶栏徽标）
const sessionRunnersCount = computed(() => {
  const sid = currentSession.value?.summary?.id
  if (!sid) return 0
  return runnerRunningCount(sid)
})
const sessionHasCrashed = computed(() => {
  const sid = currentSession.value?.summary?.id
  if (!sid) return false
  return runnerHasCrashed(sid)
})

// 提供塞输入框能力给 RunnerPanel（Teleport 不影响 provide/inject 树）
provide('runnerAppendToInput', (text: string) => {
  const existing = inputText.value
  if (existing && !existing.endsWith('\n')) {
    inputText.value = existing + '\n' + text
  } else {
    inputText.value = (existing || '') + text
  }
  nextTick(() => {
    const el = textareaRef.value
    if (!el) return
    // 光标移到末尾
    el.selectionStart = el.selectionEnd = el.value.length
    el.focus()
    // 触发 autoResize
    el.style.height = 'auto'
    el.style.height = Math.min(el.scrollHeight, 160) + 'px'
  })
})

// 悬浮面板 ref + 点击外部关闭
const runnerFloatRef = ref<HTMLElement>()
const runnerToggleBtnRef = ref<HTMLElement>()
function onRunnerClickOutside(e: MouseEvent) {
  if (!runnerFloatOpen.value || runnerPinned.value) return
  if (runnerDialogOpen.value) return
  const target = e.target as Node
  // 豁免面板自身、输入框、顶栏切换按钮
  if (runnerFloatRef.value?.contains(target)) return
  if (textareaRef.value?.contains(target)) return
  if (runnerToggleBtnRef.value?.contains(target)) return
  // 面板跟随会话列：点击落在其他列/其他区域不影响本列面板，仅点击本列内空白才收起
  if (!detailRootRef.value?.contains(target)) return
  closeRunnerPanel()
}
onMounted(() => document.addEventListener('mousedown', onRunnerClickOutside))
onUnmounted(() => document.removeEventListener('mousedown', onRunnerClickOutside))

// 悬浮面板：列容器内 absolute 定位——面板不越列界无裁剪风险，
// 滚动/列宽变化天然跟随，零 JS 定位。高度按列高比例记忆（默认 40%），底边拖拽可调
const RUNNER_FLOAT_HEIGHT_KEY = 'monet-runner-float-height'
const runnerFloatHeightRatio = ref(
  Math.min(Math.max(Number(localStorage.getItem(RUNNER_FLOAT_HEIGHT_KEY)) || 0.4, 0.15), 0.85),
)

function onRunnerResizeStart(e: MouseEvent) {
  e.preventDefault()
  const col = detailRootRef.value
  if (!col) return
  const colH = col.clientHeight || 1
  const startY = e.clientY
  const startRatio = runnerFloatHeightRatio.value
  function onMove(ev: MouseEvent) {
    runnerFloatHeightRatio.value = Math.min(
      Math.max(startRatio + (ev.clientY - startY) / colH, 0.15),
      0.85,
    )
  }
  function onUp() {
    localStorage.setItem(RUNNER_FLOAT_HEIGHT_KEY, String(runnerFloatHeightRatio.value))
    document.removeEventListener('mousemove', onMove)
    document.removeEventListener('mouseup', onUp)
  }
  document.addEventListener('mousemove', onMove)
  document.addEventListener('mouseup', onUp)
}

// 悬浮面板 Esc 关闭（document 级监听，div 不可聚焦无法捕获 keydown）
function onRunnerEscape(e: KeyboardEvent) {
  if (e.key !== 'Escape') return
  if (!runnerFloatOpen.value || runnerPinned.value) return
  if (runnerDialogOpen.value) return
  closeRunnerPanel()
}
watch(runnerFloatOpen, (open) => {
  // pinned 与否由 handler 内判断（pinned 状态可在面板打开期间切换）
  if (open) {
    document.addEventListener('keydown', onRunnerEscape)
  } else {
    document.removeEventListener('keydown', onRunnerEscape)
  }
})
onUnmounted(() => document.removeEventListener('keydown', onRunnerEscape))

// --- 侧栏面板（异步/账本/runner 三方互斥共用）---
const sidePanelVisible = computed(() => asyncPanelVisible.value || ledgerPanelOpen.value || runnerDockOpen.value)
const {
  mounted: sidePanelDom,
  expanded: sidePanelExpanded,
  targetWidth: sidebarTargetWidth,
} = useSessionSidePanelHost(sidePanelVisible, {
  rootRef: detailRootRef,
  close: () => {
    closeAsyncPanel()
    ledgerPanelOpen.value = false
    runnerDockOpen.value = false
  },
})

// 流式结束补扫转录清单：晚落盘的 agent meta / workflow children 兜底
// （任务发现本身由账本从 records 实时推导，不依赖此扫描）
watch(() => stream.value.streaming, (streaming, was) => {
  if (was && !streaming) {
    const cs = currentSession.value
    if (cs) loadSubAgentList(cs.projectId, cs.summary.id)
  }
})

// --- 斜杠命令(FR-004)状态 ---

const { assets: workshopAssets, ensureLoaded: ensureWorkshopLoaded } = useWorkshop()
ensureWorkshopLoaded()

const cursorPos = ref(0)

/** /model invalid 等校验失败提示 */
const slashError = ref<string | null>(null)

/** 非错误提示（如「已在终端打开」），muted 样式 */
const slashNotice = ref<string | null>(null)

/** /help 帮助卡片显示标志(前端层面,不写 jsonl) */
const showHelpCard = ref(false)

/** /clear 时设置:仅前端层面隐藏历史消息,刷新或切换会话恢复 */
const hideHistory = ref(false)

const slashPanelVisible = computed(() =>
  shouldTriggerPanel(inputText.value, cursorPos.value),
)

const allSlashCommands = computed(() =>
  getAllCommands(workshopAssets.value?.skills, workshopAssets.value?.commands),
)

function autoResize() {
  const el = textareaRef.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = Math.min(el.scrollHeight, 160) + 'px'
}

function syncCursor() {
  const el = textareaRef.value
  if (el) cursorPos.value = el.selectionStart ?? 0
}

function onInputChange() {
  autoResize()
  syncCursor()
  if (slashError.value) slashError.value = null
  if (slashNotice.value) slashNotice.value = null
  // textarea 增高缩小了 scrollContainer 的 clientHeight,但 contentRO 不触发(内容高度没变),
  // 原来的 scrollTop 不够贴底——看起来内容被输入框顶上去了。跟随模式下补偿一次贴底
  if (followStreaming.value) scrollToBottom()
}

// 会话切换时复位 /clear 与 /help 的视图标志,滚动恢复跟随
// (流式区已按会话隔离,无需也不应清流式数据——切回时继续展示)
watch(effectiveSessionId, () => {
  toolFoldState.reset()
  hideHistory.value = false
  showHelpCard.value = false
  slashNotice.value = null
  resumeFollow(true)
  featureBannerShown.value = false
  bannerResumed.value = false
  bannerCwd.value = ''
  bannerHookEvents.value = []
  lastScrollTop = 0
})

// --- 权限请求(仅工作台列交互;档案馆只读不渲染) ---
const permissionRequest = currentForSession(effectiveSessionId)
const { respondRequest } = usePermissionRequests()

const permissionToolUseId = computed(() => findPendingPermissionToolUseId(
  stream.value.streamingTurns.map(turn => turn.content),
  permissionRequest.value,
  toolResultMap.value,
))

provide(TOOL_EXECUTION_CONTEXT, {
  results: toolResultMap,
  asyncStates: asyncToolStates,
  permissionRequest: computed(() => permissionRequest.value
    ? {
        toolUseId: permissionToolUseId.value,
        toolName: permissionRequest.value.toolName,
        input: permissionRequest.value.input,
      }
    : null),
})

/** 按工具分发卡片:提问/计划批准走专用交互卡,其余走通用权限卡 */
const requestCard = computed(() => {
  switch (permissionRequest.value?.toolName) {
    case 'AskUserQuestion': return QuestionCard
    case 'ExitPlanMode': return PlanApprovalCard
    default: return PermissionCard
  }
})

async function onPermissionDecide(
  decision: import('@/composables/usePermissionRequests').PermissionDecision,
  extra?: RespondExtra,
) {
  const req = permissionRequest.value
  if (req) await respondRequest(req.requestId, decision, extra)
}

async function onStop() {
  const sid = effectiveSessionId.value
  if (!sid) return
  // 与按钮文案同判据:自家流式优先走温和停止,仅纯外部运行才走终止链路
  if (externalRunning.value && !stream.value.streaming) {
    if (stopping.value) return
    // 外部进程不是我们 spawn 的:终止前确认,并说明归属方(可能正在生成,杀了会丢那一轮)
    const owner = externalOwner.value
    const msg = owner
      ? t('session.killExternalConfirmBy', { owner })
      : t('session.killExternalConfirm')
    if (!(await confirmDialog(msg, t('session.killExternalOk')))) return
    stopping.value = true
    // 兜底须覆盖探测节拍(3s) + 进程收尾落盘的最坏耗时，过短会在横幅消失前解锁按钮
    stoppingTimeout = window.setTimeout(() => {
      stopping.value = false
      stoppingTimeout = null
    }, 12000)
    try {
      await invoke('kill_external_session', { sessionId: sid })
    } catch {
      stopping.value = false
    }
    return
  }
  // 自发轮(后台唤醒轮):interrupt 只打断当前在跑的这一轮,进程内已武装的
  // 定时唤醒不受影响,到点仍会再次触发——toast 说清边界,彻底断根走列头关闭
  const autoTurnOnly = !stream.value.streaming && ownProcessBusy.value
  await stopStreaming(sid)
  if (autoTurnOnly) notifyTransient(t('session.autoTurnStopped'), t('session.autoTurnStoppedHint'))
}

// --- 会话级设置(模型 / 努力等级 / 渠道) ---
const { settings, setModel, setEffort, setFastMode, setChannel, setChrome, setExtraArgs, setPermissionMode: persistPermissionMode } = useSessionSettings(effectiveSessionId)
const fastModeNotice = ref<string | null>(null)
let unlistenFastModeStatus: (() => void) | null = null

listen<{ session_id: string; active: boolean }>('fast-mode-status', event => {
  if (event.payload.session_id !== effectiveSessionId.value) return
  setFastMode(event.payload.active)
  fastModeNotice.value = event.payload.active ? null : t('topbar.fastModeFallback')
}).then(unlisten => { unlistenFastModeStatus = unlisten })

watch(effectiveSessionId, () => { fastModeNotice.value = null })
onUnmounted(() => unlistenFastModeStatus?.())

// 运行配置同源解析：CLI/项目值只供 display，Monet 显式意图才进入 launch
const runConfigCwd = computed(() => {
  const sid = effectiveSessionId.value
  if (!sid) return null
  const session = projects.value.flatMap(project => project.sessions).find(item => item.id === sid)
  return session?.cwd ?? draftCwd(sid) ?? null
})
const { runConfig } = useRunConfig(settings, runConfigCwd)

function onModelChange(modelId: string | null) {
  setModel(modelId)
}

function onEffortChange(effort: 'low' | 'medium' | 'high' | 'xhigh' | 'max' | 'ultracode' | null) {
  setEffort(effort)
}

function onFastModeChange(fastMode: boolean) {
  fastModeNotice.value = null
  setFastMode(fastMode)
}

// --- 渠道(per-session 选择 + 切换横线记账) ---

// 渠道名解析(badge/横线)需要清单:实例创建时拉一次,下拉每次打开还会各自重读
refreshChannels()

/** 解析后的最终注入渠道 id(null = 官方/零注入):发送与终端恢复共用 */
const resolvedChannelId = computed(() => runConfig.value.channelId)

// --- 等级徽章:流式轮次真实模型伪装的角色(当前解析渠道映射反查) ---
const { channels: channelList } = useChannels()
const activeModelEnv = computed(() => {
  const id = resolvedChannelId.value
  return id ? channelList.value.find(c => c.id === id)?.modelEnv : undefined
})
/** 真实模型字符串 → 伪装等级文本(官方渠道/无映射返回 null) */
function modelTierOf(model: string | null | undefined): string | null {
  const roles = resolveMappedRoles(model, activeModelEnv.value)
  return roles.length ? roles.map(r => ROLE_DISPLAY[r]).join('/') : null
}

function onChannelChange(channelId: string | null) {
  const list = messages.value
  const last = list.length > 0 ? list[list.length - 1] : null
  setChannel(channelId, last?.uuid ?? null)
}

function onChromeChange(chrome: boolean) {
  setChrome(chrome)
  // --chrome 是启动参数,进程重启在下一条消息由 needs_restart 自动触发
  slashNotice.value = t(chrome ? 'session.chromeEnabled' : 'session.chromeDisabled')
}

function onExtraArgsChange(extraArgs: string) {
  setExtraArgs(extraArgs)
  slashNotice.value = t('session.extraArgsChanged')
}

function onPermissionModeChange(mode: import('@/composables/useSessionSettings').PermissionMode | null) {
  persistPermissionMode(mode)
}

/** 当前消息流里出现的 uuid 集合:判断 mark 锚点是否还在视图内 */
const messageUuidSet = computed(() => {
  const set = new Set<string>()
  for (const m of messages.value) if (m.uuid) set.add(m.uuid)
  return set
})

/**
 * 切换横线按锚点消息 uuid 分组(null = 会话起点的切换)。
 * 锚点消息已不在视图内(被 api_error 折叠吞掉 / 流式中切换尚未落盘 / /clear 隐藏历史)的 mark
 * 不进此表,改由 unanchoredChannelMarks 在消息流末尾兜底渲染——绝不静默消失。
 */
const channelMarksByUuid = computed(() => {
  const map = new Map<string | null, ChannelMark[]>()
  for (const m of settings.value.channelMarks) {
    if (m.afterUuid !== null && !messageUuidSet.value.has(m.afterUuid)) continue
    const group = map.get(m.afterUuid)
    if (group) group.push(m)
    else map.set(m.afterUuid, [m])
  }
  return map
})

/** 锚点已失效的切换横线:统一在消息流末尾按顺序兜底渲染 */
const unanchoredChannelMarks = computed(() =>
  settings.value.channelMarks.filter(
    m => m.afterUuid !== null && !messageUuidSet.value.has(m.afterUuid),
  ),
)

function channelMarkLabel(m: ChannelMark): string {
  if (m.channelId === null) return t('session.channelSwitchedDefault')
  if (m.channelId === OFFICIAL_CHANNEL_ID) return t('session.channelSwitchedOfficial')
  return t('session.channelSwitched', { name: channelDisplayName(m.channelId) })
}

// --- 会话数据 ---

function onDeleted() {
  const sid = effectiveSessionId.value
  // 删除会话时停止该会话下所有 runner（FR-007 生命周期治理）
  if (sid) runnerStopAll(sid)
  // 在工作台中的会话被删除:一并移出工作台(FR-009)
  if (sid && findSession(sid)) {
    removeSession(sid)
  }
  if (!props.sessionId) {
    selectSession(null)
  }
  clearRecords()
  loadProjects()
}

/** 草稿会话(应用内新建未落盘)的合成 summary:首条消息落盘后自动让位真实数据 */
function draftSummary(id: string, cwd: string): SessionSummary {
  return {
    id,
    title: t('session.newSessionTitle'),
    first_user_message: null,
    model: null,
    git_branch: null,
    cwd,
    version: null,
    timestamp: null,
    last_modified: Math.floor(Date.now() / 1000),
    total_tokens: {
      input_tokens: 0,
      output_tokens: 0,
      cache_creation_input_tokens: 0,
      cache_read_input_tokens: 0,
    },
    subagent_tokens: {
      input_tokens: 0,
      output_tokens: 0,
      cache_creation_input_tokens: 0,
      cache_read_input_tokens: 0,
    },
    file_size: 0,
    message_count: 0,
    context_window: null,
  }
}

const currentSession = computed<{ summary: SessionSummary; projectId: string } | null>(() => {
  const sid = effectiveSessionId.value
  if (!sid) return null
  for (const p of projects.value) {
    const s = p.sessions.find(s => s.id === sid)
    if (s) return { summary: s, projectId: p.id }
  }
  // 工作台草稿:磁盘尚无 jsonl,合成最小 summary;projectId 按 CLI 同款编码规则推导
  const cwd = draftCwd(sid)
  if (cwd) return { summary: draftSummary(sid, cwd), projectId: cwdToProjectId(cwd) }
  return null
})
provide(SESSION_FILE_ROOT, computed(() => currentSession.value?.summary.cwd))

// 会话切换时设置 runner 项目上下文 + 增量水合
// （immediate watch 会在注册当下同步执行 getter，必须位于 currentSession 定义之后）
watch(() => currentSession.value?.summary?.id, (sid) => {
  const cwd = currentSession.value?.summary?.cwd
  setRunnerProject(cwd ?? null)
  if (sid) loadRunners(sid)
}, { immediate: true })

/** 分叉垫底激活:草稿未落盘且有分叉意图(此时历史区显示的是源会话垫底数据);
 *  落盘后 drafts/forkIntents 随 pruneDrafts 收割,标注自动消失 */
const forkBadgeSource = computed(() => {
  const sid = effectiveSessionId.value
  if (!sid || !draftCwd(sid)) return null
  return forkSourceOf(sid)
})

// 图片输入:粘贴绑 textarea;拖拽收图区放大到整个详情面板(多列并排时拖到哪列进哪列),仅可输入时生效
function bindDetailRoot(element: HTMLElement | null) {
  detailRootRef.value = element ?? undefined
}
const renderSurfaceId = Symbol('session-detail-render-surface')
let renderVisibilityObserver: IntersectionObserver | null = null
let registeredRenderSessionId: string | null = null
const renderSurfaceVisible = ref(false)
let historyReleaseTimer: number | null = null
const HISTORY_RELEASE_DELAY_MS = 20_000
const HISTORY_RELEASE_MIN_RECORDS = 250

function cancelHistoryRelease() {
  if (historyReleaseTimer !== null) window.clearTimeout(historyReleaseTimer)
  historyReleaseTimer = null
}

function restoreReleasedHistory() {
  cancelHistoryRelease()
  if (!recordsReleased.value || loading.value) return
  const cs = currentSession.value
  if (!cs) return
  void loadRecords(
    cs.projectId,
    cs.summary.id,
    true,
    forkSourceOf(cs.summary.id) ?? undefined,
  ).then(() => {
    if (followStreaming.value) scrollToBottom()
  })
}

function scheduleHistoryRelease() {
  cancelHistoryRelease()
  if (
    renderSurfaceVisible.value
    || props.mode !== 'workbench'
    || records.value.length < HISTORY_RELEASE_MIN_RECORDS
  ) return
  historyReleaseTimer = window.setTimeout(() => {
    historyReleaseTimer = null
    // 用户主动停在历史位置时保留阅读上下文；只回收仍处于贴底跟随的屏外列。
    if (renderSurfaceVisible.value || !followStreaming.value || loading.value) return
    deferredRecords = null
    releaseRecords()
  }, HISTORY_RELEASE_DELAY_MS)
}

function detachRenderSurface() {
  cancelHistoryRelease()
  renderSurfaceVisible.value = false
  renderVisibilityObserver?.disconnect()
  renderVisibilityObserver = null
  if (registeredRenderSessionId) {
    removeSessionRenderSurface(registeredRenderSessionId, renderSurfaceId)
    registeredRenderSessionId = null
  }
}

watch(
  [detailRootRef, effectiveSessionId],
  ([el, sid]) => {
    detachRenderSurface()
    if (!el || !sid) return
    registeredRenderSessionId = sid
    setSessionRenderSurface(sid, renderSurfaceId, false)
    renderVisibilityObserver = new IntersectionObserver(([entry]) => {
      // 被 v-show 隐藏、横向完全移出视口、或宿主窗口不可见时均降为后台节奏。
      const visible = !!entry?.isIntersecting && entry.intersectionRatio > 0
      renderSurfaceVisible.value = visible
      setSessionRenderSurface(sid, renderSurfaceId, visible)
      if (visible) restoreReleasedHistory()
      else scheduleHistoryRelease()
    }, { threshold: [0, 0.01] })
    renderVisibilityObserver.observe(el)
  },
  { immediate: true, flush: 'post' },
)

// IntersectionObserver 往往先于 JSONL 加载完成回报不可见；记录到达后补排冷卸载。
watch(() => records.value.length, (count) => {
  if (count >= HISTORY_RELEASE_MIN_RECORDS && !renderSurfaceVisible.value) {
    scheduleHistoryRelease()
  }
})

function activateRenderSurface() {
  const sid = effectiveSessionId.value
  if (sid) markSessionRenderActive(sid)
}

onUnmounted(detachRenderSurface)
const imageDropArea = computed<HTMLElement | null | undefined>(() =>
  interactive.value && !props.hideInput && currentSession.value?.summary.cwd
    ? detailRootRef.value
    : null,
)
const imageInput = useImageInput({ pasteTarget: textareaRef, dropTarget: imageDropArea })
onMounted(() => imageInput.attach())
const composerQueueItems = computed<ComposerQueueItem[]>(() => stream.value.pendingQueue.map((item, index) => ({
  id: String(index),
  text: item.message,
  imageCount: item.opts.images?.length ?? 0,
})))

function removeComposerQueueItem(id: string) {
  if (!effectiveSessionId.value) return
  removePendingQueueItem(effectiveSessionId.value, Number(id))
}

// 会话级图片定位上下文(主会话,无 agentId);历史区图片按此拼 ccimg 协议 URL
const imageLocator = computed<ImageLocator | null>(() => {
  const cs = currentSession.value
  if (!cs) return null
  return { projectId: cs.projectId, sessionId: cs.summary.id }
})
provide(IMAGE_LOCATOR, imageLocator)

// --- 只读条(FR-009,仅档案馆) ---

const workbenchHome = computed(() => {
  const sid = effectiveSessionId.value
  if (!sid) return null
  return findSession(sid)?.tab ?? null
})

function onOpenInWorkbench() {
  const sid = effectiveSessionId.value
  if (sid) goToSession(sid)
}

const { getMeta, updateMeta } = useSessionMeta()
const summaryGenerating = ref(false)

const currentSummary = computed(() => {
  const sid = effectiveSessionId.value
  return sid ? getMeta(sid)?.summary : undefined
})
const archiveTitle = computed(() => {
  const session = currentSession.value?.summary
  return session ? displayTitle(session, getMeta(session.id)?.title) : ''
})

async function onGenerateSummary() {
  const cs = currentSession.value
  if (!cs || summaryGenerating.value) return
  summaryGenerating.value = true
  try {
    const summary = await invoke<string>('generate_summary', { projectId: cs.projectId, sessionId: cs.summary.id })
    await updateMeta(cs.summary.id, { summary } as any)
  } catch (e) {
    console.warn('[meta] 摘要生成失败:', e)
  } finally {
    summaryGenerating.value = false
  }
}

/**
 * 最近一条「真实」assistant 记录(用于推断模型与上下文占用)。
 * 跳过 model 为 <synthetic> 的合成消息(CLI 本地生成的 API Error 占位等)——
 * 它们不代表会话用的模型,且 usage 全 0 会把上下文进度打回零。
 */
const lastAssistantRecord = computed(() => {
  for (let i = records.value.length - 1; i >= 0; i--) {
    const r = records.value[i]
    if (r.type === 'assistant' && r.message && r.message.model !== '<synthetic>') return r
  }
  return null
})

/** 最近 assistant 跑过的真实 model 字符串(含 [1m] 后缀如有);为空时 fallback 到 summary.model */
const lastAssistantModel = computed<string | null>(() => {
  return lastAssistantRecord.value?.message?.model ?? null
})

/**
 * 顶栏展示用 model 字符串。summary.model 兜底也可能是 <synthetic>
 * (整个会话只有合成记录时),过滤防止它被当自定义模型展示甚至被选用。
 */
const displayModelString = computed<string | null>(() => {
  const m = lastAssistantModel.value ?? currentSession.value?.summary.model ?? null
  return m === '<synthetic>' ? null : m
})

/**
 * 已占用上下文 token 数 = 最近一次 assistant 响应的 input_tokens + cache_read_input_tokens。
 *
 * 这两项之和反映"模型这次实际看到的 prompt token 总量",是"下次请求即将占用上下文容量"
 * 的真实近似。不能用 SessionSummary.total_tokens 累加 4 类——那是计费统计量,
 * cache_read 累计会让长会话出现几十 M 的虚高。
 */
const lastReportedAssistantUsage = computed(() => {
  for (let i = records.value.length - 1; i >= 0; i--) {
    const record = records.value[i]
    if (record.type === 'assistant' && hasReportedUsage(record.message?.usage)) {
      return record.message?.usage ?? null
    }
  }
  return null
})

const lastAssistantContextSize = computed<number>(() => {
  const usage = lastReportedAssistantUsage.value
  if (!usage) return 0
  return usage.input_tokens + usage.cache_read_input_tokens
})

const pendingUserBlocks = computed<ContentBlock[]>(() => {
  const blocks: ContentBlock[] = []
  if (stream.value.pendingImages?.length) {
    for (const img of stream.value.pendingImages) {
      blocks.push({ type: 'image', source: img.source } as ContentBlock)
    }
  }
  if (stream.value.pendingUserMessage) {
    blocks.push({ type: 'text', text: stream.value.pendingUserMessage })
  }
  return blocks
})

/** 流式区当前渲染的 message id 集合(用于历史区过滤,避免与流式区重复显示) */
const streamingMessageIds = computed(() =>
  new Set(stream.value.streamingTurns.map(t => t.messageId)),
)

/** 存在传输中的自发轮(task-notification 后台收尾轮,streaming=false 下进行):
 *  typing-dots 等进行中指示据此补位,不显示"空闲下凭空吐内容" */
const hasLiveTurn = computed(() => stream.value.streamingTurns.some(t => t.live))

/** 流式树完成时先补齐尾部元信息；落账切换到历史树后由 JSONL 时间戳接管。 */
const streamResponseCompletedAt = ref<string | null>(null)
watch(
  [effectiveSessionId, () => stream.value.streaming, hasLiveTurn],
  ([sessionId, streaming, live], [previousSessionId, previousStreaming, previousLive]) => {
    if (sessionId !== previousSessionId || streaming || live) {
      streamResponseCompletedAt.value = null
      return
    }
    if (previousStreaming || previousLive) streamResponseCompletedAt.value = new Date().toISOString()
  },
)

/** 进入消息流的 system 子类型（其余 system 记录为噪音，不渲染） */
const VISIBLE_SYSTEM_SUBTYPES = new Set(['api_error', 'compact_boundary'])

/** 剥离 CLI 落账时并入 user 消息的私有标签注入(hook additionalContext /
 *  system-reminder / command 包装等),留下真实用户文本——落账匹配用。
 *  精确全等对带注入的消息是结构性必然失配,不是偶发。 */
function stripInjections(text: string): string {
  return text.replace(TAG_RE, '')
}

/** 在 recs 中寻找 pending 用户消息对应的落账 user record uuid。
 *  只认发送时刻之后的记录(5s 时钟容差,records 为追加序、扫到更早即停)。
 *  三层匹配(审计遗留③——TAG_RE 白名单失配曾致气泡与历史长期双显):
 *  1. 命令形态:透传斜杠命令落账为 <command-name> 包装,按命令名比对(全等必失配);
 *  2. 精确层:剥离注入 + 空白折叠归一后相等;纯图片消息按含 image 块匹配;
 *  3. 宽松兜底:精确失败时,认领发送时刻之后剥离注入仍有实文的最新 user record——
 *     失配也能收敛,气泡不至长驻;误认领仅提前退场气泡,消息本体已在历史区,不丢。 */
function findLandedUserUuid(
  recs: SessionRecord[],
  pendingText: string | null,
  hasImages: boolean,
  sentAt: number,
): string | null {
  if (!pendingText && !hasImages) return null
  const norm = (s: string) => s.replace(/\s+/g, ' ').trim()
  const target = norm(pendingText ?? '')
  const cmdName = target.startsWith('/') ? target.slice(1).split(' ')[0] : null
  let fallback: string | null = null
  for (let i = recs.length - 1; i >= 0; i--) {
    const r = recs[i]
    if (r.type !== 'user') continue
    const ts = r.timestamp ? Date.parse(r.timestamp) : 0
    if (ts && sentAt && ts < sentAt - 5000) break
    const content = r.message?.content
    if (!content) continue
    const rawText = typeof content === 'string'
      ? content
      : content
          .filter((b: ContentBlock) => b.type === 'text')
          .map(b => (b as { text?: string }).text ?? '')
          .join('')
    if (!target) {
      if (typeof content !== 'string' && content.some((b: ContentBlock) => b.type === 'image')) return r.uuid
      continue
    }
    // 命令层要求 ts >= sentAt:落账由 CLI 收到消息后写盘(同机时钟必晚于发送),
    // 无下界会让 5s 容差窗内的旧同名命令记录提前认领新命令气泡(回归审查 R4)
    if (cmdName && ts >= sentAt) {
      const m = rawText.match(/<command-name>\s*\/?([^<\s]+)\s*<\/command-name>/)
      if (m && m[1] === cmdName) return r.uuid
    }
    const stripped = norm(stripInjections(rawText))
    if (stripped === target) return r.uuid
    if (!fallback && ts >= sentAt && stripped) fallback = r.uuid
  }
  return fallback
}

/** pending 用户消息在历史区的落账 record uuid(落账接管信号)。
 *  非 null = 历史条已可渲染:气泡同帧让位(模板 v-if),watch 随后清理状态。
 *  与旧实现方向相反——旧逻辑匹配成功隐藏历史条、reload 无条件清气泡,
 *  匹配失配 + 误清叠加出「两源皆空」的消息消失窗口。 */
const pendingLandedUuid = computed(() => {
  const s = stream.value
  return findLandedUserUuid(records.value, s.pendingUserMessage, !!s.pendingImages?.length, s.pendingSentAt ?? 0)
})

const streamingArtifactCandidates = computed(() => {
  if (stream.value.streaming) return []
  return detectContentBlockArtifacts(stream.value.streamingTurns.flatMap(turn => turn.content))
})

// 落账接管后清理 pending 状态(显示切换已由 v-if 原子完成,这里只是后勤)
watch(pendingLandedUuid, (uuid) => {
  if (uuid) {
    const sid = effectiveSessionId.value
    if (sid) clearPendingUserMessage(sid)
  }
})

const messages = computed(() => {
  // thinking 耗时标注:在过滤前的原始序列上算(前一行可能是不可见的 tool_result 行)
  annotateThinkingMs(records.value)
  const visible = records.value.filter(
    (r): r is Extract<SessionRecord, { type: 'user' | 'assistant' | 'system' }> => {
      if (r.type === 'assistant') {
        const msgId = r.message?.id
        if (msgId && streamingMessageIds.value.has(msgId)) return false
        return true
      }
      if (r.type === 'system') {
        return !!r.subtype && VISIBLE_SYSTEM_SUBTYPES.has(r.subtype)
      }
      if (r.type !== 'user') return false
      const content = r.message?.content
      if (!content || typeof content === 'string') return true
      return content.some((b: ContentBlock) => b.type !== 'tool_result')
    },
  )
  return visible.filter((r, i) => {
    if (r.type !== 'system' || r.subtype !== 'api_error') return true
    const next = visible[i + 1]
    return !(next?.type === 'system' && next.subtype === 'api_error')
  })
})

type VisibleRecord = Extract<SessionRecord, { type: 'user' | 'assistant' | 'system' }>

interface MsgGroup {
  user: VisibleRecord | null
  responses: VisibleRecord[]
}

/** 同 message.id 的连续 assistant 记录合并为单条（CLI 每个 content block 单独写一行） */
function mergeResponses(responses: VisibleRecord[]): VisibleRecord[] {
  const merged: VisibleRecord[] = []
  for (const r of responses) {
    if (r.type !== 'assistant') { merged.push(r); continue }
    const msgId = (r as any).message?.id
    const prev = merged.length ? merged[merged.length - 1] : null
    if (msgId && prev?.type === 'assistant' && (prev as any).message?.id === msgId) {
      const prevMsg = (prev as any).message
      const curMsg = (r as any).message
      prevMsg.content = [...prevMsg.content, ...curMsg.content]
      if (shouldReplaceUsage(prevMsg.usage, prevMsg.stop_reason, curMsg.usage, curMsg.stop_reason)) {
        prevMsg.usage = curMsg.usage
        prevMsg.stop_reason = curMsg.stop_reason
      }
      // 合并块的 timestamp 是首行落盘时间(≈回复开始);末行时间(≈回复完成)另存,组末尾标注用
      ;(prev as any)._lastTs = (r as any).timestamp ?? (prev as any)._lastTs
    } else {
      merged.push({ ...r, message: r.type === 'assistant' && r.message ? { ...r.message, content: [...r.message.content] } : r.message } as any)
    }
  }
  return merged
}

/** 思考耗时标注的合理上限:超过按异常丢弃(跨会话恢复/时钟漂移的脏差值) */
const THINKING_MS_CAP = 600_000

/**
 * 历史区 thinking 块耗时标注:必须在**原始记录序列**上按「与前一行的时间戳差」计算——
 * thinking 行落盘 ≈ 思考结束,前一行落盘 ≈ 思考开始(实测中位 10.8s,与思考时长量级吻合);
 * 「与后一行的差」是下一块的生成间隔(中位 0.6s),曾错标于此导致耗时几乎全被 <1s 显示阈值吞掉。
 * 前一行可能是 tool_result/user 行(过滤后不可见),故不能在 messages/mergeResponses 层算。
 * 幂等:已有 _thinkingMs(流式期 Date.now() 实测值,更准)不覆盖。
 */
function annotateThinkingMs(rows: SessionRecord[]): void {
  let prevTs: number | null = null
  for (const r of rows) {
    const tsStr = (r as any).timestamp as string | null
    const ts = tsStr ? new Date(tsStr).getTime() : NaN
    if (r.type === 'assistant' && prevTs !== null && Number.isFinite(ts)) {
      const content = (r as any).message?.content
      if (Array.isArray(content)) {
        const ms = ts - prevTs
        if (ms > 0 && ms < THINKING_MS_CAP) {
          for (const b of content) {
            if (b?.type === 'thinking' && !b._thinkingMs) b._thinkingMs = ms
          }
        }
      }
    }
    if (Number.isFinite(ts)) prevTs = ts
  }
}

const messageGroups = computed(() => {
  const groups: MsgGroup[] = []
  let cur: MsgGroup = { user: null, responses: [] }
  for (const msg of messages.value) {
    if (msg.type === 'user') {
      if (cur.user || cur.responses.length) groups.push(cur)
      cur = { user: msg, responses: [] }
    } else {
      cur.responses.push(msg)
    }
  }
  if (cur.user || cur.responses.length) groups.push(cur)
  return groups.map(g => ({ ...g, responses: mergeResponses(g.responses) }))
})

// ---- 消息组虚拟化(Task 2 主体) ----
// 末组豁免:虚拟化只管 messageGroups[0..n-2],末组(messageGroups[n-1])独立铺——
// 保留 anchorRO/contentRO/追随滚动/pinLastGroupBeforeSwap 全套现有语义;
// 用户视线主要在末组,末组永在视口内,豁免代价接近零。
const renderGroups = computed(() => {
  const gs = messageGroups.value
  return gs.length > 1 ? gs.slice(0, -1) : []
})
const lastGroup = computed(() => {
  const gs = messageGroups.value
  return gs.length > 0 ? gs[gs.length - 1] : null
})
const lastGroupIndex = computed(() => messageGroups.value.length - 1)

function messageGroupKey(group: MsgGroup, index: number): string {
  return stableMessageGroupKey(
    effectiveSessionId.value ?? 'no-session',
    group,
    index,
  )
}

// key 既供 Vue DOM diff，也供 virtualizer 的测量缓存使用。把 session scope
// 带进 key，避免同一个详情组件切会话后按 index 复用上一会话的高度。
const renderGroupKeys = computed(() =>
  renderGroups.value.map((group, index) => messageGroupKey(group, index)),
)
const groupKeySnapshot = shallowRef<readonly string[]>([])
const groupKeyExtractor = shallowRef<(index: number) => string>(
  index => JSON.stringify(['no-session', `missing:${index}`]),
)

// 流式最后一组每个 tick 都会重建派生数组，但历史组 key 通常未变。只有 key
// 序列真的变化时才换 extractor 引用，避免 virtual-core 反复失效整批 measurement。
watch(renderGroupKeys, (keys) => {
  const previous = groupKeySnapshot.value
  if (keys.length === previous.length && keys.every((key, index) => key === previous[index])) return
  const snapshot = [...keys]
  const sessionId = effectiveSessionId.value ?? 'no-session'
  groupKeySnapshot.value = snapshot
  groupKeyExtractor.value = index => snapshot[index] ?? JSON.stringify([
    sessionId,
    `missing:${index}`,
  ])
}, { immediate: true, flush: 'sync' })

// 末组从直铺区迁入虚拟区前先保存真高；estimateSize 首帧直接使用，避免
// 4000px 长回复先退回 200px、WebKit clamp scrollTop 后再校正的中间帧。
const groupHeightEstimates = new Map<string, number>()
const virtualBoxRef = ref<HTMLElement>()
let virtualBoxOrigin = 0
let virtualBoxOriginReady = false

function updateVirtualBoxOrigin(): void {
  const sc = scrollContainer.value
  const box = virtualBoxRef.value
  if (!sc || !box) {
    virtualBoxOrigin = 0
    virtualBoxOriginReady = false
    return
  }
  virtualBoxOrigin = box.getBoundingClientRect().top
    - sc.getBoundingClientRect().top
    + sc.scrollTop
  virtualBoxOriginReady = true
}

function measureMessageGroupElement(
  element: Element,
  entry: ResizeObserverEntry | undefined,
  instance: Virtualizer<HTMLElement, Element>,
): number {
  const index = instance.indexFromElement(element)
  if (!virtualBoxOriginReady) {
    const sc = scrollContainer.value
    const box = element.parentElement
    if (sc && box) {
      virtualBoxOrigin = box.getBoundingClientRect().top
        - sc.getBoundingClientRect().top
        + sc.scrollTop
      virtualBoxOriginReady = true
    }
  }
  const size = measureVirtualElement(element, entry, instance)
  const key = instance.options.getItemKey(index)
  if (size > 0) groupHeightEstimates.set(String(key), size)
  return size
}

const scrollMessageVirtualizer = (
  offset: number,
  options: { adjustments?: number; behavior?: ScrollBehavior },
  instance: Virtualizer<HTMLElement, Element>,
) => {
  const sc = instance.scrollElement as HTMLElement | null
  elementScroll(offset, options, instance)
  if (sc) rememberProgrammaticScroll(sc, 'virtualizer')
}

// 虚拟化启用阈值:useVirtualizationSettings 共享 ref(SettingsView 里可调)
const { threshold: virtualizationThreshold } = useVirtualizationSettings()
const shouldVirtualize = computed(() => renderGroups.value.length > virtualizationThreshold.value)

// tanstack-vue-virtual:count/estimateSize 走 getter 保持 reactive。
// estimateSize 首帧粗估 200px/组,measureElement 挂载后自动校正真高;
// gap=16 与非虚拟路径 space-y-4 对齐，避免相邻轮次轨道首尾相接；
// overscan=5 覆盖上下 5 组,兼顾滚动流畅度与 DOM 节点数
const messageVirtualizer = useVirtualizer(
  computed(() => ({
    count: renderGroups.value.length,
    getScrollElement: () => scrollContainer.value ?? null,
    getItemKey: groupKeyExtractor.value,
    estimateSize: (index: number) =>
      groupHeightEstimates.get(String(groupKeyExtractor.value(index))) ?? 200,
    measureElement: measureMessageGroupElement,
    scrollToFn: scrollMessageVirtualizer,
    gap: 16,
    overscan: 5,
  })),
)

// virtual-core 的默认“首次测量”只判断 item.start 在视口上方，即使用户正在
// 上滚也会把 actual-estimate 直接加到 scrollTop。统一收紧为：手势静止、
// 旧测量盒完整位于虚拟区视口上方时才允许补偿。
messageVirtualizer.value.shouldAdjustScrollPositionOnItemSizeChange = (item, delta, instance) =>
  shouldCompensateVirtualItemSizeChange({
    scrollDirection: instance.scrollDirection,
    upwardGestureActive: performance.now() - wheelUpIntentAt < 220,
    itemStart: item.start,
    itemSize: item.size,
    scrollOffset: Math.max(0, (instance.scrollOffset ?? 0) - virtualBoxOrigin),
    delta,
  })

watch(virtualBoxRef, (box) => {
  if (box) void nextTick(updateVirtualBoxOrigin)
}, { immediate: true })

watch(shouldVirtualize, (enabled, wasEnabled) => {
  if (enabled && !wasEnabled) rememberRenderedGroupHeights()
  void nextTick(() => {
    updateVirtualBoxOrigin()
    messageVirtualizer.value.measure()
  })
})

watch(effectiveSessionId, () => {
  groupHeightEstimates.clear()
  virtualBoxOrigin = 0
  virtualBoxOriginReady = false
  void nextTick(() => {
    messageVirtualizer.value.measure()
    updateVirtualBoxOrigin()
  })
})

// ---- 滚动锚定补偿（WebKit 无原生 scroll anchoring）----
// 消息组解冻（content-visibility 估算高度→真实高度）与图片按需加载都会改变
// 视口上方内容的总高度；Chrome 有 scroll anchoring 自动补偿，WebKit 没有——
// 表现为滚动中内容"顿一下"。用 ResizeObserver 观察每个消息组，完全位于视口
// 上方的组高度变化时同帧补偿 scrollTop（RO 回调在 layout 后 paint 前，写
// scrollTop 不触发重排，视口内容保持视觉稳定）。
const groupHeights = new WeakMap<Element, number>()
let anchorRO: ResizeObserver | null = null
let suppressAnchorCompensation = false

// ---- 组位置分类（锚定补偿的零布局读数据源）----
// 历史教训（HUD 长帧归因实测）：cv 的手工 hidden/visible 管理在 WebKit 上是
// 负优化（"渲染状态保留"实现不达标，批量切换 = 批量全价 layout，743ms 帧）；
// 补偿回调里读 offsetTop 在布局脏时强制同步 layout（278ms）。故：cv 交还
// 浏览器 auto 自管；组的"是否在视口上方"用 IO 自带几何信息维护（回调 entry
// 的 boundingClientRect/rootBounds 是浏览器附送的，零强制布局读）。
// 分类有一帧异步延迟，边缘组偶发误差可接受。
let posIO: IntersectionObserver | null = null
const groupAbove = new WeakMap<Element, boolean>()

function observeAnchorGroups() {
  const sc = scrollContainer.value
  if (!sc) return
  if (!posIO) {
    posIO = new IntersectionObserver((entries) => {
      for (const e of entries) {
        const above =
          !e.isIntersecting &&
          e.boundingClientRect.bottom <= (e.rootBounds?.top ?? 0)
        groupAbove.set(e.target, above)
      }
    }, { root: sc, threshold: 0 })
  }
  // WeakMap 基线保留：重挂后首次回调 diff=0 不误补偿；断连期间的变化照常补偿。
  // 观察集与 cv 类解耦(回归审查 R5):末组已豁免 msg-group-cv,若按类选择器收集,
  // 末组在视口上方增高(图片异步加载等)时将失去锚定补偿——按结构属性收集全部组
  anchorRO?.disconnect()
  posIO.disconnect()
  for (const el of sc.querySelectorAll('[data-anchor-index]')) {
    anchorRO?.observe(el)
    posIO.observe(el)
  }
}

onMounted(() => {
  anchorRO = new ResizeObserver((entries) => {
    const sc = scrollContainer.value
    if (!sc) return
    const perfT0 = performance.now()
    let delta = 0
    const compensateAnchor = !followStreaming.value
    for (const entry of entries) {
      const el = entry.target as HTMLElement
      const newH = entry.borderBoxSize?.[0]?.blockSize ?? entry.contentRect.height
      const oldH = groupHeights.get(el)
      groupHeights.set(el, newH)
      if (oldH === undefined) continue // 首次观测：建立基线
      const diff = newH - oldH
      if (diff === 0) continue
      // 仅补偿视口上方的组（分类由 posIO 免费维护，本回调零布局属性读——
      // 读 offsetTop 会在布局脏时强制同步 layout，实测 278ms，已废弃该写法）
      if (
        compensateAnchor
        && !suppressAnchorCompensation
        && !el.hasAttribute('data-virtual-anchor')
        && groupAbove.get(el)
      ) {
        delta += diff
      }
    }
    if (delta !== 0) {
      const before = sc.scrollTop
      sc.scrollTop += delta
      // 校正 onScroll 基线:补偿位移不计入用户手势 delta(负补偿曾被误判为
      // "用户上滚"而静默关闭跟随);clamp 时以实际生效量为准
      const actual = sc.scrollTop - before
      lastScrollTop += actual
    }
    performance.measure('anchor-comp', { start: perfT0, duration: performance.now() - perfT0 })
  })
  observeAnchorGroups()
})
onUnmounted(() => {
  anchorRO?.disconnect()
  anchorRO = null
  posIO?.disconnect()
  posIO = null
})
watch(messageGroups, () => nextTick(() => {
  updateVirtualBoxOrigin()
  observeAnchorGroups()
}))

function userTextPreview(record: VisibleRecord): string {
  if (record.type !== 'user' || !record.message) return ''
  const content = record.message.content
  if (typeof content === 'string') return content.slice(0, 120)
  const texts: string[] = []
  for (const b of content) {
    if (b.type === 'text' && (b as any).text) {
      const raw = (b as any).text as string
      const clean = raw.replace(/<[^>]+>/g, '').trim()
      if (clean) texts.push(clean)
    }
  }
  return texts.join(' ').slice(0, 120)
}

const anchorItems = computed<AnchorItem[]>(() => {
  const items: AnchorItem[] = []
  for (let i = 0; i < messageGroups.value.length; i++) {
    const g = messageGroups.value[i]
    if (!g.user || g.user.type !== 'user') continue
    if (isSystemOnlyUser(g.user)) continue
    const text = userTextPreview(g.user)
    if (!text) continue
    items.push({ index: i, text })
  }
  return items
})

/** 解析私有标签,转为特殊渲染块 */
const TAG_RE = /<(system-reminder|ide_opened_file|ide_selection|task-notification|user-prompt-submit-hook|persisted-output|tool_use_error|command-name|command-args|command-message|local-command-caveat|local-command-stdout|loop-pause)[^>]*>([\s\S]*?)<\/\1>/g
const DISCARD_TAGS_RE = /<\/?(?:antml:thinking|antml:function_calls|antml:invoke|antml:parameter)[^>]*>/g
/** 冗余/仅供模型阅读的标签,解析后直接丢弃不渲染 */
const SILENT_TAGS = new Set(['command-message', 'local-command-caveat', 'loop-pause'])

function parsePrivateTags(text: string): ContentBlock[] {
  const results: ContentBlock[] = []
  let lastIndex = 0
  const cleaned = text.replace(DISCARD_TAGS_RE, '')

  for (const match of cleaned.matchAll(TAG_RE)) {
    const before = cleaned.slice(lastIndex, match.index).trim()
    if (before) results.push({ type: 'text', text: before })

    const [, tag, content] = match
    if (!SILENT_TAGS.has(tag)) {
      results.push({ type: tag, text: content.trim() } as any)
    }

    lastIndex = match.index! + match[0].length
  }

  const after = cleaned.slice(lastIndex).trim()
  if (after) results.push({ type: 'text', text: after })

  // command-name 紧邻 command-args 时合并(args 挂到 name 块),渲染收成一行
  const merged: ContentBlock[] = []
  for (let i = 0; i < results.length; i++) {
    const b = results[i] as any
    const next = results[i + 1] as any
    if (b.type === 'command-name' && next?.type === 'command-args') {
      merged.push({ ...b, args: next.text } as any)
      i++
    } else {
      merged.push(results[i])
    }
  }
  return merged
}

function contentBlocks(record: Extract<SessionRecord, { type: 'user' | 'assistant' }>): ContentBlock[] {
  if (!record.message) return []
  let blocks: ContentBlock[]
  if (record.type === 'user') {
    const content = record.message.content
    if (typeof content === 'string') {
      blocks = [{ type: 'text', text: content }]
    } else {
      blocks = content
    }
  } else {
    blocks = record.message.content
  }
  const expanded = blocks.flatMap(b => {
    if (b.type !== 'text') return [b]
    const text = (b as any).text as string
    // 系统自动文本：用户中断
    if (/^\[Request interrupted by user/.test(text)) {
      return [{ type: 'system-event', text: text.slice(1, -1) } as any]
    }
    // 系统自动文本：图片尺寸元数据
    if (/^\[Image: (?:original|source:)/.test(text)) {
      return [{ type: 'image-meta', text: text.slice(1, -1) } as any]
    }
    const skillMatch = text.match(/^Base directory for this skill:\s*(\S+)/)
    if (skillMatch) {
      const skillPath = skillMatch[1]
      const skillName = skillPath.split(/[/\\]/).pop() || skillPath
      return [{ type: 'skill_prompt', text: text, name: skillName } as any]
    }
    if (/<(?:system-reminder|ide_opened_file|ide_selection|task-notification|user-prompt-submit-hook|persisted-output|tool_use_error|command-name|command-args|command-message|local-command-caveat|local-command-stdout|loop-pause)/.test(text)) {
      return parsePrivateTags(text)
    }
    return [b]
  })
  // 将 tool_result 内嵌的图片提升到顶层独立渲染
  const lifted: ContentBlock[] = []
  for (const b of expanded) {
    lifted.push(b)
    if (b.type === 'tool_result' && Array.isArray((b as any).content)) {
      for (const sub of (b as any).content as ContentBlock[]) {
        if (sub.type === 'image') lifted.push(sub)
      }
    }
  }
  return lifted
}

/** 历史轮次元信息：多次 assistant API 调用统一求和，完成时刻取最后一条落账记录。 */
function groupResponseMetaOf(group: MsgGroup): AssistantResponseMeta | null {
  const assistants = group.responses.filter(
    (record): record is Extract<VisibleRecord, { type: 'assistant' }> => record.type === 'assistant',
  )
  if (assistants.length === 0) return null

  const summary = summarizeAssistantResponse(assistants.map(record => ({
    model: record.message?.model,
    usage: record.message?.usage,
  })))
  let doneTs: string | null = null
  for (const record of assistants) {
    // 合并块 timestamp 是首行≈开始，_lastTs 是末行≈完成。
    doneTs = (record as typeof record & { _lastTs?: string | null })._lastTs ?? record.timestamp ?? doneTs
  }
  return {
    ...summary,
    completedText: timeOfDay(doneTs),
    completedFull: fullTime(doneTs),
    tier: modelTierOf(summary.model),
  }
}

/** 历史区头尾共用同一份摘要，避免模板重渲染时逐组重复 Intl 与求和。 */
const groupResponseMetas = computed<(AssistantResponseMeta | null)[]>(() =>
  messageGroups.value.map(groupResponseMetaOf),
)

/** 流式区与历史区复用同一元信息结构；usage 随已完成的 API message 累加。 */
const streamingResponseMeta = computed<AssistantResponseMeta | null>(() => {
  const turns = stream.value.streamingTurns
  if (turns.length === 0) return null
  const summary = summarizeAssistantResponse(turns)
  const completedAt = streamResponseCompletedAt.value
  return {
    ...summary,
    completedText: timeOfDay(completedAt),
    completedFull: fullTime(completedAt),
    tier: modelTierOf(summary.model),
  }
})

// ---- 发送时间标注 ----

/** HH:mm(时制跟随 UI 语言惯例);无效/缺失时间戳返回空串,调用侧 v-if 吞掉 */
function timeOfDay(ts: string | null | undefined): string {
  if (!ts) return ''
  const d = new Date(ts)
  if (Number.isNaN(d.getTime())) return ''
  return d.toLocaleTimeString(locale.value, { hour: '2-digit', minute: '2-digit' })
}

/** hover 用完整日期时间串(带秒) */
function fullTime(ts: string | null | undefined): string {
  if (!ts) return ''
  const d = new Date(ts)
  return Number.isNaN(d.getTime()) ? '' : d.toLocaleString(locale.value)
}

/** 常驻显示用完整时间:年月日时分,不带秒(如 2026/7/6 14:32);兼容 ms 时间戳 */
function fullStamp(ts: string | number | null | undefined): string {
  if (ts == null || ts === '') return ''
  const d = new Date(ts)
  if (Number.isNaN(d.getTime())) return ''
  return d.toLocaleString(locale.value, { year: 'numeric', month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

/** 组代表时间戳:用户消息优先,兜底首条带时间的回复(首组可能无 user) */
function groupTs(g: MsgGroup): string | null {
  const ut = (g.user as { timestamp?: string | null } | null)?.timestamp
  if (ut) return ut
  for (const r of g.responses) {
    const rt = (r as { timestamp?: string | null }).timestamp
    if (rt) return rt
  }
  return null
}

/**
 * 用户消息发送时间文案(完整年月日时分,常驻显示),与 messageGroups 同下标。
 * 预计算而非模板内调用:本组件任何状态变化(如输入框打字)都整模板 re-render,
 * 几百组逐个跑 Intl 格式化会吃掉帧预算;computed 后 re-render 只剩数组下标访问。
 */
const groupTimeLabels = computed<string[]>(() =>
  messageGroups.value.map(g =>
    fullStamp((g.user as { timestamp?: string | null } | null)?.timestamp),
  ),
)

/** 跨天日期分隔文案,与 messageGroups 同下标;同天/无时间戳为 null。首组也标(会话起始日) */
const dayDividers = computed<(string | null)[]>(() => {
  const out: (string | null)[] = []
  let prevDay: string | null = null
  const thisYear = new Date().getFullYear()
  for (const g of messageGroups.value) {
    const ts = groupTs(g)
    const d = ts ? new Date(ts) : null
    if (!d || Number.isNaN(d.getTime())) { out.push(null); continue }
    const key = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`
    if (key === prevDay) { out.push(null); continue }
    prevDay = key
    const opts: Intl.DateTimeFormatOptions = { month: 'long', day: 'numeric', weekday: 'short' }
    if (d.getFullYear() !== thisYear) opts.year = 'numeric'
    out.push(d.toLocaleDateString(locale.value, opts))
  }
  return out
})

const USER_CONTENT_TYPES = new Set(['text', 'image', 'document'])

function isSystemOnlyUser(record: Extract<SessionRecord, { type: 'user' }>): boolean {
  const blocks = contentBlocks(record as any)
  return blocks.length > 0 && blocks.every(b => !USER_CONTENT_TYPES.has(b.type))
}

/** 用户卡是否有可见内容:全空白时不渲染空卡壳(纯图片/文档消息不受影响) */
function userHasVisibleContent(record: Extract<SessionRecord, { type: 'user' }>): boolean {
  const blocks = contentBlocks(record as any)
  return blocks.some(b =>
    b.type === 'image' || b.type === 'document' || (b.type === 'text' && !!(b as any).text?.trim()),
  )
}

/**
 * /model 切换事件的横线文案:以 stdout 记录「Set model to X」为事实源——
 * 它是 CLI 确认执行成功才落的输出,有参/无参/取消三种场景天然正确
 * (取消时无 stdout,不留假横线)。文案不匹配时返回 null,安全降级为普通 stdout 行。
 */
function modelSwitchName(record: Extract<SessionRecord, { type: 'user' }>): string | null {
  const blocks = contentBlocks(record as any)
  for (const b of blocks) {
    if (b.type === 'local-command-stdout') {
      const m = (((b as any).text as string) ?? '').match(/^Set model to\s+(.+)$/)
      if (m) {
        const raw = m[1].trim()
        return inferModel(raw)?.label ?? raw
      }
    }
  }
  return null
}

/** /model 命令记录本身(胶囊+参数):静默不渲染,事件由 stdout 横线承载 */
function isModelCommandRecord(record: Extract<SessionRecord, { type: 'user' }>): boolean {
  const blocks = contentBlocks(record as any)
  const cmd = blocks.find(b => b.type === 'command-name')
  return !!cmd && (((cmd as any).text as string) ?? '').trim() === '/model'
}

// --- 斜杠命令处理 ---

function onSlashSelect(cmd: SlashCommand) {
  if (cmd.hasArg) {
    const insert = `/${cmd.name} `
    inputText.value = insert
    nextTick(() => {
      const el = textareaRef.value
      if (!el) return
      el.focus()
      autoResize()
      const pos = insert.length
      el.setSelectionRange(pos, pos)
      cursorPos.value = pos
    })
  } else {
    inputText.value = `/${cmd.name}`
    cursorPos.value = 0
    nextTick(() => handleSend())
  }
}

function onSlashClose() {
  // 用户继续编辑会自然退出触发态;关闭事件本身只清提示
  slashError.value = null
}

function clearCurrentPaneView() {
  const sid = effectiveSessionId.value
  if (sid) clearStreamingTurns(sid)
  hideHistory.value = true
  showHelpCard.value = false
}

function handleNewSession() {
  // /new:工作台列绑定固定会话,引导用左列入口;档案馆回到空选择
  if (props.mode === 'workbench') {
    slashError.value = t('session.slashNewInWorkbench')
    return
  }
  selectSession(null)
}

function handleChangeDirectory(arg: string) {
  if (props.mode === 'workbench') {
    slashError.value = t('session.slashOpenInWorkbench')
    return
  }
  // 匹配 display_path:分隔符归一 + Windows 形态盘符大小写不敏感,
  // 免得用户必须手输与磁盘完全一致的反斜杠形态
  const target = projects.value.find(p => samePath(p.display_path, arg))
  if (!target) {
    slashError.value = t('session.slashPathNotFound')
    return
  }
  if (target.sessions.length === 0) {
    slashError.value = t('session.slashNoSessions')
    return
  }
  selectSession(target.sessions[0].id)
}

function handleNativeCommand(cmd: SlashCommand) {
  switch (cmd.name) {
    case 'help':
      showHelpCard.value = true
      resumeFollow()
      break
    case 'clear':
      clearCurrentPaneView()
      break
    case 'new':
      handleNewSession()
      break
    case 'cd': {
      const arg = inputText.value.trim().replace(/^\/cd\s+/, '')
      handleChangeDirectory(arg)
      break
    }
  }
}

function handleModelSwitch(modelName: string) {
  setModel(modelName)
}

async function handleSend() {
  const text = inputText.value.trim()
  if ((!text && !imageInput.images.value.length) || !currentSession.value) return
  const cs = currentSession.value
  if (!cs.summary.cwd) return

  const parsed = parseCommand(text, workshopAssets.value?.skills, workshopAssets.value?.commands)

  // /model invalid 等:不清空输入,显示提示
  if (parsed.kind === 'invalid') {
    slashError.value = parsed.reason
    return
  }
  slashError.value = null

  // native 命令(/help /clear /new /cd):前端处理,不调 CLI
  if (parsed.kind === 'native') {
    handleNativeCommand(parsed.cmd)
    inputText.value = ''
    if (textareaRef.value) textareaRef.value.style.height = 'auto'
    return
  }

  // terminal 命令(/login /logout 等):GUI 内无法运行,在终端打开 claude 执行
  if (parsed.kind === 'terminal') {
    const cmdName = parsed.cmd.name
    inputText.value = ''
    if (textareaRef.value) textareaRef.value.style.height = 'auto'
    try {
      await invoke('run_slash_in_terminal', { cwd: cs.summary.cwd, command: cmdName })
      slashNotice.value = t('session.slashOpenedInTerminal', { cmd: `/${cmdName}` })
    } catch (e) {
      const msg = String(e)
      slashError.value = msg.startsWith('AUTOMATION_DENIED')
        ? t('session.slashTerminalDenied')
        : msg
    }
    return
  }

  // pass 命令(/model /chrome):持久化设置,不发消息
  if (parsed.kind === 'pass' && parsed.cmd.name === 'model') {
    handleModelSwitch(parsed.arg)
    inputText.value = ''
    if (textareaRef.value) textareaRef.value.style.height = 'auto'
    return
  }

  // /chrome 本地拦截:切会话级开关,下一条消息经 needs_restart 重启进程生效
  if (parsed.kind === 'pass' && parsed.cmd.name === 'chrome') {
    const next = parsed.arg === '' ? !settings.value.chrome : parsed.arg === 'on'
    onChromeChange(next)
    inputText.value = ''
    if (textareaRef.value) textareaRef.value.style.height = 'auto'
    return
  }

  // unknown / 普通文本:走原始流式发送
  inputText.value = ''
  if (textareaRef.value) textareaRef.value.style.height = 'auto'
  const sendFollowToken = resumeFollow()
  featureBannerShown.value = false
  // 发送前即时落账:上一轮流式 turns 还在时,sendMessage 会清 streamingTurns
  // 但 records 尚未 reload——内容从两源同时消失。先应用暂存/重新 reload 收进历史区
  if (stream.value.streamingTurns.length > 0) {
    if (deferredRecords?.sid === cs.summary.id) {
      devObserveSwap('send-swap')
      records.value = deferredRecords.recs
      deferredRecords = null
    } else {
      try {
        const fresh = await invoke<SessionRecord[]>('get_session_records', {
          projectId: cs.projectId,
          sessionId: cs.summary.id,
        })
        if (effectiveSessionId.value === cs.summary.id && fresh.length >= records.value.length) {
          devObserveSwap('send-swap')
          records.value = fresh
        }
      } catch { /* next reload will pick it up */ }
    }
  }
  const advisor = settings.value.advisor
  const images = imageInput.images.value.length ? await imageInput.toImageBlocks() : undefined
  imageInput.clearImages()
  // 发送前重读渠道清单(活文件),runConfig 随之解析出最新的渠道默认
  await refreshChannels()
  const rc = runConfig.value
  const opts = {
    model: rc.launch.model,
    effort: rc.launch.effort ?? null,
    fastMode: rc.launch.fastMode,
    channel: rc.channelId,
    advisor,
    chrome: settings.value.chrome,
    forkSource: forkSourceOf(cs.summary.id) ?? undefined,
    extraArgs: settings.value.extraArgs || undefined,
    images,
    permissionMode: rc.launch.permissionMode ?? undefined,
  }
  if (externalRunning.value || ownProcessBusy.value) {
    stream.value.pendingQueue.push({ message: text, opts })
    return
  }
  await sendMessage(cs.summary.id, cs.summary.cwd, text, opts)
  scrollToBottom(sendFollowToken)
}

function onInputKeydown(e: KeyboardEvent) {
  if (shouldSubmitComposer(e)) {
    e.preventDefault()
    handleSend()
  }
}

const followStreaming = ref(true)
let scrollFollowState = createScrollFollowState()
let lastScrollTop = 0
let stickyUpdateFrame = 0
let scrollBottomRequestId = 0
let scrollBottomQueuedEpoch: number | null = null

interface ProgrammaticScrollWrite {
  target: number
  at: number
  source: 'follow' | 'virtualizer' | 'anchor'
}

let programmaticScrollWrite: ProgrammaticScrollWrite | null = null

function cancelPendingFollowWrites(): void {
  scrollBottomRequestId++
  scrollBottomQueuedEpoch = null
  if (followScrollTimer !== null) window.clearTimeout(followScrollTimer)
  followScrollTimer = null
  followScrollScheduledAt = Number.POSITIVE_INFINITY
  followScrollToken = null
  followScrollSessionId = null
  followScrollElement = null
  followScrollTop = 0
}

function detachFollow(): void {
  scrollFollowState = transitionScrollFollow(scrollFollowState, 'detach')
  followStreaming.value = false
  cancelPendingFollowWrites()
}

function resumeFollow(allowLayoutReset = false): ScrollFollowToken | null {
  scrollFollowState = transitionScrollFollow(scrollFollowState, 'resume')
  followStreaming.value = true
  wheelUpIntentAt = Number.NEGATIVE_INFINITY
  cancelPendingFollowWrites()
  const token = captureScrollFollowToken(scrollFollowState)
  scrollToBottom(token, allowLayoutReset)
  return token
}

function preserveFollowAfterStreamFinished(): void {
  scrollFollowState = transitionScrollFollow(scrollFollowState, 'stream-finished')
  followStreaming.value = scrollFollowState.mode === 'following'
}

function isScrollGenerationCurrent(epoch: number, sessionId: string | null): boolean {
  return scrollFollowState.epoch === epoch
    && (effectiveSessionId.value ?? null) === sessionId
}

function rememberProgrammaticScroll(
  element: HTMLElement,
  source: ProgrammaticScrollWrite['source'],
): void {
  programmaticScrollWrite = {
    target: element.scrollTop,
    at: performance.now(),
    source,
  }
  lastScrollTop = element.scrollTop
}

function consumeProgrammaticScrollEvent(element: HTMLElement): boolean {
  const write = programmaticScrollWrite
  if (!write) return false
  programmaticScrollWrite = null
  return performance.now() - write.at < 160
    && Math.abs(element.scrollTop - write.target) < 1.5
}

provide(TOOL_FOLD_INTERACTION, detachFollow)

/** 最近一次滚轮上滚意图时刻:窗口期内 contentRO 暂停贴底。
 *  没有它,用户上滚与打字机/图片增高同帧竞争时,RO 在 layout 后把位置贴回、
 *  onScroll 事后读到的已是贴底值——脱离手势被吞,表现为"滚不上去被拽回" */
let wheelUpIntentAt = Number.NEGATIVE_INFINITY
let wheelDownIntentAt = Number.NEGATIVE_INFINITY

function onScrollWheel(e: WheelEvent) {
  const now = performance.now()
  if (e.deltaY > 0) {
    wheelDownIntentAt = now
    const element = scrollContainer.value
    if (
      !followStreaming.value
      && element
      && element.scrollHeight - element.scrollTop - element.clientHeight < 2
    ) resumeFollow()
    return
  }
  if (e.deltaY >= 0) return
  // 用户手势优先：第一帧负向 wheel 就同步脱离，不能等 scroll/rAF 后判定。
  wheelUpIntentAt = now
  detachFollow()
}

function onScroll() {
  const element = scrollContainer.value
  if (!element) return
  const programmatic = consumeProgrammaticScrollEvent(element)
  const delta = element.scrollTop - lastScrollTop
  lastScrollTop = element.scrollTop
  const distFromBottom = element.scrollHeight - element.scrollTop - element.clientHeight

  // rAF 之前先处理用户向上意图，覆盖滚动条拖拽、键盘滚动等非 wheel 路径。
  if (!programmatic && delta < -0.5 && distFromBottom > 2) detachFollow()
  if (
    !programmatic
    && !followStreaming.value
    && delta > 0
    && distFromBottom < 2
    && performance.now() - wheelDownIntentAt < 350
  ) {
    resumeFollow()
  }

  scheduleStickyUpdate()

}

// --- 用户提问吸顶层 ---
// 虚拟项本身带 transform，内部原生 sticky 无法跨项工作。原消息保留占位，
// 悬浮层只复制当前提问；选择、碰撞和推离全部使用真实卡片的 client rect。
const STICKY_CARD_GAP = 16
const pendingStickyRef = ref<HTMLElement>()
const stickyOverlayElement = ref<HTMLElement | null>(null)
const stickySurfaceElement = ref<HTMLElement | null>(null)
const stickyGroupIndex = ref(-1)
const stickyPending = ref(false)
const stickyPushOffset = ref(0)
let stickySurfaceResizeObserver: ResizeObserver | null = null

function promptElement(owner: HTMLElement | null | undefined): HTMLElement | null {
  return owner?.querySelector<HTMLElement>('.conversation-user-message') ?? null
}

function stickySurfaceCard(): HTMLElement | null {
  return promptElement(stickySurfaceElement.value)
}

function stickyRestingClientTop(): number | null {
  const card = stickySurfaceCard()
  if (card) return card.getBoundingClientRect().top - stickyPushOffset.value
  return stickyOverlayElement.value?.getBoundingClientRect().top ?? null
}

function setStickySelection(index: number, pending: boolean) {
  if (stickyGroupIndex.value === index && stickyPending.value === pending) return
  stickyPushOffset.value = 0
  stickyGroupIndex.value = index
  stickyPending.value = pending
}

/** 吸顶合格判定：只接管真实、可见的用户提问。 */
function groupQualifies(group: { user: any; responses: any[] } | null | undefined): boolean {
  if (!group?.user || group.user.type !== 'user') return false
  if (modelSwitchName(group.user) || isModelCommandRecord(group.user) || isSystemOnlyUser(group.user)) return false
  return userHasVisibleContent(group.user)
}

function findQualifiedGroup(from: number, direction: 1 | -1): number {
  for (let index = from; index >= 0 && index < messageGroups.value.length; index += direction) {
    if (groupQualifies(messageGroups.value[index])) return index
  }
  return -1
}

function updateStickyGroup() {
  if (!stickyUserPromptEnabled.value) {
    setStickySelection(-1, false)
    return
  }
  const restingTop = stickyRestingClientTop()
  if (restingTop === null) return

  const pendingCard = promptElement(pendingStickyRef.value)
  if (pendingCard && pendingCard.getBoundingClientRect().top <= restingTop + 0.5) {
    setStickySelection(-1, true)
    return
  }

  let latestPast = -1
  let firstFuture = Number.POSITIVE_INFINITY
  for (const owner of scrollContentEl.value?.querySelectorAll<HTMLElement>('[data-anchor-index]') ?? []) {
    const index = Number(owner.dataset.anchorIndex)
    const card = promptElement(owner)
    if (!Number.isInteger(index) || !card || !groupQualifies(messageGroups.value[index])) continue
    if (card.getBoundingClientRect().top <= restingTop + 0.5) latestPast = Math.max(latestPast, index)
    else firstFuture = Math.min(firstFuture, index)
  }

  if (latestPast >= 0) {
    setStickySelection(latestPast, false)
  } else if (Number.isFinite(firstFuture)) {
    setStickySelection(findQualifiedGroup(firstFuture - 1, -1), false)
  }
}

const stickyDisplay = computed(() => {
  if (!stickyUserPromptEnabled.value) return null
  const index = findQualifiedGroup(stickyGroupIndex.value, -1)
  return index >= 0 ? { group: messageGroups.value[index], index } : null
})
const stickyGroup = computed(() => stickyDisplay.value?.group ?? null)
const stickyTimeLabel = computed(() => {
  const display = stickyDisplay.value
  return display ? (groupTimeLabels.value[display.index] ?? '') : ''
})

/** 流式 pending 用户消息的发送时间标注(未落账,用前端发送时刻) */
const pendingTimeLabel = computed(() => fullStamp(stream.value.pendingSentAt))

const stickyPrevIndex = computed(() => stickyPending.value
  ? findQualifiedGroup(messageGroups.value.length - 1, -1)
  : stickyDisplay.value ? findQualifiedGroup(stickyDisplay.value.index - 1, -1) : -1)
const stickyNextIndex = computed(() => stickyPending.value
  ? -1
  : stickyDisplay.value ? findQualifiedGroup(stickyDisplay.value.index + 1, 1) : -1)

function groupPromptCard(index: number): HTMLElement | null {
  const owner = scrollContentEl.value?.querySelector<HTMLElement>(`[data-anchor-index="${index}"]`)
  return promptElement(owner)
}

function updateStickyPushOffset() {
  const currentCard = stickySurfaceCard()
  if ((!stickyDisplay.value && !stickyPending.value) || !currentCard) {
    stickyPushOffset.value = 0
    return
  }
  const nextCard = stickyPending.value
    ? null
    : stickyNextIndex.value >= 0
      ? groupPromptCard(stickyNextIndex.value)
      : promptElement(pendingStickyRef.value)
  if (!nextCard) {
    stickyPushOffset.value = 0
    return
  }

  const restingBottom = currentCard.getBoundingClientRect().bottom - stickyPushOffset.value
  stickyPushOffset.value = Math.min(
    0,
    nextCard.getBoundingClientRect().top - STICKY_CARD_GAP - restingBottom,
  )
}

function scheduleStickyUpdate() {
  if (stickyUpdateFrame) return
  stickyUpdateFrame = window.requestAnimationFrame(() => {
    stickyUpdateFrame = 0
    updateStickyGroup()
    updateStickyPushOffset()
  })
}

watch(stickySurfaceElement, element => {
  stickySurfaceResizeObserver?.disconnect()
  stickySurfaceResizeObserver = null
  if (!element) return
  stickySurfaceResizeObserver = new ResizeObserver(scheduleStickyUpdate)
  stickySurfaceResizeObserver.observe(element)
  scheduleStickyUpdate()
})

watch([
  renderGroups,
  stickyUserPromptEnabled,
  () => stream.value.pendingUserMessage,
  () => stream.value.pendingImages?.length ?? 0,
  pendingLandedUuid,
], () => void nextTick(scheduleStickyUpdate))

function elementScrollTop(element: HTMLElement, sc: HTMLElement): number {
  return element.getBoundingClientRect().top - sc.getBoundingClientRect().top + sc.scrollTop
}

function scrollToGroupIndex(index: number) {
  if (index < 0) return
  detachFollow()
  const sc = scrollContainer.value
  if (!shouldVirtualize.value) {
    const element = scrollContentEl.value?.querySelector<HTMLElement>(`[data-anchor-index="${index}"]`)
    if (!sc || !element) return
    sc.scrollTop = elementScrollTop(element, sc)
    return
  }
  if (index < renderGroups.value.length) messageVirtualizer.value.scrollToIndex(index, { align: 'start' })
}

function jumpToStickyPrompt() {
  if (!stickyPending.value) {
    scrollToGroupIndex(stickyDisplay.value?.index ?? -1)
    return
  }
  detachFollow()
  const sc = scrollContainer.value
  if (!sc || !pendingStickyRef.value) return
  sc.scrollTop = elementScrollTop(pendingStickyRef.value, sc)
}

function jumpStickyPrev() { scrollToGroupIndex(stickyPrevIndex.value) }
function jumpStickyNext() { scrollToGroupIndex(stickyNextIndex.value) }

onUnmounted(() => {
  stickySurfaceResizeObserver?.disconnect()
  stickySurfaceResizeObserver = null
  if (stickyUpdateFrame) window.cancelAnimationFrame(stickyUpdateFrame)
  stickyUpdateFrame = 0
})

/** SessionAnchorNav 点击时的虚拟化前置:让目标组先进虚拟窗口渲染出来,再由 SessionAnchorNav 的 rAF 精调 offsetTop */
function onAnchorScrollToIndex(index: number) {
  detachFollow()
  if (shouldVirtualize.value && index < renderGroups.value.length) {
    messageVirtualizer.value.scrollToIndex(index, { align: 'start' })
  }
}

function locateToolUse(toolUseId: string) {
  // 工具摘要锚点始终存在；grouped 模式下先按稳定 ID 展开所属组和单项，再定位。
  // 1. 反查 tool_use 所在的组 gi,虚拟窗口内 scrollToIndex 到大致偏移
  // 2. rAF×2 后精调 scrollIntoView 到 tool_use 元素(第一帧让 virtualizer 布局落定,第二帧让 measureElement 校准真高)
  // 兼顾旧行为:目标已在 DOM(如末组或已渲染的窗口内)则同帧直接 scrollIntoView,避免多余延迟
  detachFollow()
  const scrollEpoch = scrollFollowState.epoch
  const sessionId = effectiveSessionId.value ?? null
  const valid = () => isScrollGenerationCurrent(scrollEpoch, sessionId)
  toolFoldState.requestReveal(toolUseId)
  const gi = messageGroups.value.findIndex(g =>
    g.responses.some(r => {
      const content = (r as any).message?.content
      return Array.isArray(content) && content.some((b: any) => b?.type === 'tool_use' && b?.id === toolUseId)
    }),
  )
  const escaped = CSS.escape(toolUseId)
  const highlight = (el: HTMLElement) => {
    el.classList.add('ring-2', 'ring-primary/60')
    setTimeout(() => el.classList.remove('ring-2', 'ring-primary/60'), 1500)
  }
  const tryImmediate = scrollContainer.value?.querySelector<HTMLElement>(`[data-tool-use-id="${escaped}"]`)
  if (tryImmediate) {
    void nextTick(() => {
      if (!valid()) {
        toolFoldState.clearRevealRequest(toolUseId)
        return
      }
      requestAnimationFrame(() => {
        if (!valid()) {
          toolFoldState.clearRevealRequest(toolUseId)
          return
        }
        const el = scrollContainer.value?.querySelector<HTMLElement>(`[data-tool-use-id="${escaped}"]`) ?? tryImmediate
        el.scrollIntoView({ block: 'center', behavior: 'smooth' })
        highlight(el)
        toolFoldState.clearRevealRequest(toolUseId)
      })
    })
    return
  }
  if (gi < 0) {
    toolFoldState.clearRevealRequest(toolUseId)
    return
  }
  // Step 1: 目标不在 DOM → 用 virtualizer 滚到目标组附近
  if (gi < renderGroups.value.length) {
    messageVirtualizer.value.scrollToIndex(gi, { align: 'center' })
  }
  // Step 2: rAF×2 后精调
  requestAnimationFrame(() => {
    if (!valid()) {
      toolFoldState.clearRevealRequest(toolUseId)
      return
    }
    requestAnimationFrame(() => {
      if (!valid()) {
        toolFoldState.clearRevealRequest(toolUseId)
        return
      }
      const el = scrollContainer.value?.querySelector<HTMLElement>(`[data-tool-use-id="${escaped}"]`)
      if (!el) {
        toolFoldState.clearRevealRequest(toolUseId)
        return
      }
      el.scrollIntoView({ block: 'center', behavior: 'smooth' })
      highlight(el)
      toolFoldState.clearRevealRequest(toolUseId)
    })
  })
}

watch(toolDisplayMode, async () => {
  const sc = scrollContainer.value
  if (!sc) return
  const wasFollowing = followStreaming.value
  const followToken = captureScrollFollowToken(scrollFollowState)
  const scrollEpoch = scrollFollowState.epoch
  const sessionId = effectiveSessionId.value ?? null
  const valid = () => isScrollGenerationCurrent(scrollEpoch, sessionId)
  const scTop = sc.getBoundingClientRect().top
  const candidates = [...sc.querySelectorAll<HTMLElement>('[data-anchor-index]')]
  const anchor = candidates.find(el => el.getBoundingClientRect().bottom > scTop) ?? null
  const anchorIndex = anchor?.dataset.anchorIndex ?? null
  const anchorOffset = anchor ? anchor.getBoundingClientRect().top - scTop : 0
  const finishSwap = () => {
    suppressAnchorCompensation = false
    observeAnchorGroups()
  }

  suppressAnchorCompensation = true
  await nextTick()
  if (!valid()) {
    finishSwap()
    return
  }
  messageVirtualizer.value.measure()
  await nextTick()
  if (!valid()) {
    finishSwap()
    return
  }
  requestAnimationFrame(() => {
    if (!valid()) {
      finishSwap()
      return
    }
    requestAnimationFrame(() => {
      if (!valid()) {
        finishSwap()
        return
      }
      if (wasFollowing) {
        scrollToBottom(followToken)
      } else if (anchorIndex !== null) {
        const nextAnchor = scrollContainer.value?.querySelector<HTMLElement>(`[data-anchor-index="${CSS.escape(anchorIndex)}"]`)
        if (nextAnchor && scrollContainer.value) {
          const nextTop = nextAnchor.getBoundingClientRect().top - scrollContainer.value.getBoundingClientRect().top
          scrollContainer.value.scrollTop += nextTop - anchorOffset
          rememberProgrammaticScroll(scrollContainer.value, 'anchor')
        }
      }
      finishSwap()
    })
  })
})

function scrollToBottom(
  token: ScrollFollowToken | null = captureScrollFollowToken(scrollFollowState),
  allowLayoutReset = false,
) {
  if (!token || !canApplyScrollFollowToken(scrollFollowState, token)) return
  if (scrollBottomQueuedEpoch === token.epoch) return
  const sessionId = effectiveSessionId.value ?? null
  const requestId = ++scrollBottomRequestId
  scrollBottomQueuedEpoch = token.epoch
  const scheduledElement = scrollContainer.value
  const scheduledScrollTop = scheduledElement?.scrollTop ?? 0
  const valid = () => requestId === scrollBottomRequestId
    && isScrollGenerationCurrent(token.epoch, sessionId)
    && canApplyScrollFollowToken(scrollFollowState, token)

  const userMovedUp = (element: HTMLElement) => !allowLayoutReset
    && scheduledElement === element
    && element.scrollTop < scheduledScrollTop - 0.5
    && element.scrollHeight - element.scrollTop - element.clientHeight > 2

  const applyScroll = (element: HTMLElement) => {
    if (!valid()) return
    if (userMovedUp(element)) {
      wheelUpIntentAt = performance.now()
      detachFollow()
      return
    }
    const target = Math.max(0, element.scrollHeight - element.clientHeight)
    if (target - element.scrollTop > 0.5) {
      element.scrollTop = target
      rememberProgrammaticScroll(element, 'follow')
    } else {
      lastScrollTop = element.scrollTop
    }
  }

  void nextTick(() => {
    if (!valid()) return
    requestAnimationFrame(() => {
      if (!valid()) return
      scrollBottomQueuedEpoch = null
      const element = scrollContainer.value
      if (!element) return
      if (scheduledElement && element !== scheduledElement) return
      applyScroll(element)
      // 内容刚挂载时 scrollHeight 可能还是 0，第二帧再测一次；同样复核 ticket。
      if (element.scrollHeight <= element.clientHeight) {
        requestAnimationFrame(() => {
          if (valid()) applyScroll(element)
        })
      }
    })
  })
}

watch(() => stream.value.pendingUserMessage, (val) => {
  if (val) scrollToBottom()
})

// ---- 布局层滚动跟随(水平触发) ----
// 跟随不再挂数据事件(watch streamingTick/records 已删),改挂"内容高度变化"本身:
// 打字机、晚到子 Agent turn、records 落账替换、图片加载、cv 组解冻、横幅出现,
// 任何增高源统一在此贴底——新增数据链路无需再记得挂滚动。
// RO 回调只标记贴底请求；真正的几何读写按列刷新档位合并，避免七列同时写 scrollTop。
let contentRO: ResizeObserver | null = null
let followScrollTimer: number | null = null
let followScrollScheduledAt = Number.POSITIVE_INFINITY
let lastFollowScrollAt = 0
let followScrollToken: ScrollFollowToken | null = null
let followScrollSessionId: string | null = null
let followScrollElement: HTMLElement | null = null
let followScrollTop = 0

function applyFollowScroll() {
  const token = followScrollToken
  const sessionId = followScrollSessionId
  const scheduledElement = followScrollElement
  const scheduledScrollTop = followScrollTop
  followScrollTimer = null
  followScrollScheduledAt = Number.POSITIVE_INFINITY
  followScrollToken = null
  followScrollSessionId = null
  followScrollElement = null
  followScrollTop = 0
  if (!token || !canApplyScrollFollowToken(scrollFollowState, token)) return
  if (!isScrollGenerationCurrent(token.epoch, sessionId)) return
  const sc = scrollContainer.value
  if (!sc) return
  if (scheduledElement && sc !== scheduledElement) return
  if (
    scheduledElement === sc
    && sc.scrollTop < scheduledScrollTop - 0.5
    && sc.scrollHeight - sc.scrollTop - sc.clientHeight > 2
  ) {
    wheelUpIntentAt = performance.now()
    detachFollow()
    return
  }
  const target = Math.max(0, sc.scrollHeight - sc.clientHeight)
  // 留出亚像素容差，防 WebKit 分数布局下对相同位置反复写 scrollTop。
  if (target - sc.scrollTop > 0.5) {
    sc.scrollTop = target
    rememberProgrammaticScroll(sc, 'follow')
  } else {
    lastScrollTop = sc.scrollTop
  }
  lastFollowScrollAt = performance.now()
}

function scheduleFollowScroll() {
  const token = captureScrollFollowToken(scrollFollowState)
  if (!token) return
  const sid = effectiveSessionId.value
  const interval = sid ? sessionRenderCadence(sid) : 66
  const now = performance.now()
  const delay = Math.max(0, lastFollowScrollAt + interval - now)
  const nextAt = now + delay
  if (followScrollTimer !== null && followScrollScheduledAt <= nextAt + 1) return
  if (followScrollTimer !== null) window.clearTimeout(followScrollTimer)
  followScrollScheduledAt = nextAt
  followScrollToken = token
  followScrollSessionId = sid ?? null
  followScrollElement = scrollContainer.value ?? null
  followScrollTop = followScrollElement?.scrollTop ?? 0
  followScrollTimer = window.setTimeout(applyFollowScroll, delay)
}

watch(scrollContentEl, (el) => {
  contentRO?.disconnect()
  if (!el) return
  if (!contentRO) {
    contentRO = new ResizeObserver(() => scheduleFollowScroll())
  }
  contentRO.observe(el)
}, { immediate: true })
onUnmounted(() => {
  contentRO?.disconnect()
  contentRO = null
  cancelPendingFollowWrites()
})

// ====== 流式结束 → 延迟清理（零 DOM 重建方案）======
// 核心思路：streaming→false 后 records 和 streamingTurns 都不动——DOM 零变化 = 零跳动。
// records 后台预取暂存，在下一个天然安全时刻（sendMessage / 会话切换 / force reload）
// 一并应用 + 清理 turns。shiki 上色虽有高度变化，但 contentRO 持续贴底足够覆盖。
let deferredRecords: { sid: string; recs: SessionRecord[] } | null = null

function rememberRenderedGroupHeights(lastOnly = false): void {
  const sc = scrollContainer.value
  if (!sc) return
  const groups = [...sc.querySelectorAll<HTMLElement>('[data-group-key]')]
  const targets = lastOnly ? groups.slice(-1) : groups
  for (const element of targets) {
    const key = element.dataset.groupKey
    if (!key) continue
    const height = element.offsetHeight
    if (height > 0) groupHeightEstimates.set(key, height)
  }
}

/**
 * 换树防坍缩：新组入列时，原直铺末组会迁入虚拟器。必须在 records 赋值前
 * 同步保存旧节点真高，让 estimateSize 在 Vue patch 的第一帧就得到正确尺寸；
 * 等 nextTick 再修旧 DOM 已经太晚，WebKit 会先按 200px sizer clamp scrollTop。
 */
function pinLastGroupBeforeSwap(): void {
  rememberRenderedGroupHeights(true)
}

// 消息来源不只 records：pending/队列/自发轮也会让旧末组迁入虚拟区。
// 同步观察历史 key 增长，在 Vue patch 前统一捕获当前直铺末组的真实高度。
watch(renderGroupKeys, (keys, previous) => {
  if (keys.length > previous.length) pinLastGroupBeforeSwap()
}, { flush: 'sync' })

// FR-006 开发期换树位移观测:前后 scrollHeight diff >1px 落档(生产构建 tree-shake)
function devObserveSwap(label: string) {
  if (!import.meta.env.DEV) return
  const before = scrollContainer.value?.scrollHeight ?? 0
  void nextTick(() => {
    import('@/lib/stream-markdown/devConsistencyCheck').then(({ devCheckSwapHeight }) =>
      devCheckSwapHeight(label, before, scrollContainer.value?.scrollHeight ?? 0))
  })
}

/**
 * settle 在途的会话:期间禁止队列消费。回合结束有两个消费沿(settle watcher 尾部
 * 与 ownProcessBusy 翻空闲),后者会抢在换树前发出队列消息——sendMessage 清场把
 * 尚未落入历史区的上一轮 turns 扔掉,呈现为"队列消息发出后上一轮回复部分消失"。
 * 门闸期间的队列消息由 settle 完成后的尾部 maybeConsumeQueue 补发,不滞留。
 */
const settlingSessions = new Set<string>()

watch(() => stream.value.streaming, async (val, oldVal) => {
  if (!val && oldVal) {
    const cs = currentSession.value
    if (!cs) return
    const sid = cs.summary.id
    if (!interactive.value) return
    if (import.meta.env.DEV) console.log(`%c ========== [detail] streaming→false (deferred) sid=${sid.slice(0, 8)} t=${performance.now().toFixed(0)} ==========`, 'color:#22c55e;font-weight:bold')
    settlingSessions.add(sid)
    try {
    finishedDirty.delete(sid)
    // 流结束只能保持当前模式；用户若正在阅读历史，settle / shiki / 换树均不得
    // 把它重新切回跟随。原本已跟随时 contentRO 仍会正常贴底。
    preserveFollowAfterStreamFinished()
    // 后台预取 records 暂存，不赋值给 records.value——零 DOM 变化
    const pendingLanded = (recs: SessionRecord[] | null) => {
      const s = getStream(sid)
      if (!s.pendingUserMessage && !s.pendingImages?.length) return true
      return !!recs && !!findLandedUserUuid(recs, s.pendingUserMessage, !!s.pendingImages?.length, s.pendingSentAt ?? 0)
    }
    // 全落判据:一轮多 API message 时 JSONL 按块拆行渐进落盘,尾段 message 可能
    // 晚于 result 数百 ms。「任一落账即换树」会把未落的 turn 孤零零留在流式区——
    // 呈现为"回答结束后底下突然多出一个思考/工具块,发下一条消息才消失"(实测复现)。
    // 必须全部 streamingTurns 的 messageId 都出现在 records 才换,否则暂存 fallback
    const allLanded = (recs: SessionRecord[] | null): boolean => {
      if (!recs) return false
      const landed = new Set(
        recs
          .filter(r => r.type === 'assistant')
          .map(r => (r.message as { id?: string } | null | undefined)?.id ?? ''),
      )
      const turns = getStream(sid).streamingTurns
      return turns.length > 0 && turns.every(t => landed.has(t.messageId))
    }
    // 空场兜底(防御纵深):settle 窗口内 turns 被清场(理论上已被 settling 门闸拦住,
    // 但保留防未知清场路径)时无换树 diff,records 直接追加应用,不得打入暂存——
    // 否则上一轮回复会从两个源同时消失
    const emptyButGrown = (recs: SessionRecord[] | null): boolean =>
      !!recs && getStream(sid).streamingTurns.length === 0 && recs.length > records.value.length
    let newRecords: SessionRecord[] | null = null
    for (const delay of [300, 400, 800]) {
      await new Promise(r => setTimeout(r, delay))
      try {
        newRecords = await invoke<SessionRecord[]>('get_session_records', {
          projectId: cs.projectId,
          sessionId: sid,
        })
      } catch { /* 下一轮重试 */ }
      if (allLanded(newRecords) && pendingLanded(newRecords)) break
      if (emptyButGrown(newRecords)) break
    }
    if (effectiveSessionId.value !== sid) return
    if (newRecords) {
      // 产物单向(v2.5.0)已保证换树像素等价:本轮 assistant 全部落账时不再等
      // 「下一个天然安全时刻」,立即原子换树+摘 turn(与自发轮落账同款模式)——
      // usage/token 标注随历史区即刻出现,不必等下一条消息
      if (emptyButGrown(newRecords)) {
        // 空场:无 turn 可摘、无换树 diff,直接追加应用
        records.value = newRecords
        deferredRecords = null
      } else if (allLanded(newRecords) && pendingLanded(newRecords)) {
        // 换树前等本轮文本的完成态 HTML 预热落缓存:预热任务排在逐段上色队列末尾,
        // await 它 = 同时等到「上色完成 + 缓存命中」两个条件——否则换树时历史区
        // cached miss 触发全文 shiki 同步渲染(卡帧),且流式区半彩半素与历史区
        // 全彩之间有颜色跳变,叠加 DOM 换树呈现为整屏闪烁
        const texts: string[] = []
        for (const t of getStream(sid).streamingTurns) {
          for (const b of t.content) {
            const txt = b.type === 'text' ? (b as { text?: string }).text : undefined
            if (txt) texts.push(persistKeyOf(txt))
          }
        }
        await Promise.all(texts.map(t => renderMarkdownDeferred(t)))
        // 等待期间可能切会话/新消息已发出,复查后再换树
        if (effectiveSessionId.value !== sid) return
        devObserveSwap('settle-swap')
        records.value = newRecords
        removeLandedTurns(sid, newRecords)
        deferredRecords = null
        if (import.meta.env.DEV) console.log(`%c ========== [detail] records settled immediately: count=${newRecords.length} sid=${sid.slice(0, 8)} ==========`, 'color:#22c55e')
      } else {
        // 落账未确认(JSONL flush 晚/reload 空手):退回暂存,下一个安全时刻应用
        deferredRecords = { sid, recs: newRecords }
        if (import.meta.env.DEV) console.log(`%c ========== [detail] records deferred: count=${newRecords.length} sid=${sid.slice(0, 8)} ==========`, 'color:#22c55e')
      }
    }
    } finally {
      settlingSessions.delete(sid)
    }
    maybeConsumeQueue()
  }
})

/**
 * 队列消费单点(审计遗留①⑥):所有"会话转入空闲"的沿都调它,空闲条件自查——
 * 本地流式/外部进程/自持进程忙(后台任务在跑)任一为真都不消费,等下一个沿。
 * consumePendingQueue 自身有 streaming 守卫 + 同步 shift,多沿并发天然防重。
 */
function maybeConsumeQueue() {
  if (stream.value.streaming || externalRunning.value || ownProcessBusy.value) return
  const cs = currentSession.value
  if (!cs?.summary.cwd) return
  // settle 在途:换树完成前发送会清掉未落账的上一轮 turns(丢内容),
  // 由 settle watcher 尾部的 maybeConsumeQueue 补发
  if (settlingSessions.has(cs.summary.id)) return
  consumePendingQueue(cs.summary.id, cs.summary.cwd)
}

// 忙态翻空闲即消费:自发轮落账摘除(live 清空)、进程退出清 live 等一切翻 false
// 的路径统一走这个沿——ownProcessBusy 闸门拦下的消息在此发出
watch(ownProcessBusy, (busy) => {
  if (!busy) maybeConsumeQueue()
})

// 自发轮落账:CLI 被 task-notification 唤醒的后台任务收尾轮结束(stream-done
// initiator=auto)时静默 reload,把已落账 turn 摘除——与历史区同 batch 原子切换。
// 不动 streaming/pending:那是用户轮的领地;pendingLandedUuid 若在本次 reload
// 中匹配成功(用户消息恰在此期间落盘),气泡走既有契约自然退场
watch(autoTurnLanded, async () => {
  if (!interactive.value) return
  const cs = currentSession.value
  const snapshot = cs ? autoLandedSessions.get(cs.summary.id) : undefined
  if (!cs || !snapshot) return
  const sid = cs.summary.id
  autoLandedSessions.delete(sid)
  await new Promise(r => setTimeout(r, 300))
  let fresh: SessionRecord[] | null = null
  try {
    fresh = await invoke<SessionRecord[]>('get_session_records', {
      projectId: cs.projectId,
      sessionId: sid,
    })
  } catch { /* 失败留给 finishedDirty 下次加载兜底 */ }
  if (!fresh || fresh.length <= records.value.length) {
    await new Promise(r => setTimeout(r, 400))
    try {
      fresh = await invoke<SessionRecord[]>('get_session_records', {
        projectId: cs.projectId,
        sessionId: sid,
      })
    } catch { /* 同上 */ }
  }
  if (effectiveSessionId.value !== sid || !fresh) return
  if (fresh.length >= records.value.length) {
    records.value = fresh
    // 同一同步段摘除已落账 turn:历史区解除 streamingMessageIds 过滤,原子切换;
    // 快照内未落盘的孤儿 turn 降级 live(记录不会再来,防永久卡「进行中」)
    removeLandedTurns(sid, fresh)
    demoteUnlandedTurns(sid, snapshot, fresh)
    finishedDirty.delete(sid)
  }
})

// 滚动跟随已移交布局层 contentRO(见上方"布局层滚动跟随"):
// 旧实现 watch(streamingTick) 带 streaming===true 守卫,回合结束后晚到的
// 子 Agent 事件渲染时恒短路(Bug:不跟随滚动),且每 tick 读 scrollHeight
// 是强制布局热点。contentRO 对增高源一视同仁,无此两病。

// --- 外部运行跟随 ---
//
// 应用关闭后 claude CLI 子进程不随窗口退出(刻意保留:任务继续跑),重开应用后
// stdout 管道已不可重接。改走伪流式:探测到该会话仍有 CLI 进程在跑(命令行含
// session-id)时,周期静默 reload jsonl 落账记录 + 保持滚动跟随,进程退出后做
// 一次收尾 reload。整段追加无打字机,但进度不再需要手动刷新。
/** typing-dots 显隐总闸:false 时点从 DOM 摘除(infinite 动画在 opacity:0 下仍持续产帧,唤醒合成器) */
const typingActive = computed(() => stream.value.streaming || externalRunning.value || hasLiveTurn.value)
/** 外部进程归属应用（父进程链解析,如 Terminal / 其他 GUI 工具),横幅与停止确认共用 */
const externalOwner = ref<string | null>(null)
/** 终止外部进程进行中:锁按钮防重复 kill,SIGTERM 后到 probe 撤横幅有数秒窗口 */
const stopping = ref(false)
let stoppingTimeout: number | null = null

/** 终止完成的感知时点 = 横幅消失(externalRunning→false);超时兜底防进程无视 SIGTERM 导致永锁 */
watch(externalRunning, (running) => {
  if (!running && stopping.value) {
    stopping.value = false
    if (stoppingTimeout != null) {
      window.clearTimeout(stoppingTimeout)
      stoppingTimeout = null
    }
  }
})
let followSessionId: string | null = null
let externalTimer: number | null = null
let probing = false
let externalIdleTicks = 0

interface ExternalSessionInfo {
  running: boolean
  pid: number | null
  owner: string | null
}

/** 静默重载:不动 loading 态/滚动状态,记录数有增长才替换(jsonl 仅追加) */
async function silentReloadRecords() {
  const cs = currentSession.value
  if (!cs) return
  try {
    const fresh = await invoke<SessionRecord[]>('get_session_records', {
      projectId: cs.projectId,
      sessionId: cs.summary.id,
    })
    // 异步窗口内可能已切会话
    if (effectiveSessionId.value !== cs.summary.id) return
    if (fresh.length > records.value.length) {
      records.value = fresh
    }
  } catch {
    // 下一轮探测重试
  }
}

async function probeExternal() {
  if (probing) return
  probing = true
  try {
    const cs = currentSession.value
    // 本地流式由 stream-event 实时驱动,无需外部探测
    if (!cs || stream.value.streaming) {
      externalRunning.value = false
      externalIdleTicks = 0
      stopExternalFollow()
      return
    }
    let running = false
    try {
      const info = await invoke<ExternalSessionInfo>('check_session_running', { sessionId: cs.summary.id })
      running = info.running
      externalOwner.value = info.owner
    } catch {
      // 探测失败视为未运行
    }
    if (effectiveSessionId.value !== cs.summary.id) return
    if (running) {
      // 进程在跑就保持运行态,不做闲置退出(API 调用等响应可能 10-30s 无输出)
      if (!externalRunning.value) {
        externalRunning.value = true
      }
      const prevCount = records.value.length
      await silentReloadRecords()
      if (records.value.length > prevCount) {
        externalIdleTicks = 0
      }
    } else if (externalRunning.value) {
      // 进程退出:收尾 reload 拿最终落账,结束跟随,消费排队消息
      externalRunning.value = false
      externalIdleTicks = 0
      await silentReloadRecords()
      stopExternalFollow()
      maybeConsumeQueue()
    } else {
      // 进程未运行且从未标记过运行态,累积空轮次后停止探测。
      // 顺带补一次队列消费:externalRunning 的两条撤销路径(切会话/本地流式起)
      // 不经过上面的退出分支,排队消息曾在此滞留(审计遗留⑥)
      externalIdleTicks++
      maybeConsumeQueue()
      if (externalIdleTicks >= 4) {
        stopExternalFollow()
      }
    }
  } finally {
    probing = false
  }
}

function startExternalFollow() {
  stopExternalFollow()
  externalRunning.value = false
  externalOwner.value = null
  stopping.value = false
  externalIdleTicks = 0
  // 先起定时器再立即探一次:未运行的会话首轮探测即自停,运行中的持续跟随
  // 3s 节拍：探测走全量进程扫描（macOS 已是纯 syscall 但仍非免费），
  // 外部进程出现/消失的感知延迟秒级即可，不追流式实时性
  externalTimer = window.setInterval(probeExternal, 3000)
  probeExternal()
}

function stopExternalFollow() {
  if (externalTimer !== null) {
    clearInterval(externalTimer)
    externalTimer = null
  }
}

onUnmounted(stopExternalFollow)

// 文件变化驱动探测：watcher 检测到 JSONL 增长时触发 projects-changed，
// 如果当前会话探测已停止且未在本地流式，重启探测以捕获孤儿进程输出。
// 仅本会话入选变更集（或 full 全量刷新）才重启——其他会话写盘不改变本会话的
// 外部进程状态，无差别重启会让多实例探测在任何会话活跃期间永不停歇
let unlistenProjectsChanged: (() => void) | null = null
listen<{ full: boolean; changes: { projectId: string; sessionId: string }[] }>('projects-changed', (event) => {
  if (externalTimer !== null) return
  const cs = currentSession.value
  if (!cs || stream.value.streaming) return
  const touched = event.payload?.full
    || event.payload?.changes?.some(c => c.sessionId === cs.summary.id)
  if (!touched) return
  startExternalFollow()
}).then(fn => { unlistenProjectsChanged = fn })
onUnmounted(() => unlistenProjectsChanged?.())

/**
 * 搜索命中定位(档案馆实例专属;工作台列不消费,防止同会话开列时抢走目标):
 * 按 uuid 反查所在消息组,滚到组锚点并闪烁。消费判据用 detail.currentSessionId
 * 保证原子性——records 未加载完时不消费不置空,留给加载完成路径。
 */
function consumeScrollTarget(): boolean {
  if (interactive.value) return false
  const target = pendingScrollTarget.value
  if (!target || target.sessionId !== detail.currentSessionId.value || loading.value) return false
  pendingScrollTarget.value = null
  const gi = messageGroups.value.findIndex(g =>
    (g.user as { uuid?: string } | null)?.uuid === target.uuid
    || g.responses.some(r => (r as { uuid?: string }).uuid === target.uuid))
  if (gi < 0) return false
  // 定位后禁跟随:外部跟随 reload 的 scrollToBottom 不得抢走落点
  detachFollow()
  const scrollEpoch = scrollFollowState.epoch
  const sessionId = effectiveSessionId.value ?? null
  const valid = () => isScrollGenerationCurrent(scrollEpoch, sessionId)
  // 三层收敛:虚拟窗口内 scrollToIndex 到大致偏移 + rAF×2 精调 scrollIntoView
  if (gi < renderGroups.value.length) {
    messageVirtualizer.value.scrollToIndex(gi, { align: 'start' })
  }
  void nextTick(() => {
    if (!valid()) return
    requestAnimationFrame(() => {
      if (!valid()) return
      requestAnimationFrame(() => {
        if (!valid()) return
        const el = scrollContainer.value?.querySelector<HTMLElement>(`[data-anchor-index="${gi}"]`)
        if (!el) return
        el.scrollIntoView({ block: 'start' })
        el.classList.add('search-hit-flash')
        setTimeout(() => el.classList.remove('search-hit-flash'), 1600)
      })
    })
  })
  return true
}

// 目标会话已是当前加载会话时(currentSession watch 不会重跑),置值即定位
watch(pendingScrollTarget, (t) => {
  if (t) consumeScrollTarget()
})

let loadedSessionId: string | null = null

watch(
  () => currentSession.value,
  async (cs) => {
    if (cs) {
      const force = finishedDirty.has(cs.summary.id)
      if (force) finishedDirty.delete(cs.summary.id)
      const sessionChanged = cs.summary.id !== loadedSessionId
      // 同一会话 summary 属性变化(标题/标签等)不重新加载 records,
      // 只有切换会话或 force(后台流式落账)才刷新
      if (sessionChanged || force) {
        loadedSessionId = cs.summary.id
        deferredRecords = null
        closeAllSubAgents()
        await loadRecords(cs.projectId, cs.summary.id, force, forkSourceOf(cs.summary.id) ?? undefined)
        loadSubAgentList(cs.projectId, cs.summary.id)
        if (force && !stream.value.streaming && effectiveSessionId.value === cs.summary.id) {
          // keepPending:后台落账的 force reload 可能早于用户消息落盘(CLI 还在
          // 排队处理),裸清会让气泡与历史区两源皆空=消息凭空消失;退场统一交给
          // pendingLandedUuid 落账匹配。keepLive:在播自发轮同理不清;
          // 已落账的 live 残留(无人消费信号的自发轮)按 records 摘除
          clearStreamingTurns(cs.summary.id, { keepPending: true, keepLive: true })
          removeLandedTurns(cs.summary.id, records.value)
        }
        // 搜索命中定位优先于默认滚底
        if (!consumeScrollTarget()) scrollToBottom()
      }
      if (cs.summary.id !== followSessionId) {
        followSessionId = cs.summary.id
        startExternalFollow()
        // webview 刷新后前端 processAlive 丢失而长活进程可能还在，按 Rust 进程表校准
        syncProcessAlive(cs.summary.id)
      }
    } else {
      loadedSessionId = null
      followSessionId = null
      stopExternalFollow()
      clearRecords()
    }
  },
  { immediate: true },
)

/** 顶栏手动刷新:磁盘记录为权威,流式已结束则连流式区残留一并重置 */
async function onReload() {
  await reloadRecords()
  const sid = effectiveSessionId.value
  if (sid && !stream.value.streaming) {
    // keepPending/keepLive 同 force 路径:手动刷新不得清掉未落账的用户消息气泡
    // 与在播自发轮——「宁可气泡多活一会,不可消息凭空消失」
    clearStreamingTurns(sid, { keepPending: true, keepLive: true })
    removeLandedTurns(sid, records.value)
    startExternalFollow()
  }
}
</script>

<template>
  <!-- 空态 -->
  <SessionContentState v-if="!currentSession" fill class="h-full">
    {{ mode === 'workbench' ? $t('session.notExist') : $t('archive.selectSession') }}
  </SessionContentState>

  <SessionSurface
    v-else
    :root-ref="bindDetailRoot"
    @pointerdown.capture="activateRenderSurface"
  >
    <SessionIdentityBar
      v-if="mode === 'archive'"
      engine-name="Claude Code"
      :title="archiveTitle"
      accent="claude"
      :tags="getMeta(currentSession.summary.id)?.tags ?? []"
    />
    <!-- 会话顶栏(单行极简:标题由列头/列表承担,不重复显示) -->
    <SessionTopBar
      :session-id="currentSession.summary.id"
      :short-id-value="shortId(currentSession.summary.id)"
      :project-id="currentSession.projectId"
      :cwd="currentSession.summary.cwd"
      :git-branch="currentSession.summary.git_branch"
      :model-string="displayModelString"
      :used-context-tokens="stream.realUsedTokens ?? lastAssistantContextSize"
      :real-context-window="stream.realContextWindow ?? currentSession.summary.context_window ?? null"
      :total-tokens="currentSession.summary.total_tokens"
      :subagent-tokens="currentSession.summary.subagent_tokens"
      :last-modified="currentSession.summary.last_modified"
      :selected-model-id="settings.modelId"
      :selected-effort="settings.effort"
      :selected-fast-mode="settings.fastMode"
      :fast-mode-notice="fastModeNotice"
      :selected-channel-id="settings.channelId"
      :resolved-channel-id="resolvedChannelId"
      :run-config="runConfig"
      :selected-advisor="settings.advisor"
      :selected-chrome="settings.chrome"
      :selected-extra-args="settings.extraArgs"
      :selected-permission-mode="settings.permissionMode"
      @model-change="onModelChange"
      @effort-change="onEffortChange"
      @fast-mode-change="onFastModeChange"
      @channel-change="onChannelChange"
      @chrome-change="onChromeChange"
      @extra-args-change="onExtraArgsChange"
      @permission-mode-change="onPermissionModeChange"
      @reload="onReload"
      @deleted="onDeleted"
    >
      <button
        v-if="asyncTasks.length > 0"
        class="p-1 rounded transition-colors flex items-center gap-0.5"
        :class="asyncSidebarOpen
          ? 'text-claude bg-claude/10'
          : asyncActiveCount > 0
            ? 'text-claude hover:bg-muted'
            : 'text-muted-foreground hover:text-foreground hover:bg-muted'"
        :title="$t('asyncTask.title')"
        @click="asyncSidebarOpen ? closeAsyncPanel() : openAsyncPanel()"
      >
        <span class="i-carbon-lightning w-3.5 h-3.5" :class="asyncActiveCount > 0 && 'animate-pulse'" />
        <span class="text-[10px] font-semibold tabular-nums leading-none">{{ asyncActiveCount > 0 ? asyncActiveCount : asyncTasks.length }}</span>
      </button>
      <button
        v-if="ledgerEntries.length > 0"
        class="p-1 rounded transition-colors flex items-center gap-0.5"
        :class="ledgerPanelOpen ? 'text-claude bg-claude/10' : 'text-muted-foreground hover:text-foreground hover:bg-muted'"
        :title="$t('fileLedger.title')"
        @click="toggleLedgerPanel"
      >
        <span class="i-carbon-catalog w-3.5 h-3.5" />
        <span class="text-[10px] font-semibold tabular-nums leading-none">{{ ledgerEntries.length }}</span>
      </button>
      <!-- Runner 跑单按钮（ref 用于点外关闭豁免） -->
      <span ref="runnerToggleBtnRef">
        <button
          v-if="interactive"
          class="p-1 rounded transition-colors flex items-center gap-0.5"
          :class="(runnerDockOpen || runnerFloatOpen)
            ? 'text-claude bg-claude/10'
            : sessionHasCrashed
              ? 'text-destructive hover:bg-muted'
              : sessionRunnersCount > 0
                ? 'text-claude hover:bg-muted'
                : 'text-muted-foreground hover:text-foreground hover:bg-muted'"
          :title="$t('runner.title')"
          @click="toggleRunnerPanel"
        >
          <span class="i-carbon-play w-3.5 h-3.5" />
          <span v-if="sessionRunnersCount > 0" class="text-[10px] font-semibold tabular-nums leading-none">{{ sessionRunnersCount }}</span>
        </button>
      </span>
    </SessionTopBar>

    <!-- 加载态 -->
    <SessionContentState v-if="loading" fill>{{ $t('session.loadingChat') }}</SessionContentState>

    <!-- 错误态 -->
    <SessionContentState v-else-if="error" tone="danger" fill>{{ error }}</SessionContentState>

    <!-- 无记录(草稿会话给引导文案) -->
    <SessionContentState v-else-if="messages.length === 0 && !stream.streaming && !stream.streamingTurns.length && !stream.pendingUserMessage && !showHelpCard" fill>
      {{ effectiveSessionId && draftCwd(effectiveSessionId) ? $t('session.draftGuide') : $t('session.noRecords') }}
    </SessionContentState>

    <!-- 对话消息流 -->
    <SessionViewport
      v-else
      :scroll-ref="bindScrollContainer"
      @wheel="onScrollWheel"
      @scroll="onScroll"
    >
      <template #overlay>
        <SessionAnchorNav
          :anchors="anchorItems"
          :scroll-container="scrollContainer"
          :on-scroll-to-index="onAnchorScrollToIndex"
        />
    <!-- 分叉草稿悬浮标注:垫底渲染期常显(不随滚动消失,原横线置于消息流顶部会被
         默认滚底藏走);发出首条消息即隐,落盘收割后 forkBadgeSource 自然为 null -->
    <div
      v-if="forkBadgeSource && !stream.streaming && !stream.pendingUserMessage"
      class="absolute top-2 left-1/2 -translate-x-1/2 z-30 flex items-center gap-1.5
             px-2.5 py-1 rounded-full border border-border bg-popover/70 backdrop-blur-md
             shadow-paper text-[11px] text-muted-foreground whitespace-nowrap"
    >
      <span class="i-carbon-branch w-3 h-3 shrink-0" />
      <span>{{ $t('session.forkedFrom', { id: forkBadgeSource.slice(0, 8) }) }}</span>
    </div>
    <!-- 会话横幅:悬浮通知层,不占文档流——出现/增高不推挤消息、不触发滚动跟随,
         根除"hook 事件陆续到达时横幅增高顶块/与用户上滚手势竞态拽回"。
         固定停留 5s 后淡出,生命周期由控制器负责 -->
    <SessionBannerOverlay
      :visible="interactive && featureBannerShown"
      :session-id="effectiveSessionId || ''"
      :resumed="bannerResumed"
      :cwd="bannerCwd"
      :model="settings.modelId"
      :effort="(settings.effort as string | null)"
      :features="htmlVisualEnabled ? [$t('settings.htmlVisual')] : []"
      :hook-events="bannerHookEvents"
    />
      </template>
    <div
      v-if="stickyUserPromptEnabled"
      ref="stickyOverlayElement"
      class="sticky-user-overlay"
    >
      <div
        v-if="stickyPending || stickyGroup?.user"
        ref="stickySurfaceElement"
        class="sticky-user-surface"
        :style="{ transform: `translate3d(0, ${stickyPushOffset}px, 0)` }"
        :title="$t('session.stickyJumpHint')"
        @click="jumpToStickyPrompt"
      >
        <ConversationUserMessage :time-label="stickyPending ? pendingTimeLabel : stickyTimeLabel">
          <UserMsgContent v-if="stickyPending" :blocks="pendingUserBlocks" />
          <UserMsgContent
            v-else-if="stickyGroup?.user"
            :blocks="contentBlocks(stickyGroup.user as any)"
            :record-uuid="stickyGroup.user.uuid ?? undefined"
          />
          <template #actions>
            <span class="flex items-center gap-0.5">
              <button
                class="sticky-nav-btn"
                :disabled="stickyPrevIndex < 0"
                :title="$t('session.stickyPrev')"
                @click.stop="jumpStickyPrev"
              ><span class="i-carbon-chevron-up w-3.5 h-3.5" /></button>
              <button
                class="sticky-nav-btn"
                :disabled="stickyNextIndex < 0"
                :title="$t('session.stickyNext')"
                @click.stop="jumpStickyNext"
              ><span class="i-carbon-chevron-down w-3.5 h-3.5" /></button>
            </span>
          </template>
        </ConversationUserMessage>
      </div>
    </div>

    <!-- 内容包裹层:所有增高源(打字机/晚到turn/落账替换/图片/cv解冻/横幅)都反映为它的高度变化,
         contentRO 观察它实现水平触发的滚动跟随 -->
    <div ref="scrollContentEl" class="space-y-4 pb-2 relative">
      <template v-if="!hideHistory">
        <!-- 渠道切换横线:会话起点的切换(本地记账,jsonl 无渠道信息) -->
        <DividerMark
          v-for="(m, j) in channelMarksByUuid.get(null) ?? []"
          :key="`channel-mark-start-${j}`"
          icon="i-carbon-cloud"
          :label="channelMarkLabel(m)"
        />
        <!-- 按轮次分组,虚拟化 + 末组豁免:
             - 非末组(0..n-2)进虚拟窗口,tanstack-vue-virtual 按 estimateSize 粗估、measureElement 校正真高;
             - 末组独立铺,保留 anchorRO/contentRO/追随滚动/pinLastGroupBeforeSwap 全套现有语义。
             - shouldVirtualize=false 时全部走"末组独立铺"路径,把 renderGroups 当作"上方历史组"照铺(渲染上等价原全铺) -->
        <div
          v-if="shouldVirtualize"
          ref="virtualBoxRef"
          :style="{ height: messageVirtualizer.getTotalSize() + 'px', position: 'relative', width: '100%' }"
        >
          <div
            v-for="vitem in messageVirtualizer.getVirtualItems()"
            :key="String(vitem.key)"
            :ref="(el) => el && messageVirtualizer.measureElement(el as Element)"
            :data-anchor-index="vitem.index"
            :data-group-key="vitem.key"
            :data-index="vitem.index"
            data-virtual-anchor="true"
            :style="{ position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${vitem.start}px)` }"
            :class="['space-y-4', { 'sticky-source-hidden': !stickyPending && stickyDisplay?.index === vitem.index }]"
          >
            <MessageGroup
              :group="renderGroups[vitem.index]"
              :gi="vitem.index"
              :day-label="dayDividers[vitem.index]"
              :time-label="groupTimeLabels[vitem.index]"
              :response-meta="groupResponseMetas[vitem.index]"
              :channel-marks-by-uuid="channelMarksByUuid"
              :model-switch-name="modelSwitchName"
              :is-model-command-record="isModelCommandRecord"
              :is-system-only-user="isSystemOnlyUser"
              :user-has-visible-content="userHasVisibleContent"
              :content-blocks="contentBlocks"
              :channel-mark-label="channelMarkLabel"
            />
          </div>
        </div>
        <!-- shouldVirtualize=false 时:renderGroups 上方历史组按原顺序全铺(渲染等价) -->
        <template v-else>
          <div
            v-for="(group, gi) in renderGroups"
            :key="messageGroupKey(group, gi)"
            :data-anchor-index="gi"
            :data-group-key="messageGroupKey(group, gi)"
            :class="['space-y-4', { 'sticky-source-hidden': !stickyPending && stickyDisplay?.index === gi }]"
          >
            <MessageGroup
              :group="group"
              :gi="gi"
              :day-label="dayDividers[gi]"
              :time-label="groupTimeLabels[gi]"
              :response-meta="groupResponseMetas[gi]"
              :channel-marks-by-uuid="channelMarksByUuid"
              :model-switch-name="modelSwitchName"
              :is-model-command-record="isModelCommandRecord"
              :is-system-only-user="isSystemOnlyUser"
              :user-has-visible-content="userHasVisibleContent"
              :content-blocks="contentBlocks"
              :channel-mark-label="channelMarkLabel"
            />
          </div>
        </template>
        <!-- 末组独立铺(始终豁免虚拟化) -->
        <div
          v-if="lastGroup"
          :key="messageGroupKey(lastGroup, lastGroupIndex)"
          :data-anchor-index="lastGroupIndex"
          :data-group-key="messageGroupKey(lastGroup, lastGroupIndex)"
          :class="['space-y-4', { 'sticky-source-hidden': !stickyPending && stickyDisplay?.index === lastGroupIndex }]"
        >
          <MessageGroup
            :group="lastGroup"
            :gi="lastGroupIndex"
            :day-label="dayDividers[lastGroupIndex]"
            :time-label="groupTimeLabels[lastGroupIndex]"
            :response-meta="groupResponseMetas[lastGroupIndex]"
            :channel-marks-by-uuid="channelMarksByUuid"
            :model-switch-name="modelSwitchName"
            :is-model-command-record="isModelCommandRecord"
            :is-system-only-user="isSystemOnlyUser"
            :user-has-visible-content="userHasVisibleContent"
            :content-blocks="contentBlocks"
            :channel-mark-label="channelMarkLabel"
            auto-open-artifact
            granular-visibility
          />
        </div>
        <!-- 锚点失效的切换横线兜底:末尾按序渲染,不静默消失 -->
        <DividerMark
          v-for="(m, j) in unanchoredChannelMarks"
          :key="`channel-mark-tail-${j}`"
          icon="i-carbon-cloud"
          :label="channelMarkLabel(m)"
        />
      </template>

      <!-- 流式区:pendingUserMessage + streamingTurns(横幅已移出文档流,悬浮层见上方) -->
      <div v-if="stream.pendingUserMessage || stream.pendingImages?.length || stream.streamingTurns.length || (stream.streaming && stream.streamingTurns.length === 0)" class="space-y-4">
        <!-- 落账接管即让位:pendingLandedUuid 非 null 时历史条与气泡同帧原子切换,无双显无空窗 -->
        <div
          v-if="(stream.pendingUserMessage || stream.pendingImages?.length) && !pendingLandedUuid"
          ref="pendingStickyRef"
          :class="{ 'sticky-source-hidden': stickyPending }"
        >
          <ConversationUserMessage :time-label="pendingTimeLabel">
            <UserMsgContent :blocks="pendingUserBlocks" />
          </ConversationUserMessage>
        </div>

        <AssistantResponseFrame
          v-if="stream.streamingTurns.length"
          :meta="streamingResponseMeta"
          :show-footer="!!streamResponseCompletedAt"
        >
          <div
            v-for="turn in stream.streamingTurns"
            :key="turn.messageId"
            class="stream-response-entry"
            :class="{ 'settled-turn-cv': !turn.live }"
          >
            <ContentBlockList
              :blocks="filterConsumedResults(turn.content)"
              :streaming="!!turn.live"
            />
          </div>
          <ArtifactPreviewList
            v-if="currentSession?.summary.cwd"
            :candidates="streamingArtifactCandidates"
            :root="currentSession.summary.cwd"
            :auto-open="!!streamResponseCompletedAt"
          />
        </AssistantResponseFrame>

        <div v-if="stream.streaming && stream.streamingTurns.length === 0" class="flex gap-3">
          <div class="w-0.5 shrink-0 rounded-full bg-claude/60" />
          <div class="text-xs text-muted-foreground flex items-center gap-1.5">
            <span v-if="stream.pendingImages?.length" class="i-carbon-upload w-3 h-3 animate-pulse" />
            {{ stream.pendingImages?.length ? $t('session.sending') : $t('session.thinking') }}
          </div>
        </div>
      </div>

      <SessionTypingIndicator :active="typingActive" />

      <div v-if="stream.streamError" class="px-3 py-2 rounded-md bg-destructive/10 text-destructive text-xs">
        {{ stream.streamError }}
        <template v-if="stream.streamError.includes('EPERM')">
          <div class="mt-1.5 text-foreground/80">{{ t('session.epermHint') }}</div>
          <button
            class="mt-1.5 px-2 py-1 rounded border border-border text-foreground/90 hover:bg-accent transition-colors"
            @click="invoke('open_privacy_settings', { panel: 'filesAndFolders' })"
          >{{ t('session.epermOpenSettings') }}</button>
        </template>
      </div>

      <!-- /help 本地帮助卡片 -->
      <SlashHelpCard v-if="showHelpCard" :commands="allSlashCommands" />

      <!-- 回到底部:用户上滚脱离底部时,贴滚动视口底部 -->
      <SessionBackToBottom v-if="!followStreaming" @click="resumeFollow()" />
    </div>
    </SessionViewport>

    <!-- 工作台列:权限/提问/计划卡片(固定在输入栏上方,按工具分发) -->
    <SessionInteractionPanel
      v-if="interactive && permissionRequest"
    >
      <component
        :is="requestCard"
        :key="permissionRequest.requestId"
        :request="permissionRequest"
        @decide="onPermissionDecide"
      />
    </SessionInteractionPanel>

    <!-- 工作台列:输入栏 + 斜杠命令面板(档案馆只读化:整块不渲染;赛马模式由共享输入替代) -->
    <SessionComposer
      v-if="interactive && !hideInput && currentSession.summary.cwd"
      :dragging="imageInput.isDragging.value"
      :busy="stream.streaming || externalRunning || ownProcessBusy"
      :has-content="!!inputText.trim() || !!imageInput.images.value.length"
      can-send-while-busy
      :stop-disabled="stopping"
      :stop-loading="stopping"
      :stop-variant="externalRunning && !stream.streaming ? 'danger' : 'accent'"
      :stop-label="externalRunning && !stream.streaming
        ? (stopping ? $t('session.terminating') : $t('session.terminateExternal'))
        : $t('common.stop')"
      @send="handleSend"
      @stop="onStop"
    >
      <template #notices>
        <div v-if="slashError" class="mb-1 text-xs text-destructive">
          {{ slashError }}
        </div>

        <div v-if="slashNotice" class="mb-1 text-xs text-muted-foreground flex items-center gap-1.5">
          <span class="i-carbon-terminal w-3 h-3 shrink-0" />
          {{ slashNotice }}
        </div>

        <!-- 外部运行跟随提示（能解析出归属方时点名是谁在跑） -->
        <div v-if="externalRunning" class="mb-1 text-xs text-muted-foreground flex items-center gap-1.5">
          <span class="w-1.5 h-1.5 rounded-full bg-claude animate-pulse shrink-0" />
          {{ externalOwner ? $t('session.externalRunningBy', { owner: externalOwner }) : $t('session.externalRunning') }}
        </div>
      </template>

      <template #overlay>
        <SlashCommandPanel
          :visible="slashPanelVisible"
          :query="inputText"
          :skills="workshopAssets?.skills"
          :commands="workshopAssets?.commands"
          class="absolute bottom-full left-4 mb-1"
          @select="onSlashSelect"
          @close="onSlashClose"
        />
      </template>

      <template #queue>
        <SessionComposerQueue :items="composerQueueItems" @remove="removeComposerQueueItem" />
      </template>

      <template #attachments>
        <SessionComposerAttachments
          :images="imageInput.images.value"
          :dragging="imageInput.isDragging.value"
          :error="imageInput.lastError.value?.message"
          @remove="imageInput.removeImage"
        />
      </template>

      <template #field="{ fieldClass }">
        <SessionComposerField
          ref="composerFieldRef"
          v-model="inputText"
          :placeholder="$t('session.inputPlaceholder')"
          :class="fieldClass"
          @keydown="onInputKeydown"
          @input="onInputChange"
          @keyup="syncCursor"
          @click="syncCursor"
          @select="syncCursor"
        />
      </template>

    </SessionComposer>

    <!-- 档案馆:常驻只读条(FR-009) -->
    <SessionReadonlyBar
      v-if="!interactive"
      :label="workbenchHome ? $t('session.runningInWorkbench', { name: workbenchHome.name }) : $t('session.readonlyPreview')"
    >
      <button
        class="shrink-0 px-2.5 py-1 text-xs rounded-md border border-border text-muted-foreground hover:text-foreground hover:bg-muted transition-colors flex items-center gap-1"
        :disabled="summaryGenerating"
        @click="onGenerateSummary"
      >
        <span v-if="summaryGenerating" class="i-carbon-renew w-3 h-3 animate-spin" />
        <span v-else class="i-carbon-text-short-paragraph w-3 h-3" />
        {{ currentSummary ? $t('archive.refreshSummary') : $t('archive.generateSummary') }}
      </button>
      <button
        class="shrink-0 px-2.5 py-1 text-xs rounded-md bg-primary text-primary-foreground hover:shadow-paper transition-shadow"
        @click="onOpenInWorkbench"
      >
        {{ workbenchHome ? $t('session.goTo') : $t('session.openInWorkbench') }}
      </button>
    </SessionReadonlyBar>
    
    <!-- Runner 悬浮面板：列内 absolute，随列滚动/缩放天然跟随 -->
    <div
      v-if="runnerFloatOpen"
      ref="runnerFloatRef"
      class="runner-float"
      :style="{ height: `${runnerFloatHeightRatio * 100}%` }"
    >
      <RunnerPanel
        mode="float"
        :session-id="currentSession?.summary.id ?? ''"
        :session-cwd="currentSession?.summary.cwd ?? ''"
        :project-name="currentSession?.projectId ?? ''"
        @close="closeRunnerPanel"
        @toggle-pin="toggleRunnerPin"
        @toggle-dock="toggleRunnerDock"
      />
      <div class="runner-float-resize" @mousedown="onRunnerResizeStart" />
    </div>
    <template #side-panel>
    <SessionSidePanel
      :mounted="sidePanelDom"
      :expanded="sidePanelExpanded"
      :width="sidebarTargetWidth"
    >
      <AsyncTaskPanel
        v-if="asyncPanelVisible"
        ref="asyncPanelRef"
        :tasks="asyncTasks"
        :open-tabs="subAgentTabs"
        :active-tab-id="subAgentActiveTabId"
        :project-id="currentSession?.projectId ?? null"
        :session-id="currentSession?.summary.id ?? null"
        :stopping-task-ids="stoppingAsyncTaskIds"
        @select-agent="toggleSubAgent($event)"
        @close-tab="closeSubAgentTab($event)"
        @close="closeAsyncPanel"
        @locate="locateToolUse"
        @stop="onStopAsyncTask"
      />
      <FileLedgerPanel
        v-if="ledgerPanelOpen"
        ref="ledgerPanelRef"
        :modified="ledgerModified"
        :read-only="ledgerReadOnly"
        :cwd="currentSession?.summary.cwd ?? null"
        :session-id="currentSession?.summary.id ?? null"
        @close="ledgerPanelOpen = false"
        @locate="locateToolUse"
      />
      <RunnerPanel
        v-if="runnerDockOpen"
        mode="dock"
        :session-id="currentSession?.summary.id ?? ''"
        :session-cwd="currentSession?.summary.cwd ?? ''"
        :project-name="currentSession?.projectId ?? ''"
        @close="closeRunnerPanel"
        @toggle-pin="toggleRunnerPin"
        @toggle-dock="toggleRunnerDock"
      />
    </SessionSidePanel>
    </template>
  </SessionSurface>

</template>

<style scoped>
.msg-block {
  contain: layout style;
}
/* 消息组级按需渲染:屏外轮次跳过 style/layout/paint,把横滚整列解冻的尖峰
   降为单轮次粒度(审计 P1-2 第一步;列级 content-visibility 在 SortableColumn 互补保留)。
   auto 关键字记住实际渲染高度,首次以 300px 估算——组高度差异大,记忆后滚动条稳定 */
.msg-group-cv {
  content-visibility: auto;
  contain-intrinsic-size: auto 300px;
}
/* 自发轮完成后可能暂留到 JSONL 落账；稳定 turn 移出视口后不再参与渲染遍历。 */
.settled-turn-cv {
  content-visibility: auto;
  contain-intrinsic-size: auto 220px;
}
.sticky-user-overlay {
  position: sticky;
  top: 0;
  z-index: 20;
  height: 0;
  overflow: visible;
  pointer-events: none;
}
.sticky-user-surface {
  cursor: pointer;
  pointer-events: auto;
  will-change: transform;
}
.sticky-source-hidden :deep(.conversation-user-message) { visibility: hidden; }
.sticky-nav-btn {
  border: none;
  background: none;
  padding: 1px;
  border-radius: var(--radius);
  color: var(--muted-foreground);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
}
.sticky-nav-btn:hover:not(:disabled) { color: var(--foreground); background: var(--muted); }
.sticky-nav-btn:disabled { opacity: 0.3; cursor: default; }
/* 搜索命中定位落点反馈:accent 底色淡出(亮暗双套走 token) */
.search-hit-flash {
  border-radius: 6px;
  animation: search-hit-fade 1.6s ease-out;
}
@keyframes search-hit-fade {
  0%, 25% { background-color: color-mix(in srgb, var(--accent) 12%, transparent); }
  100% { background-color: transparent; }
}
/* Runner 悬浮面板 */
/* 定位由 placeRunnerFloat 动态计算（锚定本会话列，随列走），此处只留外观 */
.runner-float-resize {
  position: absolute;
  left: 0;
  right: 0;
  bottom: -3px;
  height: 7px;
  cursor: ns-resize;
}
.runner-float-resize:hover {
  background: linear-gradient(to bottom, transparent 2px, var(--border) 3px, var(--border) 4px, transparent 5px);
}
.runner-float {
  position: absolute;
  top: 44px;
  left: 12px;
  right: 12px;
  min-height: 200px;
  background: var(--popover);
  border: 1px solid var(--border);
  border-radius: 6px;
  box-shadow: var(--shadow-paper-lifted);
  z-index: 30;
  display: flex;
  flex-direction: column;
}
</style>
