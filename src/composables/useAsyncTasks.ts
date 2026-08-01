import type { SessionRecord, ContentBlock, AsyncMeta, SubAgentMeta } from '@/types'
import type { StreamingTurn } from './useStreaming'

// ============================================================================
// 异步任务账本 —— 从会话记录流推导所有异步任务的统一视图
// ----------------------------------------------------------------------------
// 数据模型（协议驱动，与具体工具解耦）：
//   启动 tool_use ──(tool_use_id)── 占位 tool_result(toolUseResult.async_launched)
//                 ──(tool-use-id / task-id)── 终态 <task-notification>
// 三方配对成一个 AsyncTaskItem。物种仅决定卡片增强（图标/详情视图），
// 不认识的工具只要走协议（async_launched 占位 + task-notification）就落 generic 兜底。
// 流式期间事件管道只有 assistant 侧事件：tool_use 实时入账为 running，
// 占位与终态等流式落账 reload 后补全。
// ============================================================================

export type AsyncSpecies = 'bash' | 'agent' | 'workflow' | 'monitor' | 'wakeup' | 'generic'

export type AsyncTaskState =
  | 'running'    // 已启动，未见终态
  | 'waiting'    // wakeup 专属：等待唤醒（scheduledFor 未来且是最晚排定的唤醒，单槽语义）
  | 'completed'
  | 'failed'     // 带 exitCode
  | 'killed'     // 会话中被主动停止
  | 'unknown'    // 孤儿：无终态通知且会话已不活跃（或 resume 补发的"结论不明"）

export interface AsyncTaskItem {
  /** 稳定 key：启动 tool_use id，孤儿条目用 task-id */
  key: string
  species: AsyncSpecies
  state: AsyncTaskState
  /** 主标题：description / workflowName / reason */
  title: string
  /** 副行：命令文本 / prompt 摘要 / 监视命令 */
  detail: string | null
  /** 定位跳转锚点（孤儿条目可能为空） */
  toolUseId: string | null
  /** 可下钻转录的子 agent id（agent 物种 / workflow child） */
  agentId: string | null
  /** workflow 目录名（subagents/workflows/<runId>/），子 agent 清单靠它关联 */
  runId: string | null
  /** 后台任务 ID（bash 为 b 前缀、workflow 为 w 前缀），终态通知按它配对 */
  taskId: string | null
  outputFile: string | null
  /** wakeup：精确触发时刻 epoch_ms（倒计时） */
  scheduledFor: number | null
  model: string | null
  exitCode: number | null
  startedAt: string | null
  endedAt: string | null
  usage: { tokens: number | null; toolUses: number | null; durationMs: number | null } | null
  /** 终态通知里的 result 全文（agent/workflow）——详情视图展示 */
  resultText: string | null
  /** monitor 的 event 快照（persistent 会有多条） */
  events: string[]
  /** workflow 子 agent（由磁盘扫描按 runId 关联，UI 层合并） */
  children: SubAgentMeta[]
}

// ---- task-notification XML 解析 ----

export interface ParsedNotification {
  taskId: string | null
  toolUseId: string | null
  outputFile: string | null
  status: string | null
  summary: string | null
  result: string | null
  event: string | null
  tokens: number | null
  toolUses: number | null
  durationMs: number | null
  exitCode: number | null
  /** resume 补发的"找不到完成记录"合成通知——结论不明，非真实终态 */
  orphanHint: boolean
}

function tag(text: string, name: string): string | null {
  const m = text.match(new RegExp(`<${name}>([\\s\\S]*?)</${name}>`))
  return m ? m[1].trim() : null
}

export function parseTaskNotification(text: string): ParsedNotification | null {
  if (!text.includes('<task-notification>')) return null
  const summary = tag(text, 'summary')
  const usageBlock = tag(text, 'usage') ?? ''
  const num = (name: string): number | null => {
    const v = tag(usageBlock, name)
    if (v === null) return null
    const n = Number(v)
    return Number.isFinite(n) ? n : null // "0" 是合法值（agent 秒败时 tokens 为 0），不折成 null
  }
  const exitMatch = summary?.match(/exit code (\d+)/)
  return {
    taskId: tag(text, 'task-id'),
    toolUseId: tag(text, 'tool-use-id'),
    outputFile: tag(text, 'output-file'),
    status: tag(text, 'status'),
    summary,
    result: tag(text, 'result'),
    event: tag(text, 'event'),
    tokens: num('subagent_tokens') ?? num('total_tokens'),
    toolUses: num('tool_uses'),
    durationMs: num('duration_ms'),
    exitCode: exitMatch ? Number(exitMatch[1]) : null,
    orphanHint: summary?.startsWith('No completion record') ?? false,
  }
}

