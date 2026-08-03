import { computed, type Ref, type ComputedRef } from 'vue'
import i18n from '../locales'
import { useSessionStream } from './useStreaming'
import { queueForSession } from './usePermissionRequests'
import { useNotifications } from './useNotifications'
import { signalFor } from './useTurnSignals'
import { useEngineRuntimeState } from '@/engines/runtimeState'

/**
 * 会话运行状态派生(FR-003 状态枚举):监控卡状态行与右区列头共用。
 * 状态点颜色:运行三态(流式/Workflow/等待工具)= primary;等待权限 = accent;
 * 出错 = destructive;空闲 = border 色。
 */

export type SessionStatusKey =
  | 'streaming'
  | 'workflow'
  | 'waiting_tool'
  | 'waiting_permission'
  | 'external_running'
  | 'error'
  | 'idle'

export interface SessionStatus {
  key: SessionStatusKey
  label: string
  /** 状态点配色(UnoCSS class) */
  dotClass: string
  /** 运行态脉冲动画 */
  pulse: boolean
  /** 卡片左边框语义('accent' | 'destructive' | null) */
  edge: 'accent' | 'destructive' | null
}

const STATUS_META_STATIC: Record<SessionStatusKey, { dotClass: string; pulse: boolean; edge: 'accent' | 'destructive' | null; labelKey: string }> = {
  streaming: { dotClass: 'bg-primary', pulse: true, edge: null, labelKey: 'session.streaming' },
  workflow: { dotClass: 'bg-primary', pulse: true, edge: null, labelKey: '' },
  waiting_tool: { dotClass: 'bg-primary', pulse: true, edge: null, labelKey: 'session.waitingTool' },
  waiting_permission: { dotClass: 'bg-accent', pulse: false, edge: 'accent', labelKey: 'session.waitingPermission' },
  // labelKey 用 externalActive：externalRunning 已被会话详情的后台运行横幅占用
  external_running: { dotClass: 'bg-primary', pulse: true, edge: null, labelKey: 'session.externalActive' },
  error: { dotClass: 'bg-destructive', pulse: false, edge: 'destructive', labelKey: 'session.error' },
  idle: { dotClass: 'bg-border', pulse: false, edge: null, labelKey: 'session.idle' },
}

function getStatusMeta(key: SessionStatusKey): Omit<SessionStatus, 'key'> {
  const meta = STATUS_META_STATIC[key]
  const label = meta.labelKey ? i18n.global.t(meta.labelKey) : 'Workflow'
  return { label, dotClass: meta.dotClass, pulse: meta.pulse, edge: meta.edge }
}

export function useSessionStatus(
  sessionId: Ref<string | null> | ComputedRef<string | null>,
) {
  const stream = useSessionStream(sessionId)
  const perms = queueForSession(sessionId)
  const { persistentToasts } = useNotifications()
  const engineRuntime = useEngineRuntimeState(sessionId)

  return computed<SessionStatus>(() => {
    const sid = sessionId.value
    let key: SessionStatusKey = 'idle'
    const enginePhase = engineRuntime.value.snapshot?.phase
    if (perms.value.length > 0 || enginePhase === 'awaitingInteraction') {
      key = 'waiting_permission'
    } else if (
      stream.value.streamError ||
      (sid && persistentToasts.value.some(t => t.kind === 'error' && t.sessionId === sid))
    ) {
      key = 'error'
    } else if (enginePhase === 'failed' || enginePhase === 'exited') {
      key = 'error'
    } else if (enginePhase === 'running' || stream.value.streaming || stream.value.streamingTurns.some(t => t.live)) {
      // live turn:CLI 自发轮(task-notification 后台任务收尾轮)在 streaming=false
      // 下进行中——内容正在流入,不应显示空闲(审计遗留②)
      const tool = stream.value.activeTool
      if (tool?.startsWith('Workflow')) key = 'workflow'
      else if (tool) key = 'waiting_tool'
      else key = 'streaming'
    } else {
      // 无应用内流状态时，用状态跟踪扩展的外部信号兜底（未安装则恒为 null）
      const sig = signalFor(sid)
      if (sig?.state === 'started') key = 'external_running'
      else if (sig?.state === 'blocked') key = 'waiting_permission'
      else if (sig?.state === 'failed') key = 'error'
    }
    return { key, ...getStatusMeta(key) }
  })
}
