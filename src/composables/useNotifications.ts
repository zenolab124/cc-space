import { ref, computed, watch } from 'vue'
import i18n from '../locales'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
  registerActionTypes,
  onAction,
} from '@tauri-apps/plugin-notification'
import { displayTitle } from '@/types'
import { useSessionMeta } from './useSessionMeta'
import { useProjects } from './useProjects'
import { useWorkbench } from './useWorkbench'
import { useUiState } from './useUiState'
import { onStreamFinished, getStream, useStreaming } from './useStreaming'
import {
  usePermissionRequests,
  respondRequest,
  isInteractiveTool,
  type PermissionRequest,
} from './usePermissionRequests'
import { respondInteraction as respondEngineInteraction } from '@/engines/client'
import { sessionUiId } from '@/engines/integration'
import type { InteractionRef, InteractionRequest, RuntimeEventEnvelope } from '@/engines/types'

/**
 * 应用内通知层(FR-006/007/008/010)
 *
 * - 持久型 toast:权限请求(从权限队列直接派生——与左列决策条/列内权限卡同源,任一处处理同步消失)、
 *   出错停住(errorEvents 集合:工作台内流式错误 + 外部会话 api_error 兜底)
 * - 瞬态型 toast:5 秒自动消失(任务完成/软上限收起/超时自动拒绝/状态重置)
 * - 角标计数 = 等权限会话数 + 未处理错误数(与 toast 同源)
 * - 系统通知:应用非前台时同步发送,权限被拒静默降级
 */

// ---- 瞬态 toast ----

export interface TransientToast {
  id: number
  title: string
  sub?: string
}

const transients = ref<TransientToast[]>([])
let transientSeq = 0
const TRANSIENT_MS = 5000

function notifyTransient(title: string, sub?: string) {
  const id = ++transientSeq
  transients.value.push({ id, title, sub })
  window.setTimeout(() => {
    transients.value = transients.value.filter(t => t.id !== id)
  }, TRANSIENT_MS)
}

// ---- 错误事件(持久型来源之二) ----

export interface SessionErrorEvent {
  sessionId: string
  /** 错误摘要(等宽副行) */
  sub: string
  /** stream=工作台内流式错误(可重试);external=外部会话兜底(FR-010,仅「去会话」) */
  source: 'stream' | 'external'
  updatedAt: number
}

/** sessionId → 错误事件;同一会话去重为一条,新事件更新内容 */
const errorEvents = ref<Map<string, SessionErrorEvent>>(new Map())

function upsertErrorEvent(sessionId: string, sub: string, source: 'stream' | 'external') {
  const next = new Map(errorEvents.value)
  next.set(sessionId, { sessionId, sub: sub.slice(0, 120), source, updatedAt: Date.now() })
  errorEvents.value = next
}

/** 确认/解除某会话的错误事件(重试成功、去会话、手动 dismiss、会话退出工作台) */
function dismissError(sessionId: string) {
  if (!errorEvents.value.has(sessionId)) return
  const next = new Map(errorEvents.value)
  next.delete(sessionId)
  errorEvents.value = next
}

// ---- 持久型 toast 派生(同源同步的关键:不复制状态,直接 computed) ----

export type PersistentToast =
  | {
      kind: 'permission'
      key: string
      sessionId: string
      title: string
      sub: string
      request: PermissionRequest
      at: number
    }
  | {
      kind: 'engine-interaction'
      key: string
      sessionId: string
      title: string
      sub: string
      interaction: InteractionRequest
      at: number
    }
  | {
      kind: 'error'
      key: string
      sessionId: string
      title: string
      sub: string
      source: 'stream' | 'external'
      canRetry: boolean
      at: number
    }

function sessionTitle(sessionId: string): string {
  const { projects } = useProjects()
  const { getMeta } = useSessionMeta()
  for (const p of projects.value) {
    const s = p.sessions.find(s => s.id === sessionId)
    if (s) return displayTitle(s, getMeta(sessionId)?.title)
  }
  const workbench = useWorkbench()
  if (workbench.draftCwd(sessionId) || workbench.engineDraft(sessionId)) return i18n.global.t('session.newSessionTitle')
  return sessionId.slice(0, 8)
}

/** 查会话 cwd(去会话→展开列需要;重试需要) */
function sessionCwd(sessionId: string): string | null {
  const { projects } = useProjects()
  for (const p of projects.value) {
    const s = p.sessions.find(s => s.id === sessionId)
    if (s) return s.cwd
  }
  const workbench = useWorkbench()
  return workbench.draftCwd(sessionId) ?? workbench.engineDraft(sessionId)?.cwd ?? null
}