// ---- 物种检测 ----

function detectSpecies(name: string, input: Record<string, unknown>): AsyncSpecies | null {
  switch (name) {
    case 'Bash':
      return input.run_in_background === true ? 'bash' : null
    case 'Agent':
    case 'Task': // 旧版 CLI 的子 agent 工具名
    case 'SendMessage': // 续聊 = 重新激活一个后台 agent
      return 'agent'
    case 'Workflow':
      return 'workflow'
    case 'Monitor':
      return 'monitor'
    case 'ScheduleWakeup':
      return 'wakeup'
    default:
      return null // 未知工具：靠占位回执的 async_launched 兜底落 generic
  }
}

function titleFromInput(species: AsyncSpecies, input: Record<string, unknown>): string {
  const s = (k: string) => (typeof input[k] === 'string' ? (input[k] as string) : '')
  switch (species) {
    case 'bash':
      return s('description') || s('command')
    case 'agent':
      return s('description') || s('summary') || s('to')
    case 'workflow':
      return s('name') || s('description')
    case 'monitor':
      return s('description')
    case 'wakeup':
      return s('reason')
    default:
      return ''
  }
}

function detailFromInput(species: AsyncSpecies, input: Record<string, unknown>): string | null {
  const s = (k: string) => (typeof input[k] === 'string' ? (input[k] as string) : null)
  switch (species) {
    case 'bash':
      return s('command')
    case 'agent':
      return s('prompt') ?? s('message')
    case 'monitor':
      return s('command')
    case 'wakeup':
      return s('prompt')
    default:
      return null
  }
}

// ---- 账本构建 ----

type ToolUseBlock = Extract<ContentBlock, { type: 'tool_use' }>

/** ContentBlock 的 catch-all 变体让 type 判别收窄不干净，显式守卫 */
function isToolUse(b: ContentBlock): b is ToolUseBlock {
  return b.type === 'tool_use'
    && typeof (b as Record<string, unknown>).id === 'string'
    && typeof (b as Record<string, unknown>).name === 'string'
}

interface LedgerEntry extends AsyncTaskItem {
  /** 终态是否已到（避免 running 覆盖） */
  settled: boolean
  /** 占位回执所在记录序号——wakeup 单槽语义"最晚排定"判据 */
  touchIdx: number
  /** Monitor：persistent 标记（非持续监视一次 event 即终态） */
  persistent: boolean | null
}

export interface LedgerLiveContext {
  /**
   * 自有长活进程的启动时刻 epoch ms（进程代际锚点）。启动早于此刻且无终态的
   * 任务属于已死的前代进程（关列重杀/渠道变更重启/resume 前的历史），终态通知
   * 永远不会来——判「结论不明」而非「进行中」，否则 processAlive 恒 true 的
   * 常开列下这些条目永远卡在进行中
   */
  ownProcessStartMs?: number | null
  /** 外部 claude 进程在跑（跟看）：外部进程可能正是历史任务的属主，代际判据失效，跳过判死 */
  externalRunning?: boolean
}

export function buildAsyncLedger(
  records: SessionRecord[],
  streamingTurns: StreamingTurn[],
  /**
   * 会话是否活跃：本会话流式中 / 自持长活进程存活（turn 结束后仍在跑后台任务）/
   * 外部 claude 进程正在跑（跟看）——决定无终态条目算 running 还是 unknown
   */
  live: boolean,
  ctx?: LedgerLiveContext,
): AsyncTaskItem[] {
  const byToolUse = new Map<string, LedgerEntry>()
  const order: LedgerEntry[] = []

  function addEntry(e: LedgerEntry) {
    order.push(e)
    if (e.toolUseId) byToolUse.set(e.toolUseId, e)
  }

  function newEntry(partial: Partial<LedgerEntry> & { key: string; species: AsyncSpecies }): LedgerEntry {
    return {
      state: 'running',
      title: '',
      detail: null,
      toolUseId: null,
      agentId: null,
      runId: null,
      taskId: null,
      outputFile: null,
      scheduledFor: null,
      model: null,
      exitCode: null,
      startedAt: null,
      endedAt: null,
      usage: null,
      resultText: null,
      events: [],
      children: [],
      settled: false,
      touchIdx: -1,
      persistent: null,
      ...partial,
    }
  }

  function ingestToolUse(block: ToolUseBlock, timestamp: string | null) {
    if (byToolUse.has(block.id)) return
    const input = block.input ?? {}
    const species = detectSpecies(block.name, input)
    if (!species) return
    addEntry(newEntry({
      key: block.id,
      species,
      toolUseId: block.id,
      title: titleFromInput(species, input),
      detail: detailFromInput(species, input),
      model: typeof input.model === 'string' ? input.model : null,
      startedAt: timestamp,
    }))
  }

  function ingestAsyncMeta(entry: LedgerEntry | undefined, meta: AsyncMeta, toolUseId: string, timestamp: string | null, idx: number) {
    let e = entry
    if (!e) {
      // 未知工具的占位回执（协议兜底）：tool_use 没被物种检测收录，靠 async_launched 补建 generic
      if (meta.status !== 'async_launched') return
      e = newEntry({
        key: toolUseId,
        species: 'generic',
        toolUseId,
        title: meta.description ?? meta.workflow_name ?? meta.summary ?? '',
        startedAt: timestamp,
      })
      addEntry(e)
    }
    e.touchIdx = idx
    e.agentId = meta.agent_id ?? meta.resumed_agent_id ?? e.agentId
    e.taskId = meta.task_id ?? meta.background_task_id ?? e.taskId
    e.runId = meta.run_id ?? e.runId
    e.outputFile = meta.output_file ?? e.outputFile
    e.scheduledFor = meta.scheduled_for ?? e.scheduledFor
    e.model = meta.resolved_model ?? e.model
    e.persistent = meta.persistent ?? e.persistent
    if (!e.title) e.title = meta.description ?? meta.workflow_name ?? meta.summary ?? ''
    if (e.species === 'workflow' && meta.summary) e.detail = e.detail ?? meta.summary
    // 前台 Agent：阻塞到完成才落 result，status=completed 即终态
    if (meta.status === 'completed' && !e.settled) {
      e.state = 'completed'
      e.endedAt = timestamp
      e.settled = true
    }
  }

  function ingestNotification(n: ParsedNotification, timestamp: string | null) {
    // 配对：tool-use-id 直查 → task-id 兜底（Monitor 无 tool-use-id）
    let e = n.toolUseId ? byToolUse.get(n.toolUseId) : undefined
    if (!e && n.taskId) {
      e = order.find(x => x.taskId === n.taskId || x.agentId === n.taskId)
    }
    if (!e) {
      // 孤儿通知（启动记录在上个会话/上下文压缩丢失）：单独成条，物种按 summary 措辞猜
      const s = n.summary ?? ''
      const species: AsyncSpecies = s.startsWith('Background command') ? 'bash'
        : s.startsWith('Agent') ? 'agent'
        : s.startsWith('Dynamic workflow') ? 'workflow'
        : s.startsWith('Monitor event') ? 'monitor'
        : 'generic'
      e = newEntry({
        key: n.taskId ?? `orphan-${order.length}`,
        species,
        toolUseId: n.toolUseId,
        taskId: n.taskId,
        title: s.match(/"([^"]+)"/)?.[1] ?? s,
        // 通知自身时刻兜底 startedAt：status=running 的中途进度孤儿（启动记录丢失）
        // 若不带时刻会豁免代际判死，在常开进程下重现永久"进行中"（审查确认的残留实例）
        startedAt: timestamp,
      })
      addEntry(e)
    }
    e.outputFile = n.outputFile ?? e.outputFile
    if (n.event !== null) {
      // Monitor 事件流：非持续监视命中条件即终态（实测无独立终态通知，一次 event 就是结局）；
      // persistent 监视持续追加，仅超时事件终结
      e.events.push(n.event)
      e.endedAt = timestamp
      if (!e.settled && (e.persistent !== true || n.event.includes('Monitor timed out'))) {
        e.state = 'completed'
        e.settled = true
      }
      return
    }
    if (n.status === 'running') return // 中途进度补发，不改状态
    e.settled = true
    e.endedAt = timestamp
    e.exitCode = n.exitCode
    e.resultText = n.result ?? e.resultText
    if (n.tokens !== null || n.toolUses !== null || n.durationMs !== null) {
      e.usage = { tokens: n.tokens, toolUses: n.toolUses, durationMs: n.durationMs }
    }
    e.state = n.orphanHint ? 'unknown'
      : n.status === 'completed' ? 'completed'
      : n.status === 'failed' ? 'failed'
      : n.status === 'killed' ? 'killed'
      : 'unknown' // stopped 及未知 status：结论不明
  }

  // ---- 主扫描 ----
  records.forEach((record, idx) => {
    if (record.type === 'task_notification') {
      const n = parseTaskNotification(record.content)
      if (n) ingestNotification(n, record.timestamp)
    } else if (record.type === 'assistant') {
      const blocks = record.message?.content ?? []
      for (const b of blocks) {
        if (isToolUse(b)) ingestToolUse(b, record.timestamp)
      }
    } else if (record.type === 'user') {
      const content = record.message?.content
      if (Array.isArray(content)) {
        // toolUseResult 是记录级字段：仅单 tool_result 记录可安全归属
        const results = content.filter(b => b.type === 'tool_result')
        if (results.length !== 1) return
        const r = results[0] as Extract<ContentBlock, { type: 'tool_result' }>
        if (record.async_meta) {
          ingestAsyncMeta(byToolUse.get(r.tool_use_id), record.async_meta, r.tool_use_id, record.timestamp, idx)
        } else if (r.is_error) {
          // 启动即失败（参数校验失败/用户拒绝/中断）：无占位回执，按 tool_use_id 直接退账，
          // 否则条目永挂 running（流式）/ unknown（档案）
          const e = byToolUse.get(r.tool_use_id)
          if (e && !e.settled) {
            e.state = 'failed'
            e.settled = true
            e.endedAt = record.timestamp
          }
        }
      }
    }
  })

  // 流式增量：落账前的 tool_use 先入账为 running（占位/终态等 reload 补全）
  for (const turn of streamingTurns) {
    for (const b of turn.content) {
      if (isToolUse(b)) ingestToolUse(b, null)
    }
  }

  // ---- 状态收尾 ----
  const now = Date.now()
  // 进程代际判死：live 由自有进程撑起（非外部跟看）时，启动早于本代进程的
  // 无终态条目属于已死的前代进程，终态通知永远不会来。startedAt 为 null 仅剩
  // 流式增量入账路径（必属当代，不判）；record 落账来源的条目一律带时刻
  const staleBefore = !ctx?.externalRunning ? ctx?.ownProcessStartMs ?? null : null
  const isStale = (e: LedgerEntry): boolean => {
    if (staleBefore === null || e.startedAt === null) return false
    const t = Date.parse(e.startedAt)
    return Number.isFinite(t) && t < staleBefore
  }
  // wakeup 单槽语义：后排定的覆盖前排的，只有最晚一次可能仍在等。
  // 中途活动不撤销唤醒（实测通知唤起会话后，未到时刻的唤醒仍按时触发）
  let latestWakeupIdx = -1
  for (const e of order) {
    if (e.species === 'wakeup' && e.touchIdx > latestWakeupIdx) latestWakeupIdx = e.touchIdx
  }
  for (const e of order) {
    if (e.settled) continue
    if (e.species === 'wakeup') {
      // 等待中 = 会话仍活 且 属当代进程 且 触发时刻在未来 且 是最晚排定的唤醒
      // （唤醒是 CLI 进程内概念）；流式中刚设的（占位未落账）也算等待。
      // 未来唤醒但无活态信号/进程已换代 → 结论不明：交互式 REPL 等检测不到的
      // 外部进程可能仍会按时触发它，断言"已结束"是误导。
      // 已过时刻/被更晚唤醒覆盖 → 已结束（触发/落空不可分辨）
      const pendingFuture = e.scheduledFor !== null && e.scheduledFor > now && e.touchIdx === latestWakeupIdx
      const justSet = live && e.scheduledFor === null
      e.state = pendingFuture
        ? (live && !isStale(e) ? 'waiting' : 'unknown')
        : justSet ? 'waiting' : 'completed'
      continue
    }
    // 无终态通知：会话活跃（自有流式/外部进程在跑）且属当代进程视为在跑；
    // 否则结论不明（孤儿运行态）
    if (!live || isStale(e)) e.state = 'unknown'
  }

  // 内部字段不外泄
  return order.map(({ settled: _s, touchIdx: _t, persistent: _p, ...item }) => item)
}

/** 进行中（含等待唤醒）——面板置顶区与徽章高亮判据 */
export function isActive(item: AsyncTaskItem): boolean {
  return item.state === 'running' || item.state === 'waiting'
}

/** CLI stop_task 使用的稳定 ID；已结束或尚未拿到 ID 的条目不可终止。 */
export function asyncTaskStopId(item: AsyncTaskItem): string | null {
  if (!isActive(item)) return null
  return item.taskId ?? item.agentId
}