/** 持久 toast / 系统通知的类型标签：交互工具按真实语义命名，不再一律「权限请求」 */
export function requestKindLabel(toolName: string): string {
  switch (toolName) {
    case 'AskUserQuestion': return i18n.global.t('notification.claudeAsking')
    case 'ExitPlanMode': return i18n.global.t('notification.planApproval')
    case 'EnterPlanMode': return i18n.global.t('notification.enterPlanMode')
    default: return i18n.global.t('notification.permissionRequest')
  }
}

function permissionSub(req: PermissionRequest): string {
  const input = req.input
  // 交互工具:摘要取内容本身,比工具名更有信息量
  if (req.toolName === 'AskUserQuestion') {
    const qs = input.questions
    const first = Array.isArray(qs) && typeof (qs[0] as any)?.question === 'string'
      ? (qs[0] as any).question as string
      : ''
    const tail = Array.isArray(qs) && qs.length > 1 ? ` ${i18n.global.t('notification.nQuestions', { n: qs.length })}` : ''
    return first ? `${first.length > 40 ? first.slice(0, 40) + '…' : first}${tail}` : i18n.global.t('notification.waitingAnswer')
  }
  if (req.toolName === 'ExitPlanMode') {
    const plan = typeof input.plan === 'string' ? input.plan.split('\n')[0].replace(/^#+\s*/, '') : ''
    return plan ? (plan.length > 48 ? plan.slice(0, 48) + '…' : plan) : i18n.global.t('notification.planReady')
  }
  if (req.toolName === 'EnterPlanMode') {
    return i18n.global.t('notification.enterPlanDesc')
  }
  for (const k of ['file_path', 'command', 'url', 'pattern']) {
    const v = input[k]
    if (typeof v === 'string' && v) {
      const s = v.split('\n')[0]
      return `${req.toolName} · ${s.length > 48 ? s.slice(0, 48) + '…' : s}`
    }
  }
  return req.toolName
}

const { queue } = usePermissionRequests()
const engineInteractions = ref<Map<string, { request: InteractionRequest; sessionId: string; at: number }>>(new Map())

function engineInteractionKey(reference: InteractionRef): string {
  const runtimeId = typeof reference.runtimeId === 'string' ? reference.runtimeId : reference.runtimeId[0]
  return `${sessionUiId(reference.session)}:${runtimeId}:${reference.requestId}`
}

const persistentToasts = computed<PersistentToast[]>(() => {
  const list: PersistentToast[] = []
  // 权限:同一会话仅保留一条(队首),后续请求依次顶上(去重更新)
  const seen = new Set<string>()
  for (const req of queue.value) {
    if (seen.has(req.sessionId)) continue
    seen.add(req.sessionId)
    list.push({
      kind: 'permission',
      key: `perm:${req.sessionId}`,
      sessionId: req.sessionId,
      title: sessionTitle(req.sessionId),
      sub: permissionSub(req),
      request: req,
      at: req.timestamp,
    })
  }
  for (const entry of engineInteractions.value.values()) {
    list.push({
      kind: 'engine-interaction',
      key: `engine-perm:${engineInteractionKey(entry.request.reference)}`,
      sessionId: entry.sessionId,
      title: sessionTitle(entry.sessionId),
      sub: entry.request.title || entry.request.kind,
      interaction: entry.request,
      at: entry.at,
    })
  }
  for (const ev of errorEvents.value.values()) {
    list.push({
      kind: 'error',
      key: `err:${ev.sessionId}`,
      sessionId: ev.sessionId,
      title: sessionTitle(ev.sessionId),
      sub: ev.sub,
      source: ev.source,
      canRetry: ev.source === 'stream' && !!getStream(ev.sessionId).lastSent,
      at: ev.updatedAt,
    })
  }
  return list.sort((a, b) => a.at - b.at)
})

/** ActivityBar 角标(FR-007):未处理持久型事件数,瞬态不计入 */
const badgeCount = computed(() => persistentToasts.value.length)

/** 同屏持久型上限 3 条,超出折叠;点击汇总条展开全部 */
const toastsExpanded = ref(false)

// ---- 系统通知(FR-008) ----

let notifPermissionAsked = false

/**
 * 应用窗口非前台或最小化时发系统通知;前台抑制。
 * 首次需要发送时才请求授权;权限被拒/发送失败静默降级,不影响应用内 toast。
 * export：会话状态跟踪扩展(useTurnSignals)复用同一抑制与授权逻辑。
 */
export async function maybeNotifySystem(title: string, body: string, extra?: { actionTypeId?: string; extra?: Record<string, unknown> }) {
  try {
    const win = getCurrentWindow()
    const [focused, minimized] = await Promise.all([
      win.isFocused(),
      win.isMinimized(),
    ])
    if (focused && !minimized) return
    let granted = await isPermissionGranted()
    if (!granted && !notifPermissionAsked) {
      notifPermissionAsked = true
      granted = (await requestPermission()) === 'granted'
    }
    if (granted) {
      sendNotification({ title, body, ...extra })
    }
  } catch (_) {
    // 静默降级:通知层自身失败不弹错误、不阻断应用内 toast
  }
}

// ---- 「去会话」(FR-006/008/010 共用跳转闭环) ----

function goToSession(sessionId: string) {
  const { openSession, findSession, expandSession, setActiveTab } = useWorkbench()
  const { switchSection } = useUiState()
  const found = findSession(sessionId)
  let collapsed: string[]
  if (found) {
    setActiveTab(found.tab.id)
    collapsed = expandSession(found.tab.id, sessionId).collapsedSessionIds
  } else {
    collapsed = openSession(sessionId).collapsedSessionIds
  }
  if (collapsed.length > 0) {
    notifyTransient(i18n.global.t('notification.collapsed', { names: collapsed.map(sessionTitle).join('、') }))
  }
  switchSection('workbench')
  // 错误事件已被注意:同步解除其 toast(左列卡状态仍如实反映)
  dismissError(sessionId)
}

// ---- 初始化:挂接各事件源(App 启动调用一次) ----

let initialized = false

export async function initNotificationLayer(): Promise<void> {
  if (initialized) return
  initialized = true

  const { isSessionVisibleInWorkbench, findSession } = useWorkbench()

  // 0. 注册 Mac 原生通知 actions(Allow / Deny 按钮)
  try {
    await registerActionTypes([{
      id: 'permission-decision',
      actions: [
        { id: 'allow', title: i18n.global.t('common.allow'), foreground: true },
        { id: 'deny', title: i18n.global.t('common.deny'), destructive: true },
      ],
    }])
    await onAction((notification) => {
      const n = notification as unknown as { actionId?: string; extra?: Record<string, unknown> }
      const requestId = n.extra?.requestId as string | undefined
      if (!requestId || !n.actionId) return
      void respondRequest(requestId, n.actionId === 'allow' ? 'allow_once' : 'deny')
    })
  } catch (_) {
    // 注册失败静默降级:系统通知不带 action 按钮,不影响应用内 toast
  }

  // 1. 流结束:错误→持久型;完成→不可见时瞬态;非前台同步系统通知
  onStreamFinished((sessionId, hasError) => {
    const title = sessionTitle(sessionId)
    if (hasError) {
      const err = getStream(sessionId).streamError ?? i18n.global.t('notification.streamError')
      upsertErrorEvent(sessionId, err, 'stream')
      void maybeNotifySystem(i18n.global.t('notification.errorStopped'), `${title} · ${err.slice(0, 60)}`)
    } else {
      const { activeSection } = useUiState()
      const visible =
        activeSection.value === 'workbench' && isSessionVisibleInWorkbench(sessionId)
      if (!visible) {
        notifyTransient(i18n.global.t('notification.taskComplete', { title }))
      }
      void maybeNotifySystem(i18n.global.t('notification.taskCompleteTitle'), title)
    }
  })

  // 2. 权限请求入队(watch 队列新增):非前台系统通知
  const knownRequests = new Set<string>()
  watch(
    () => queue.value.map(r => r.requestId),
    (ids) => {
      for (const req of queue.value) {
        if (!knownRequests.has(req.requestId)) {
          knownRequests.add(req.requestId)
          const isInteractive = isInteractiveTool(req.toolName)
          void maybeNotifySystem(
            requestKindLabel(req.toolName),
            `${sessionTitle(req.sessionId)} · ${permissionSub(req)}`,
            isInteractive ? undefined : {
              actionTypeId: 'permission-decision',
              extra: { requestId: req.requestId },
            },
          )
        }
      }
      // 收缩集合防泄漏
      const live = new Set(ids)
      for (const id of knownRequests) {
        if (!live.has(id)) knownRequests.delete(id)
      }
    },
  )

  // 3. 外部会话出错兜底(FR-010):不属于任何工作台的会话 jsonl 新增 api_error
  await listen<{ sessionId: string; projectId: string; content: string }>(
    'session-api-error',
    (e) => {
      const { sessionId, content } = e.payload
      // 工作台内会话的出错由流式链路处理,不走本通道(链路互斥,防重复通知)
      if (findSession(sessionId)) return
      upsertErrorEvent(sessionId, content || i18n.global.t('notification.apiError'), 'external')
      void maybeNotifySystem(i18n.global.t('notification.errorStopped'), `${sessionTitle(sessionId)} · ${(content || '').slice(0, 60)}`)
    },
  )

  // 4. 通用引擎运行时：批准请求、错误与完成通知均以结构化会话键归档。
  await listen<RuntimeEventEnvelope[]>('engine-runtime-events', event => {
    for (const envelope of event.payload) {
      const sessionId = sessionUiId(envelope.session)
      const runtimeEvent = envelope.event
      if (runtimeEvent.kind === 'interactionRequested') {
      const request = runtimeEvent.request as unknown as InteractionRequest
      const next = new Map(engineInteractions.value)
      next.set(engineInteractionKey(request.reference), { request, sessionId, at: Date.now() })
      engineInteractions.value = next
      void maybeNotifySystem(
        i18n.global.t('notification.permissionRequest'),
        `${sessionTitle(sessionId)} · ${request.title || request.kind}`,
      )
      } else if (runtimeEvent.kind === 'interactionResolved') {
      const reference = runtimeEvent.reference as unknown as InteractionRef
      if (reference?.requestId) {
        const next = new Map(engineInteractions.value)
        next.delete(engineInteractionKey(reference))
        engineInteractions.value = next
      }
      } else if (runtimeEvent.kind === 'runtimeError') {
      const message = String(runtimeEvent.message || i18n.global.t('notification.streamError'))
      upsertErrorEvent(sessionId, message, 'external')
      void maybeNotifySystem(i18n.global.t('notification.errorStopped'), `${sessionTitle(sessionId)} · ${message.slice(0, 60)}`)
      } else if (runtimeEvent.kind === 'turnCompleted') {
      const next = new Map(engineInteractions.value)
      for (const [id, pending] of next) {
        if (pending.sessionId === sessionId) next.delete(id)
      }
      engineInteractions.value = next
      if (runtimeEvent.error) {
        const message = String(runtimeEvent.error)
        upsertErrorEvent(sessionId, message, 'external')
      } else {
        const { activeSection } = useUiState()
        const visible = activeSection.value === 'workbench' && isSessionVisibleInWorkbench(sessionId)
        if (!visible) notifyTransient(i18n.global.t('notification.taskComplete', { title: sessionTitle(sessionId) }))
        void maybeNotifySystem(i18n.global.t('notification.taskCompleteTitle'), sessionTitle(sessionId))
      }
      }
    }
  })

  // 5. Remote Control 判决 toast:仅手动开关回报成败(连接时自动开启的失败静默,
  //    避免第三方渠道用户每次起进程都被骚扰);按钮状态由 useStreaming 监听同一事件负责
  await listen<{ session_id: string; active: boolean; manual: boolean; error?: string | null }>(
    'rc-status',
    (e) => {
      if (!e.payload.manual) return
      if (e.payload.error) {
        notifyTransient(i18n.global.t('workbench.column.rcFailed'), e.payload.error)
      } else {
        notifyTransient(i18n.global.t(e.payload.active ? 'workbench.column.rcOpened' : 'workbench.column.rcClosed'))
      }
    },
  )
}

export async function respondEngineToast(
  toast: Extract<PersistentToast, { kind: 'engine-interaction' }>,
  action: 'allow' | 'allowSession' | 'deny',
) {
  const options = toast.interaction.options
  const option = action === 'deny'
    ? options.find(item => item.dangerous && /decline|deny/i.test(item.id)) ?? options.find(item => item.dangerous)
    : action === 'allowSession'
      ? options.find(item => !item.dangerous && /session/i.test(item.id)) ?? options.find(item => !item.dangerous)
      : options.find(item => !item.dangerous && !/session/i.test(item.id)) ?? options.find(item => !item.dangerous)
  if (!option) return
  await respondEngineInteraction(toast.interaction.reference, option.id)
  const next = new Map(engineInteractions.value)
  next.delete(engineInteractionKey(toast.interaction.reference))
  engineInteractions.value = next
}

// ---- 操作:toast 上的按钮 ----

/** 错误 toast「重试」:重发最近一次消息,成功后解除错误事件 */
async function retryFromToast(sessionId: string): Promise<void> {
  const { retrySession } = useStreaming()
  const ok = await retrySession(sessionId)
  if (ok) dismissError(sessionId)
}

export function useNotifications() {
  return {
    transients,
    persistentToasts,
    badgeCount,
    toastsExpanded,
    notifyTransient,
    upsertErrorEvent,
    dismissError,
    goToSession,
    retryFromToast,
    sessionTitle,
    sessionCwd,
  }
}
