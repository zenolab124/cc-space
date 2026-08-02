import { reactive, computed, ref, type Ref, type ComputedRef } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { hasReportedUsage, shouldReplaceUsage, type ContentBlock } from '@/types'
import { triggerMetaGeneration } from './useSessionMeta'
import { collectSessionCapabilities } from '@/features'
import type { EffortSetting } from './useSessionSettings'
import { frameWatchRetain, frameWatchRelease, probeFinishFlip } from '@/utils/perfProbe'
import { useConfirm } from './useConfirm'
import i18n from '../locales'
import {
  resolveStreamRenderPriority,
  smoothTakeForElapsed,
  streamRenderInterval,
} from '@/lib/streamRenderPolicy'

export interface SendOptions {
  model?: string
  /** null/缺省 = 跟随 CLI(不附加 --effort);'ultracode' 经 --settings 注入 */
  effort?: EffortSetting
  /**
   * 已解析的最终渠道 id(resolveChannel 产物):null/缺省 = 零注入走官方。
   * 解析值进 lastSent 一起缓存——重试必须复用发送时的渠道,不受期间默认值变化影响
   */
  channel?: string | null
  /** 顾问模式:true 时经 --settings 注入 advisorModel + env flag(主模型由调用方强制为 sonnet) */
  advisor?: boolean
  /** Chrome 集成:true 时 spawn 带 --chrome(启动参数,开关变更触发进程重启) */
  chrome?: boolean
  /**
   * 分叉意图的源会话 id:自有 jsonl 未落盘时 Rust 端以
   * --resume 源 --fork-session --session-id 本会话 spawn(CLI 原生分叉);
   * 文件已存在则被忽略,残留无害
   */
  forkSource?: string
  /** 自定义 CLI 参数原始串(逃生舱,变更触发进程重启;协议级参数 Rust 端黑名单剔除) */
  extraArgs?: string
  /** Anthropic image content blocks(base64 编码) */
  images?: Array<{ type: 'image'; source: { type: 'base64'; media_type: string; data: string } }>
  permissionMode?: string
  /** 跳过断链校验，强制以新会话语义打开（用户确认"仍新建"后重发时置 true） */
  forceNew?: boolean
}

export interface StreamingTurn {
  messageId: string
  content: ContentBlock[]
  /** 本轮实际运行的模型真值(流式期实时标注,与落账后的 message.model 同源) */
  model?: string
  /**
   * 该 turn 是否仍在传输中。与会话级 streaming 解耦:CLI 自发轮(task-notification
   * 唤醒)在 streaming=false 下到达,靠它走 plain 降级渲染管线(否则每帧全文 shiki
   * 重解析)且在用户直发时豁免清场。completeFinish / 落账摘除时结束。
   */
  live?: boolean
  /** 该 API message 的 token 用量(assistant 快照自带,message 完成即到)——
   *  块级 usage 即时显示的数据源,与落账后 record 的 message.usage 同源同值 */
  usage?: Record<string, number>
  stopReason?: string
}
export interface TailLine {
  kind: 'text' | 'tool' | 'error'
  text: string
}

/**
 * 单会话流式状态（v2.1.0 per-session 化：多会话并行流式互不阻塞）。
 * tail/activeTool 供工作台左列监控卡消费——监控卡只订阅这两个轻量字段，
 * 不 mount 消息流组件树（NFR-001 渲染分级的结构性前提）。
 */
export interface PendingQueueItem {
  message: string
  opts: SendOptions
}

export interface SessionStreamState {
  streaming: boolean
  streamingTurns: StreamingTurn[]
  pendingUserMessage: string | null
  /** 发送时附带的图片（流式待发/发送中回显用） */
  pendingImages: SendOptions['images'] | null
  /** 发送时刻(ms)：pending 落账匹配只认此后的记录，纯图片消息也靠它锚定 */
  pendingSentAt: number | null
  /** BTW 预输入队列：流式中用户发的消息暂存于此，前一轮 result 落账后再逐条发送 */
  pendingQueue: PendingQueueItem[]
  streamError: string | null
  /** 本次流式开始时刻（监控卡持续时间显示） */
  startedAt: number | null
  /** 进行中的工具名（block_stop(tool_use) 后等待执行；「等待工具/Workflow」状态判定） */
  activeTool: string | null
  /** 尾部行（150ms 合并刷新） */
  tail: TailLine[]
  /** 最近一次发送的消息（出错重试用） */
  lastSent: { cwd: string; message: string; opts: SendOptions } | null
  /** API 报告的真实上下文容量（result 事件 modelUsage.contextWindow） */
  realContextWindow: number | null
  /** API 报告的已用 input token 数（result 事件 modelUsage.inputTokens） */
  realUsedTokens: number | null
  /** API 报告的 output token 数（result 事件 modelUsage.outputTokens） */
  realOutputTokens: number | null
  /** Remote Control 是否已启用（open_session 自动启用，手动可关） */
  rcActive: boolean
  /**
   * Monet 自持长活进程是否存活。与 streaming 是两回事：turn 结束（result 到达）
   * 后进程继续活着跑后台任务（Workflow/子 agent/后台 bash）。异步账本的 live 判据
   * 靠它避免把「turn 已结束但后台任务在跑」的条目误判为 unknown。
   * 写入源：stream-event 到达置 true（进程在说话必然活着）、
   * session-process-exited 置 false、syncProcessAlive 按 Rust 进程表校准。
   */
  processAlive: boolean
  /**
   * 自有长活进程的启动时刻 epoch ms（进程代际锚点）。异步账本据此把
   * 「启动于前代进程、无终态通知」的任务判为结论不明而非进行中——前代
   * 进程已死，它名下任务的终态通知永远不会来。写入源：sendMessage 冷启动
   * 时刻、processAlive false→true 边沿、syncProcessAlive 的 Rust 真值校准。
   */
  processStartedAtMs: number | null
}

interface BlockDeltaPayload {
  type: 'text_delta' | 'input_json_delta' | 'thinking_delta' | 'signature_delta' | string
  text?: string
  partial_json?: string
  thinking?: string
  signature?: string
}

interface StreamEventPayload {
  kind: 'assistant_message' | 'block_start' | 'block_delta' | 'block_stop' | 'result' | 'error'
  session_id: string
  message_id?: string
  content?: ContentBlock[]
  /** 本轮实际运行的模型真值(message_start / assistant 快照的 message.model) */
  model?: string
  /** assistant 快照携带的该 message token 用量 */
  usage?: Record<string, number>
  stop_reason?: string
  // block_start / block_delta / block_stop
  index?: number
  content_block?: ContentBlock
  delta?: BlockDeltaPayload
  // result
  text?: string
  cost_usd?: number
  context_window?: number
  input_tokens?: number
  output_tokens?: number
  // error
  message?: string
}

// ---- per-session 状态表 ----

const streams = reactive(new Map<string, SessionStreamState>())

function createState(): SessionStreamState {
  return {
    streaming: false,
    streamingTurns: [],
    pendingUserMessage: null,
    pendingImages: null,
    pendingSentAt: null,
    pendingQueue: [],
    streamError: null,
    startedAt: null,
    activeTool: null,
    tail: [],
    lastSent: null,
    realContextWindow: null,
    realUsedTokens: null,
    realOutputTokens: null,
    rcActive: false,
    processAlive: false,
    processStartedAtMs: null,
  }
}

/** 取（或建）某会话的流式状态 */
export function getStream(sessionId: string): SessionStreamState {
  let s = streams.get(sessionId)
  if (!s) {
    // set 后必须重新 get：reactive Map 的 get 才返回深层 reactive 代理,
    // 直接返回原始对象会让后续 mutate 绕过响应式
    streams.set(sessionId, createState())
    s = streams.get(sessionId)!
  }
  return s
}

const EMPTY_STATE: SessionStreamState = createState()

/** 组件侧：按响应式 sessionId 取当前会话的流式状态（无状态时返回冻结空态） */
export function useSessionStream(
  sessionId: Ref<string | null> | ComputedRef<string | null>,
) {
  return computed<SessionStreamState>(() => {
    const sid = sessionId.value
    if (!sid) return EMPTY_STATE
    return streams.get(sid) ?? EMPTY_STATE
  })
}

/** 无副作用查询某会话是否在流式（getStream 会顺手建条目，只读判断用这个） */
export function isSessionStreaming(sessionId: string): boolean {
  return streams.get(sessionId)?.streaming ?? false
}

/** 是否有任一会话正在流式（全局视图用） */
export function anyStreaming(): boolean {
  for (const s of streams.values()) {
    if (s.streaming) return true
  }
  return false
}

// 进程代际计数：exited 事件 bump。invoke 与 event 走不同 IPC 通道无序保证，
// 校准的 true 结果若晚于 exited 事件到达，靠代际比对丢弃，防止活态卡 true
const processEpoch = new Map<string, number>()

/** 按 Rust 进程表校准某会话的 processAlive——webview 刷新后前端状态丢失而长活进程还在，加载会话时调用 */
export async function syncProcessAlive(sessionId: string): Promise<void> {
  const epoch = processEpoch.get(sessionId) ?? 0
  try {
    // Rust 返回进程启动时刻 epoch ms（null = 无进程）——顺带校准进程代际锚点
    const startedAtMs = await invoke<number | null>('has_own_process', { sessionId })
    const alive = startedAtMs !== null
    // 降级（false）永远安全；升级（true）仅在 invoke 期间无进程退出时可信。
    // false 晚到覆盖新进程 true 的反向场景由下一条 stream-event 立即纠正
    if (!alive || (processEpoch.get(sessionId) ?? 0) === epoch) {
      const s = getStream(sessionId)
      s.processAlive = alive
      if (alive) s.processStartedAtMs = startedAtMs
    }
  } catch {
    // 校准失败保持现值（事件流仍会驱动更新）
  }
}

// ---- 帧级批 flush（全局单队列，key=messageId#index 天然多流安全） ----

// 累积每个 (messageId, index) 上 tool_use 的 partial_json 字符串,
// block_stop 时 JSON.parse 一次性写入 block.input
const toolPartialJson = new Map<string, string>()
const partialJsonKey = (messageId: string, index: number) => `${messageId}#${index}`

// messageId → { turn, sessionId }：flush 时 O(1) 反查（跨会话）
const turnIndex = new Map<string, { turn: StreamingTurn; sessionId: string }>()
// thinking 块计时：block_start 记时刻，block_stop 算差值
const thinkingStartTimes = new Map<string, number>()

/**
 * 已落账 message 的墓碑:settle 摘除 turn 后,同 mid 的迟到事件不得重建 turn。
 * 实测根因(2026-07-16,AVIF 会话取证):--thinking-display summarized 会对同一段
 * 思考产生多个摘要版本,CLI 落盘其一,但把另一版本也透传进事件流,且到达时机晚于
 * settle——前端为它重建 turn 就呈现为「回答完毕后底下突然多出一个思考块,发下一条
 * 消息即消失(清场)」。落账 = 内容已完整在历史区,同 mid 重建必为重复,误杀面为零。
 * FIFO 容量 50,跨轮自然淘汰。
 */
const landedTombstones = new Set<string>()
function tombstone(messageId: string) {
  landedTombstones.add(messageId)
  if (landedTombstones.size > 50) {
    const oldest = landedTombstones.values().next().value!
    landedTombstones.delete(oldest)
  }
}

/** 非流式期新建 turn 的取证快照(自发轮正常路径,也是未知幽灵的唯一入口)——
 *  留最近 20 条到 localStorage,幽灵再现时直接锁死事件来源 */
function ghostAudit(kind: string, mid: string | undefined, block: string | null, sid: string) {
  try {
    const prev: unknown[] = JSON.parse(localStorage.getItem('monet-ghost-audit') ?? '[]')
    prev.push({ at: new Date().toISOString(), kind, mid, block, sid: sid.slice(0, 8) })
    localStorage.setItem('monet-ghost-audit', JSON.stringify(prev.slice(-20)))
  } catch { /* 满/禁用则放弃 */ }
}

/** 丢弃某 turn 的全部传输态(索引 + 残余 pending):防死 key 让节奏档位虚高 */
function dropTurnTransients(messageId: string) {
  turnIndex.delete(messageId)
  const prefix = `${messageId}#`
  for (const key of pendingTextDeltas.keys()) {
    if (key.startsWith(prefix)) pendingTextDeltas.delete(key)
  }
  for (const key of toolPartialJson.keys()) {
    if (key.startsWith(prefix)) toolPartialJson.delete(key)
  }
  for (const key of thinkingStartTimes.keys()) {
    if (key.startsWith(prefix)) thinkingStartTimes.delete(key)
  }
}

// RAF 批 flush:字符级 delta 高频到达,直接 mutate 会让 BlockText 的 markdown-it+shiki
// 重渲染 100+ 次/秒,导致卡顿且滚动跟随 watcher 无法识别字符级变化。
// 策略:text/thinking/signature delta 先累到 pendingTextDeltas,每帧合并 flush 一次到 block。
// streamingTick 每帧递增一次,SessionDetail watch 它即可统一处理滚动跟随。
// 渲染开销天然按订阅分级:无挂载组件的会话(仅左列/非激活 tab)更新 block 无人订阅,零渲染成本。
interface PendingTextDelta {
  text?: string
  thinking?: string
  signature?: string
  /** 该 key 本轮缓冲的首数据时刻:预热期(SMOOTH_WARMUP_MS)内攒着不吐 */
  bornAt?: number
}
const pendingTextDeltas = new Map<string, PendingTextDelta>()

// 一个会话可能同时存在于工作台与档案馆(v-show 常驻)，按渲染 surface 汇总可见性。
// 调度只关心“至少一个 surface 可见”，避免隐藏视图最后一次 IO 回调覆盖可见视图。
const streamRenderSurfaces = new Map<string, Map<symbol, boolean>>()
let activeRenderSessionId: string | null = null

export function setSessionRenderSurface(
  sessionId: string,
  surfaceId: symbol,
  visible: boolean,
): void {
  let surfaces = streamRenderSurfaces.get(sessionId)
  if (!surfaces) {
    surfaces = new Map()
    streamRenderSurfaces.set(sessionId, surfaces)
  }
  surfaces.set(surfaceId, visible)
  bump(true)
}

export function removeSessionRenderSurface(sessionId: string, surfaceId: symbol): void {
  const surfaces = streamRenderSurfaces.get(sessionId)
  if (!surfaces) return
  surfaces.delete(surfaceId)
  if (surfaces.size === 0) {
    streamRenderSurfaces.delete(sessionId)
    if (activeRenderSessionId === sessionId) activeRenderSessionId = null
  }
  bump(true)
}

export function markSessionRenderActive(sessionId: string): void {
  if (activeRenderSessionId === sessionId) return
  activeRenderSessionId = sessionId
  bump(true)
}

function isSessionSurfaceVisible(sessionId: string): boolean {
  const surfaces = streamRenderSurfaces.get(sessionId)
  if (!surfaces) return false
  for (const visible of surfaces.values()) {
    if (visible) return true
  }
  return false
}

export function sessionRenderCadence(sessionId: string): number {
  const visible = isSessionSurfaceVisible(sessionId)
  const priority = resolveStreamRenderPriority(
    visible,
    visible && activeRenderSessionId === sessionId,
  )
  return streamRenderInterval(priority, document.visibilityState === 'visible')
}

/** 取(或建)pending 条目,新建时盖 bornAt 预热时间戳 */
function pendingEntryOf(key: string): PendingTextDelta {
  let e = pendingTextDeltas.get(key)
  if (!e) {
    e = { bornAt: performance.now() }
    pendingTextDeltas.set(key, e)
  }
  return e
}
export const streamingTick = ref(0)
let rafId: number | null = null
let scheduledFlushAt = Number.POSITIVE_INFINITY
const lastSessionFlushAt = new Map<string, number>()

/**
 * 平滑窗口:任意时刻缓冲里的字符会在约该时长内播完。稳态下显示速率 = API
 * 到达速率,延迟上界 ≈ 本值——区别于已退役的打字机(固定人造速率、积压无界):
 * 这是到达率自适应整形,只把真流式 delta「几字一蹦」的脉冲摊成连续流,
 * API 快则播快、API 停则 ~300ms 放干,不改变任何时序语义。
 */
const SMOOTH_WINDOW_MS = 300
/** 泄洪阈值:超大缓冲(快照兜底对账等)扫屏无意义,直接瞬吐 */
const FLOOD_CHARS = 20000
/**
 * 冷启动预热(jitter buffer):块的首批数据攒够该时长再开始吐。
 * API 首 delta 常单独早到且只有一两个字,无预热时它瞬间播完、缓冲放干,
 * 屏幕停顿等主流到达——"首字弹出→顿→再平滑"的突刺。预热让首字与主流
 * 连成一条流起步,代价是块首延迟 +200ms(秒级 TTFT 面前无感)。
 * 缓冲中途放干后重来会再次预热:此时已在真实生成停顿(>300ms)之后,
 * 多 200ms 无感,且让恢复段同样平滑起步。
 */
const SMOOTH_WARMUP_MS = 200

/**
 * 帧级批 flush。
 * @param onlySession  仅 flush 指定会话的 pending(收尾时不影响其他在播会话)
 * @param instant      true:全量瞬吐(窗口隐藏 / 落账前收尾);false:平滑排量
 * @returns 是否还有未消化字符
 */
function flushTextDeltas(onlySession?: string, instant = false, elapsedMs = 16): boolean {
  if (pendingTextDeltas.size === 0) return false
  // HUD 长帧归因埋点:流式期主线程的最大嫌疑段
  const perfT0 = performance.now()
  try {
    return flushTextDeltasInner(onlySession, instant, elapsedMs)
  } finally {
    performance.measure('stream-flush', { start: perfT0, duration: performance.now() - perfT0 })
  }
}

/** 比例排量:每帧吐「缓冲 × 帧时长 ÷ 平滑窗口」,大块指数衰减追平,尾部至少 1 字防拖尾 */
function smoothTake(bufferLen: number, elapsedMs: number): number {
  if (bufferLen > FLOOD_CHARS) return Infinity
  return smoothTakeForElapsed(bufferLen, elapsedMs, SMOOTH_WINDOW_MS)
}

function flushTextDeltasInner(onlySession?: string, instant = false, elapsedMs = 16): boolean {
  let hasRemaining = false
  pendingTextDeltas.forEach((p, key) => {
    const hashIdx = key.indexOf('#')
    if (hashIdx < 0) return
    const mid = key.slice(0, hashIdx)
    const index = parseInt(key.slice(hashIdx + 1), 10)
    const entry = turnIndex.get(mid)
    if (!entry) return
    if (onlySession && entry.sessionId !== onlySession) {
      return
    }
    const block = entry.turn.content[index] as ContentBlock | undefined
    if (!block) return

    // 预热期:攒着不吐(instant 路径豁免)。signature 只在 thinking 尾部到达,
    // 彼时 bornAt 早已超期,实际不受影响
    if (!instant && p.bornAt !== undefined && performance.now() - p.bornAt < SMOOTH_WARMUP_MS) {
      hasRemaining = true
      return
    }

    if (p.thinking !== undefined && block.type === 'thinking') {
      const take = instant ? Infinity : smoothTake(p.thinking.length, elapsedMs)
      ;(block as { thinking: string }).thinking += p.thinking.slice(0, take)
      const rem = p.thinking.slice(take)
      if (rem) {
        p.thinking = rem
        hasRemaining = true
      } else {
        p.thinking = undefined
      }
    }
    if (p.signature !== undefined && block.type === 'thinking') {
      ;(block as Record<string, unknown>).signature = p.signature
      p.signature = undefined
    }
    if (p.text !== undefined && block.type === 'text') {
      const take = instant ? Infinity : smoothTake(p.text.length, elapsedMs)
      ;(block as { text: string }).text += p.text.slice(0, take)
      const rem = p.text.slice(take)
      if (rem) {
        p.text = rem
        hasRemaining = true
      } else {
        p.text = undefined
      }
    }
    if (p.text === undefined && p.thinking === undefined && p.signature === undefined) {
      pendingTextDeltas.delete(key)
    }
  })
  return hasRemaining
}

/** 正在排空批处理残余的会话(CLI 已 done,等 pending 落净再翻 streaming=false,最多一帧) */
const drainingStreams = new Set<string>()

function hasSessionPending(sessionId: string): boolean {
  for (const [key] of pendingTextDeltas) {
    const mid = key.slice(0, key.indexOf('#'))
    const entry = turnIndex.get(mid)
    if (entry?.sessionId === sessionId) return true
  }
  return false
}

function pendingSessionIds(): Set<string> {
  const ids = new Set<string>()
  for (const key of pendingTextDeltas.keys()) {
    const hashIdx = key.indexOf('#')
    if (hashIdx < 0) continue
    const entry = turnIndex.get(key.slice(0, hashIdx))
    if (entry) ids.add(entry.sessionId)
  }
  return ids
}

function nextFlushDelay(now: number): number | null {
  let delay = Number.POSITIVE_INFINITY
  for (const sid of pendingSessionIds()) {
    const interval = sessionRenderCadence(sid)
    const last = lastSessionFlushAt.get(sid)
    delay = Math.min(delay, last === undefined ? interval : Math.max(0, last + interval - now))
  }
  if (drainingStreams.size > 0 && !Number.isFinite(delay)) return 0
  return Number.isFinite(delay) ? delay : null
}

function bump(forceReschedule = false) {
  const now = performance.now()
  const delay = nextFlushDelay(now)
  if (delay === null) return
  const nextAt = now + delay
  if (rafId !== null && !forceReschedule && scheduledFlushAt <= nextAt + 1) return
  if (rafId !== null) window.clearTimeout(rafId)
  scheduledFlushAt = nextAt
  rafId = window.setTimeout(() => {
    rafId = null
    scheduledFlushAt = Number.POSITIVE_INFINITY
    const flushAt = performance.now()
    const documentVisible = document.visibilityState === 'visible'
    let flushed = false
    for (const sid of pendingSessionIds()) {
      const interval = sessionRenderCadence(sid)
      const last = lastSessionFlushAt.get(sid)
      if (last !== undefined && flushAt - last + 1 < interval) continue
      const elapsed = last === undefined ? interval : Math.max(interval, flushAt - last)
      lastSessionFlushAt.set(sid, flushAt)
      // 整个窗口隐藏时无人观看，平滑无意义，直接落地省 CPU。
      flushTextDeltas(sid, !documentVisible, elapsed)
      flushed = true
    }
    if (flushed) streamingTick.value++
    // 检查排空中的会话:pending 落净 → 完成收尾
    if (drainingStreams.size > 0) {
      for (const sid of [...drainingStreams]) {
        if (!hasSessionPending(sid)) {
          drainingStreams.delete(sid)
          completeFinish(sid)
        }
      }
    }
    if (pendingTextDeltas.size > 0 || drainingStreams.size > 0) bump()
  }, delay)
}

// ---- 尾部状态聚合（监控卡消费,150ms 合并;FR-003） ----

/** 各会话的原始文本尾巴累积（不走帧级批处理,收到即累,150ms 刷成 tail 行） */
const tailTextAcc = new Map<string, string>()
const tailDirty = new Set<string>()
let tailTimer: number | null = null
const TAIL_FLUSH_MS = 150
const TAIL_KEEP_CHARS = 400

function markTailDirty(sessionId: string) {
  tailDirty.add(sessionId)
  if (tailTimer !== null) return
  tailTimer = window.setTimeout(() => {
    tailTimer = null
    tailDirty.forEach(sid => rebuildTail(sid))
    tailDirty.clear()
  }, TAIL_FLUSH_MS)
}

/** 重建某会话的 tail 行：最近工具行（若有）在上 + 文本末 2 行 */
function rebuildTail(sessionId: string) {
  const state = streams.get(sessionId)
  if (!state) return
  const lines: TailLine[] = []
  if (state.activeTool) {
    lines.push({ kind: 'tool', text: state.activeTool })
  }
  const acc = tailTextAcc.get(sessionId) ?? ''
  const textLines = acc.split('\n').map(l => l.trim()).filter(Boolean)
  for (const l of textLines.slice(-2)) {
    lines.push({ kind: 'text', text: l.length > 60 ? l.slice(-60) : l })
  }
  if (state.streamError) {
    lines.push({ kind: 'error', text: state.streamError.slice(0, 80) })
  }
  state.tail = lines.slice(-3)
}

function accumulateTailText(sessionId: string, text: string) {
  const prev = tailTextAcc.get(sessionId) ?? ''
  const next = (prev + text).slice(-TAIL_KEEP_CHARS)
  tailTextAcc.set(sessionId, next)
  markTailDirty(sessionId)
}

/** 从工具 input 提取摘要目标（file_path/command/url/pattern 首个字符串） */
function toolTarget(input: Record<string, unknown> | undefined): string | null {
  if (!input) return null
  for (const k of ['file_path', 'command', 'url', 'pattern', 'skill', 'prompt']) {
    const v = input[k]
    if (typeof v === 'string' && v) {
      const s = v.split('\n')[0]
      return s.length > 36 ? `${s.slice(0, 36)}…` : s
    }
  }
  return null
}

/**
 * 冲掉指定消息中 < beforeIndex 的所有未播文字,让它们立刻显示完整。
 * 场景:新块到达时,前面还在打字的块不该阻塞后续内容。
 */
function flushPendingBefore(messageId: string, beforeIndex: number) {
  const prefix = `${messageId}#`
  for (const [key, p] of pendingTextDeltas) {
    if (!key.startsWith(prefix)) continue
    const idx = parseInt(key.slice(prefix.length), 10)
    if (idx >= beforeIndex) continue
    const entry = turnIndex.get(messageId)
    if (!entry) continue
    const block = entry.turn.content[idx]
    if (!block) continue
    if (p.text !== undefined && block.type === 'text') {
      ;(block as { text: string }).text += p.text
      p.text = undefined
    }
    if (p.thinking !== undefined && block.type === 'thinking') {
      ;(block as { thinking: string }).thinking += p.thinking
      p.thinking = undefined
    }
    if (p.text === undefined && p.thinking === undefined && p.signature === undefined) {
      pendingTextDeltas.delete(key)
    }
  }
}

/**
 * 从 assistant 快照提取增量(快照可能多次到达:空→部分→完整,按已知长度差量对账)。
 * text / thinking → 喂入帧级批处理队列。thinking 曾直接设值,导致多次快照
 * 之间整块跳变(CLI 2.1.177+ 无 delta,快照就是唯一颗粒),现与 text 同款对账渐现。
 */
function feedSnapshotText(
  messageId: string,
  index: number,
  incoming: ContentBlock,
  stripped: Record<string, unknown>,
  sessionId: string,
) {
  if (incoming.type === 'text') {
    const key = partialJsonKey(messageId, index)
    const incText = (incoming as { text?: string }).text ?? ''
    if (!incText) return
    const curText = typeof stripped.text === 'string' ? stripped.text : ''
    const pendingLen = pendingTextDeltas.get(key)?.text?.length ?? 0
    const alreadyKnown = curText.length + pendingLen
    if (incText.length > alreadyKnown) {
      const delta = incText.slice(alreadyKnown)
      const e = pendingEntryOf(key)
      e.text = (e.text ?? '') + delta
      accumulateTailText(sessionId, delta)
    }
    stripped.text = curText
  } else if (incoming.type === 'thinking') {
    // 与 text 同款差量对账:阻塞后续块的问题由 flushPendingBefore(跨帧新块冲刷)
    // 与 instantKeys(同帧多块瞬吐)兜住,思考残量不会挡正文
    const key = partialJsonKey(messageId, index)
    const incThink = (incoming as { thinking?: string }).thinking ?? ''
    if (!incThink) return
    const curThink = typeof stripped.thinking === 'string' ? stripped.thinking : ''
    const pendingLen = pendingTextDeltas.get(key)?.thinking?.length ?? 0
    const alreadyKnown = curThink.length + pendingLen
    if (incThink.length > alreadyKnown) {
      const delta = incThink.slice(alreadyKnown)
      const e = pendingEntryOf(key)
      e.thinking = (e.thinking ?? '') + delta
    }
    stripped.thinking = curThink
  }
}

/**
 * 快照块能否合并进某个流式块:类型相同之外还须内容兼容——流式累积必是快照
 * 终态的前缀,否则它是同类型的另一个块(interleaved thinking / 多 text 块场景),
 * 应继续向后找或 append。快照内容字段为空(redacted thinking)视为兼容。
 * tool_use 有唯一 id,直接比对。
 */
function snapshotMatches(current: ContentBlock, incoming: ContentBlock): boolean {
  if (current.type !== incoming.type) return false
  const compat = (cur: unknown, inc: unknown): boolean => {
    if (typeof inc !== 'string' || inc.length === 0) return true
    return typeof cur === 'string' && inc.startsWith(cur)
  }
  if (incoming.type === 'text') {
    return compat((current as { text?: string }).text, (incoming as { text?: string }).text)
  }
  if (incoming.type === 'thinking') {
    return compat(
      (current as { thinking?: string }).thinking,
      (incoming as { thinking?: string }).thinking,
    )
  }
  if (incoming.type === 'tool_use') {
    const curId = (current as { id?: string }).id
    const incId = (incoming as { id?: string }).id
    return !curId || !incId || curId === incId
  }
  return true
}

/**
 * 快照块合并进流式块:text/thinking 一律跳过——字符级 delta 累积是这两个字段的
 * 权威来源(快照可能先于 pending 落地到达,直接覆盖会丢增量;redacted 模式下快照
 * thinking 还是空串,覆盖会擦掉已积累的明文)。其余字段(signature/input/name/id)
 * 取快照值校正——快照里 tool_use.input 是 CLI 已 parse 的完整对象,比 partial_json
 * 拼接更可靠。
 */
function mergeSnapshotBlock(current: Record<string, unknown>, incoming: ContentBlock) {
  for (const [k, v] of Object.entries(incoming)) {
    if (k === 'text' || k === 'thinking') continue
    current[k] = v
  }
}

// ---- 流结束回调（通知层订阅:完成 toast / 系统通知） ----

type StreamFinishedCallback = (sessionId: string, hasError: boolean) => void
const finishedCallbacks = new Set<StreamFinishedCallback>()

/** 注册流结束回调，返回注销函数 */
export function onStreamFinished(cb: StreamFinishedCallback): () => void {
  finishedCallbacks.add(cb)
  return () => { finishedCallbacks.delete(cb) }
}

/**
 * 「后台结束」脏标记:流式结束时该会话的详情可能没有挂载(在别的 tab/未展开),
 * 下次该会话的详情加载时据此强制 reload,拿到 jsonl 落账后的完整记录。
 */
export const finishedDirty = reactive(new Set<string>())

// ---- 全局事件监听（App 挂载时注册一次,按 session_id 分发） ----

let listenersReady = false

export async function initStreamListeners(): Promise<void> {
  if (listenersReady) return
  listenersReady = true
  document.addEventListener('visibilitychange', () => bump(true))

  const handleStreamEvent = (payload: StreamEventPayload) => {
    const sid = payload.session_id
    if (!sid) return
    const state = getStream(sid)

    // 进程在发事件必然存活。error 除外：它也是进程异常退出的临终报错载体，
    // 且 spawn 失败等场景只有 error 没有后续 exited 事件，置 true 会卡死活态
    if (payload.kind !== 'error') {
      // false→true 边沿 = 新进程代际开始，刷新锚点。webview 刷新场景下边沿
      // 时刻晚于真实进程启动（误判死风险短暂存在），随后 syncProcessAlive 的
      // Rust 真值校准会修正
      if (!state.processAlive) state.processStartedAtMs = Date.now()
      state.processAlive = true
    }

    switch (payload.kind) {
      case 'block_start':
        // partial messages 模式下,content_block_start 首次出现某 index 的块——
        // 找不到 turn 就新建,然后在 index 位置放置初始块(text:""/tool_use:input{} 等)
        if (payload.message_id && payload.index !== undefined && payload.content_block) {
          // 已落账 message 的迟到事件(重复思考摘要等):内容已在历史区,忽略
          if (landedTombstones.has(payload.message_id)) break
          let entry = turnIndex.get(payload.message_id)
          if (!entry) {
            if (!state.streaming) ghostAudit('block_start', payload.message_id, payload.content_block.type, sid)
            const turn: StreamingTurn = { messageId: payload.message_id, content: [], model: payload.model, live: true }
            state.streamingTurns.push(turn)
            // reactive 数组里取回代理对象,保证后续 mutate 走响应式
            const reactiveTurn = state.streamingTurns[state.streamingTurns.length - 1]
            entry = { turn: reactiveTurn, sessionId: sid }
            turnIndex.set(payload.message_id, entry)
          } else if (!entry.turn.model && payload.model) {
            entry.turn.model = payload.model
          }
          // 新块开始 → 冲掉前面块的批处理积压:显示顺序必须与因果一致——
          // 工具卡走即时通道,若前面文本还在回放,会出现"工具卡先完整出现、
          // 上方文本还在爬"的时序倒挂(真流式暴露的缺口:assistant 快照路径
          // 已有此冲刷,block_start 路径此前没有)
          flushPendingBefore(payload.message_id, payload.index)
          ;(entry.turn.content as ContentBlock[])[payload.index] = payload.content_block
          // thinking 块计时起点
          if (payload.content_block.type === 'thinking') {
            thinkingStartTimes.set(partialJsonKey(payload.message_id, payload.index), Date.now())
          }
          // 新块到达:之前等待执行的工具已经返回(开始下一段输出)
          if (payload.content_block.type === 'tool_use') {
            const name = (payload.content_block as { name?: string }).name ?? i18n.global.t('block.tool')
            state.activeTool = name
          } else {
            state.activeTool = null
          }
          markTailDirty(sid)
          bump()
        }
        break
      case 'block_delta':
        // 字符级增量——text/thinking/signature 走 pending 批处理(避免 markdown 重渲染卡顿);
        // input_json_delta 直接累 toolPartialJson,block_stop 才一次性 parse,无渲染压力
        if (payload.message_id && payload.index !== undefined && payload.delta) {
          const d = payload.delta
          const key = partialJsonKey(payload.message_id, payload.index)
          switch (d.type) {
            case 'text_delta':
              if (d.text) {
                const e = pendingEntryOf(key)
                e.text = (e.text ?? '') + d.text
                accumulateTailText(sid, d.text)
              }
              break
            case 'thinking_delta':
              if (d.thinking) {
                const e = pendingEntryOf(key)
                e.thinking = (e.thinking ?? '') + d.thinking
              }
              break
            case 'signature_delta':
              if (d.signature) {
                const e = pendingEntryOf(key)
                e.signature = d.signature
              }
              break
            case 'input_json_delta':
              if (d.partial_json !== undefined) {
                toolPartialJson.set(key, (toolPartialJson.get(key) ?? '') + d.partial_json)
              }
              break
          }
          bump()
        }
        break
      case 'block_stop':
        // 该 block 不再有 delta。text 残余不在此落地——下一帧批 flush 落净
        // (流结束的 finishStream 有 flush(true) 兜底防丢);立即落地会让块尾瞬间跳满。
        // tool_use 的 partial_json 在 stop 时 parse 写入 block.input
        if (payload.message_id && payload.index !== undefined) {
          const key = partialJsonKey(payload.message_id, payload.index)
          const accumulated = toolPartialJson.get(key)
          const entry = turnIndex.get(payload.message_id)
          const block = entry?.turn.content[payload.index] as ContentBlock | undefined
          if (accumulated !== undefined) {
            if (block?.type === 'tool_use') {
              try {
                ;(block as { input: Record<string, unknown> }).input = JSON.parse(accumulated)
              } catch {
                // 部分 tool_use 可能 partial_json 为空字符串(参数即 {}),parse 失败安全降级保持 {}
              }
            }
            toolPartialJson.delete(key)
          }
          // thinking 块计时终点
          if (block?.type === 'thinking') {
            const startTime = thinkingStartTimes.get(key)
            if (startTime) {
              ;(block as { _thinkingMs?: number })._thinkingMs = Date.now() - startTime
              thinkingStartTimes.delete(key)
            }
          }
          // 工具块结束 = CLI 即将执行该工具:尾部工具行补目标摘要,状态转「等待工具」
          if (block?.type === 'tool_use') {
            const name = (block as { name?: string }).name ?? i18n.global.t('block.tool')
            const target = toolTarget((block as { input?: Record<string, unknown> }).input)
            state.activeTool = target ? `${name} · ${target}` : name
            markTailDirty(sid)
          }
        }
        bump()
        break
      case 'assistant_message':
        // 快照校正 + 批处理喂料。CLI 2.1.177+ 纯快照模式下不发 content_block_delta,
        // 只发 assistant 快照(可能多次:空→部分→完整)。检测文本增量,
        // 扣除已落地+已排队部分,喂入 pendingTextDeltas 复用批处理管线。
        // 兼容旧 CLI:若 block_delta 已喂过相同字符,增量为 0,不重复。
        if (payload.message_id && payload.content) {
          const mid = payload.message_id
          // 已落账 message 的迟到快照(重复思考摘要等):内容已在历史区,忽略
          if (landedTombstones.has(mid)) break
          const entry = turnIndex.get(mid)
          if (entry) {
            const existing = entry.turn
            if (!existing.model && payload.model) existing.model = payload.model
            if (shouldReplaceUsage(existing.usage, existing.stopReason, payload.usage, payload.stop_reason)) {
              existing.usage = payload.usage
              existing.stopReason = payload.stop_reason
            }
            let cursor = 0
            for (const incoming of payload.content) {
              let matched = -1
              for (let j = cursor; j < existing.content.length; j++) {
                const b = existing.content[j] as ContentBlock | undefined
                if (b && snapshotMatches(b, incoming)) {
                  matched = j
                  break
                }
              }
              if (matched < 0) {
                const idx = existing.content.length
                // 新块到达 → 冲掉前面还在打字的块,避免积压阻塞
                flushPendingBefore(mid, idx)
                const stripped: Record<string, unknown> = { ...incoming }
                if (incoming.type === 'text') stripped.text = ''
                if (incoming.type === 'thinking') stripped.thinking = ''
                feedSnapshotText(mid, idx, incoming, stripped, sid)
                ;(existing.content as ContentBlock[]).push(stripped as ContentBlock)
                cursor = existing.content.length
              } else {
                feedSnapshotText(mid, matched, incoming, existing.content[matched] as unknown as Record<string, unknown>, sid)
                mergeSnapshotBlock(
                  existing.content[matched] as unknown as Record<string, unknown>,
                  incoming,
                )
                cursor = matched + 1
              }
            }
          } else {
            const strippedContent: ContentBlock[] = payload.content.map(b => {
              const s: Record<string, unknown> = { ...b }
              if (b.type === 'text') s.text = ''
              if (b.type === 'thinking') s.thinking = ''
              return s as ContentBlock
            })
            if (!state.streaming) ghostAudit('assistant_message', mid, null, sid)
            state.streamingTurns.push({
              messageId: mid,
              content: strippedContent,
              model: payload.model,
              usage: hasReportedUsage(payload.usage) ? payload.usage : undefined,
              stopReason: payload.stop_reason,
              live: true,
            })
            const reactiveTurn = state.streamingTurns[state.streamingTurns.length - 1]
            turnIndex.set(mid, { turn: reactiveTurn, sessionId: sid })
            for (let idx = 0; idx < payload.content.length; idx++) {
              feedSnapshotText(mid, idx, payload.content[idx], strippedContent[idx] as unknown as Record<string, unknown>, sid)
            }
          }
          bump()
        }
        break
      case 'result':
        // result 到达,流将结束;工具等待态清空
        state.activeTool = null
        if (payload.context_window) state.realContextWindow = payload.context_window
        if (payload.input_tokens) state.realUsedTokens = payload.input_tokens
        if (payload.output_tokens) state.realOutputTokens = payload.output_tokens
        markTailDirty(sid)
        break
      case 'error':
        state.streamError = payload.message || i18n.global.t('common.unknownError')
        markTailDirty(sid)
        break
    }
  }

  // 新版 Rust 将 32ms 窗口内的多会话 delta 合成一个 IPC；保留单事件监听，
  // 兼容开发期前端连到旧后端的热重载场景。
  await Promise.all([
    listen<StreamEventPayload[]>('stream-events', (event) => {
      for (const payload of event.payload) handleStreamEvent(payload)
    }),
    listen<StreamEventPayload>('stream-event', (event) => handleStreamEvent(event.payload)),
  ])

  // 轮次归属分流:自发轮(CLI 被 task-notification 唤醒,result 带 origin → initiator=auto)
  // 不得冒领用户消息的 streaming 标志——用户在后台任务期间发的消息还在 CLI 队列里,
  // 自发轮的 done 若走 finishStream 会提前收尾:落账 reload 匹配不到未落盘的用户
  // 消息、真轮的 done 又被 !streaming 守卫吞掉,落账链就此断裂(消息重复出现/消失)。
  // initiator 缺失(emit_error / EOF 兜底路径)按 user 处理;user 语义但 streaming
  // 已 false 的失配(打标遗漏的边角)同样转 auto 收尾,不再静默丢弃。
  await listen<{ session_id: string; initiator?: string }>('stream-done', (event) => {
    const sid = event.payload?.session_id
    if (!sid) return
    const state = streams.get(sid)
    if (event.payload?.initiator === 'auto' || (state && !state.streaming)) {
      landAutoTurn(sid)
      return
    }
    finishStream(sid)
  })

  // 新进程握手完成：刷新代际锚点。warm 重启（渠道/effort/模型变更 needs_restart）
  // 时旧进程慢退 → EOF 晚于新进程入表 → superseded 抑制 exited → processAlive
  // 无 false→true 边沿，三个锚点写入源全部落空——此监听是该场景下锚点唯一的
  // 确定性刷新通道（Rust 在入表前 emit，与 exited 的先后序有保证）
  await listen<{ session_id: string; started_at_ms?: number }>('session-connected', (event) => {
    const sid = event.payload?.session_id
    if (!sid) return
    const s = getStream(sid)
    s.processAlive = true
    s.processStartedAtMs = event.payload.started_at_ms ?? Date.now()
  })

  // 长活进程真正退出（与 stream-done 的"轮次结束"语义分离）。
  // 被新进程顶替的旧进程 EOF 不发此事件（Rust 端 superseded 判定），活态不抖动
  await listen<{ session_id: string }>('session-process-exited', (event) => {
    const sid = event.payload?.session_id
    if (!sid) return
    // streams.get 而非 getStream:已驱逐(evictSessionTransients)的会话完全跳过,
    // 临终事件不得在 streams/processEpoch 重建任何条目(泄漏)
    const s = streams.get(sid)
    if (s) {
      processEpoch.set(sid, (processEpoch.get(sid) ?? 0) + 1)
      s.processAlive = false
      // live 失效保险:进程已死必无在传 turn,且其记录永无落盘可能——不清则
      // typing-dots 常驻/状态永不 idle/渲染卡 plain(回归审查 R2,已证实)
      let hadLive = false
      for (const t of s.streamingTurns) {
        if (t.live) {
          t.live = false
          hadLive = true
        }
      }
      if (hadLive) {
        streamingTick.value++
        markTailDirty(sid)
      }
    }
  })

  // Remote Control 判决:rcActive 的唯一写入源(CLI 对 rc-* 请求的 success/error 真值),
  // 发送侧不乐观置位——判决与 invoke 返回并发,乐观值会覆盖先到的判决。
  // getStream 而非 streams.get:判决可能先于首条消息到达(展开列即连接),需建态承接。
  // toast 由 useNotifications 监听同一事件负责(它反向依赖本模块,不能在此 import)
  await listen<{ session_id: string; active: boolean }>('rc-status', (event) => {
    getStream(event.payload.session_id).rcActive = event.payload.active
  })
}

// ---- 自发轮收尾(task-notification 唤醒的后台任务收尾轮) ----

/** 自发轮落账信号:SessionDetail 各实例 watch 它,命中自己会话时静默 reload */
export const autoTurnLanded = ref(0)
/** 待落账会话 → done 时刻在播 turn 的 messageId 快照(信号 payload,消费方认领后删除)。
 *  快照供落账后降级孤儿 live 用:fresh 里没有对应记录的快照 turn 说明该轮记录
 *  不会再落盘(崩溃/ID 失配),live 必须降级,否则永久卡「进行中」。连环自发轮合并快照 */
export const autoLandedSessions = new Map<string, Set<string>>()

/**
 * 自发轮结束收尾:与 finishStream 的本质区别是**不动 streaming / pending**——
 * streaming 标志属于用户 send 的轮次,自发轮无权翻转它;pending 气泡的退场
 * 只由落账匹配驱动。这里只做传输层收尾 + 发落账信号:
 * 挂载中的 SessionDetail 静默 reload 并摘除已落账 turn(原子切换到历史区),
 * 未挂载的会话靠 finishedDirty 在下次加载时 force reload 兜底。
 */
function landAutoTurn(sessionId: string) {
  const state = streams.get(sessionId)
  if (!state) return
  if (import.meta.env.DEV) console.log(`%c ========== [stream] landAutoTurn(自发轮收尾) sid=${sessionId.slice(0, 8)} t=${performance.now().toFixed(0)} ==========`, 'color:#8b5cf6;font-weight:bold')
  // 该轮已结束,残字瞬吐落地(自发轮不做排空等待,马上要与历史区做原子切换)
  flushTextDeltas(sessionId, true)
  state.activeTool = null
  markTailDirty(sessionId)
  streamingTick.value++
  finishedDirty.add(sessionId)
  // 快照当刻在播 turn:落账后据此降级未落盘的孤儿 live。不在此直接清 live——
  // 落账前清会让 keepLive 清扫掉 turn 而 records 尚无副本,内容闪失
  const liveIds = new Set(state.streamingTurns.filter(t => t.live).map(t => t.messageId))
  const prev = autoLandedSessions.get(sessionId)
  if (prev) liveIds.forEach(id => prev.add(id))
  else autoLandedSessions.set(sessionId, liveIds)
  autoTurnLanded.value++
}

/** 落账后降级孤儿 live:快照内、recs 中无对应 assistant 记录的 turn 说明其记录
 *  不会再落盘,live 置 false(渲染转终态、忙态解除)。快照守卫防误伤等待期间新开的轮 */
function demoteUnlandedTurns(
  sessionId: string,
  snapshot: Set<string>,
  recs: ReadonlyArray<{ type: string; message?: unknown }>,
) {
  const state = streams.get(sessionId)
  if (!state) return
  const landed = new Set<string>()
  for (const r of recs) {
    if (r.type !== 'assistant') continue
    const id = (r.message as { id?: string } | null | undefined)?.id
    if (id) landed.add(id)
  }
  let changed = false
  for (const t of state.streamingTurns) {
    if (t.live && snapshot.has(t.messageId) && !landed.has(t.messageId)) {
      t.live = false
      changed = true
    }
  }
  if (changed) {
    streamingTick.value++
    markTailDirty(sessionId)
  }
}

/**
 * 摘除已在 recs 中落账的流式 turn:历史区同 messageId 记录解除
 * streamingMessageIds 过滤,与 turn 卸载同一 batch 原子切换,无双显无空窗。
 * 用户轮正在流式时也安全——其 messageId 尚未落盘,不会被摘。
 */
function removeLandedTurns(sessionId: string, recs: ReadonlyArray<{ type: string; message?: unknown }>) {
  const state = streams.get(sessionId)
  if (!state || !state.streamingTurns.length) return
  const landed = new Set<string>()
  for (const r of recs) {
    if (r.type !== 'assistant') continue
    const id = (r.message as { id?: string } | null | undefined)?.id
    if (id) landed.add(id)
  }
  if (!landed.size) return
  const keep = state.streamingTurns.filter(t => !landed.has(t.messageId))
  if (keep.length === state.streamingTurns.length) return
  for (const t of state.streamingTurns) {
    if (landed.has(t.messageId)) {
      dropTurnTransients(t.messageId)
      tombstone(t.messageId)
    }
  }
  state.streamingTurns = keep
  markTailDirty(sessionId)
}

/**
 * 某一路流结束(CLI done)——如果批处理还有未落地残余,进入排空模式,
 * 等 pending 落净(最多一帧)再翻 streaming=false,避免:
 *   1. flushTextDeltas(true) 一次性倾倒剩余文字(文字跳变)
 *   2. streaming=false 触发 BlockText plain→shiki 切换(布局抖动)
 *   3. 300ms 后 records reload(DOM 重建)
 * 三次突变叠在一帧 → 可见闪烁。
 * 通知回调(toast/系统通知)立即触发,不等排空。
 */
function finishStream(sessionId: string) {
  const state = streams.get(sessionId)
  if (!state || !state.streaming) return
  if (import.meta.env.DEV) console.log(`%c ========== [stream] finishStream sid=${sessionId.slice(0, 8)} pending=${hasSessionPending(sessionId)} t=${performance.now().toFixed(0)} ==========`, 'color:#f59e0b;font-weight:bold')
  state.activeTool = null
  markTailDirty(sessionId)
  const hasError = !!state.streamError
  finishedCallbacks.forEach(cb => {
    try { cb(sessionId, hasError) } catch (_) { /* */ }
  })
  if (hasSessionPending(sessionId)) {
    // 屏外会话没有逐字播放价值，结果到达时直接追平，避免后台 residual timer。
    if (!isSessionSurfaceVisible(sessionId) || document.visibilityState !== 'visible') {
      flushTextDeltas(sessionId, true)
      completeFinish(sessionId)
      return
    }
    drainingStreams.add(sessionId)
    bump()
    return
  }
  completeFinish(sessionId)
}

function completeFinish(sessionId: string) {
  const state = streams.get(sessionId)
  if (!state || !state.streaming) return
  if (import.meta.env.DEV) console.log(`%c ========== [stream] completeFinish → streaming=false sid=${sessionId.slice(0, 8)} t=${performance.now().toFixed(0)} ==========`, 'color:#ef4444;font-weight:bold')
  state.streaming = false
  // turn 传输态同步结束:BlockText 的 plain→shiki 上色由 streaming||live 联合 prop
  // 翻 false 触发,同一实例 patch 一次(维持零重挂契约)
  for (const t of state.streamingTurns) t.live = false
  probeFinishFlip(sessionId)
  frameWatchRelease()
  finishedDirty.add(sessionId)
  markTailDirty(sessionId)
  streamingTick.value++
  // 注意:不清 streamingTurns / pendingUserMessage——SessionDetail 在 reload 拿到
  // jsonl 落账后同 batch 清,避免窗口期空白闪烁;无详情挂载的会话等下次发送时重置。
  if (state.lastSent) {
    triggerMetaGeneration(sessionId, state.lastSent.cwd)
    // 成功落账后剥离图片 base64:重试(retrySession)只服务失败场景,成功后这批
    // base64 会钉死在 streams 单例直到下次发送——多图会话可达数十 MB(审计 P1 尾项)
    if (state.lastSent.opts.images?.length) {
      state.lastSent = {
        ...state.lastSent,
        opts: { ...state.lastSent.opts, images: undefined },
      }
    }
  }
}

// ---- 操作 ----

/** 发送消息并开始流式接收。opts 可选,缺省时不向 CLI 附加 --model / --effort */
async function sendMessage(
  sessionId: string,
  cwd: string,
  message: string,
  opts: SendOptions = {},
) {
  const state = getStream(sessionId)

  if (state.streaming) {
    state.pendingQueue.push({ message, opts })
    return
  }

  // 清掉上一轮的 turn 索引、残余 pending 与尾部。
  // 豁免仍在播的自发轮(live):清掉它会让后续快照以 alreadyKnown=0 重建 turn,
  // 整段任务回答从第一个字重播;保留则渲染连续,其落账摘除由 landAutoTurn 驱动
  const liveTurns = state.streamingTurns.filter(t => t.live)
  for (const t of state.streamingTurns) {
    if (!t.live) dropTurnTransients(t.messageId)
  }
  tailTextAcc.delete(sessionId)

  // 冷启动（无活进程）即将 spawn 新代际：先记锚点。spawn 前时刻偏早，
  // 只会少判死不会误杀，且覆盖首事件到达前的窗口期
  if (!state.processAlive) state.processStartedAtMs = Date.now()
  state.streaming = true
  frameWatchRetain()
  state.streamError = null
  state.pendingUserMessage = message
  state.pendingImages = opts.images?.length ? opts.images : null
  state.pendingSentAt = Date.now()
  state.streamingTurns = liveTurns
  state.startedAt = Date.now()
  state.activeTool = null
  state.tail = []
  state.lastSent = { cwd, message, opts }
  try {
    await invoke('start_streaming', {
      sessionId,
      cwd,
      message,
      model: opts.model ?? null,
      effort: opts.effort ?? null,
      channel: opts.channel ?? null,
      advisor: opts.advisor ?? false,
      chrome: opts.chrome ?? false,
      forkSource: opts.forkSource ?? null,
      extraArgs: opts.extraArgs || null,
      images: opts.images?.length ? opts.images : null,
      permissionMode: opts.permissionMode ?? null,
      sessionCapabilities: collectSessionCapabilities(),
      forceNew: opts.forceNew ?? false,
    })
  } catch (e) {
    const err = String(e)
    // 断链校验：会话历史在别处（worktree 迁走等），给用户知情选择
    if (err.includes('SESSION_ELSEWHERE:')) {
      finishStream(sessionId)
      const { confirm } = useConfirm()
      const ok = await confirm(
        i18n.global.t('session.elsewhereConfirm'),
        i18n.global.t('session.elsewhereOk'),
      )
      if (ok) {
        await sendMessage(sessionId, cwd, message, { ...opts, forceNew: true })
      }
      return
    }
    state.streamError = err
    finishStream(sessionId)
  }
}

/** 出错重试:用最近一次发送的消息与参数重发(FR-003 就地决策) */
async function retrySession(sessionId: string): Promise<boolean> {
  const state = streams.get(sessionId)
  if (!state || state.streaming || !state.lastSent) return false
  const { cwd, message, opts } = state.lastSent
  state.streamError = null
  await sendMessage(sessionId, cwd, message, opts)
  return true
}

/** 清空某会话的流式渲染区(不影响磁盘 jsonl)。供 /clear 与会话切换使用。
 *  keepPending:保留 pending 用户消息——流结束 reload 场景用,气泡的退场由
 *  「历史区落账匹配成功」驱动(clearPendingUserMessage),而非 reload 无条件批清;
 *  否则守卫误判 reload 成功而用户消息未落账时,两个显示源皆空 = 消息凭空消失。
 *  keepLive:豁免仍在播的自发轮 turn(未落账,清了会被后续快照重建从头重播) */
function clearStreamingTurns(sessionId: string, opts?: { keepPending?: boolean; keepLive?: boolean }) {
  const state = streams.get(sessionId)
  if (!state) return
  const keep = opts?.keepLive ? state.streamingTurns.filter(t => t.live) : []
  for (const t of state.streamingTurns) {
    if (!keep.includes(t)) dropTurnTransients(t.messageId)
  }
  state.streamingTurns = keep
  if (!opts?.keepPending) {
    state.pendingUserMessage = null
    state.pendingImages = null
    state.pendingSentAt = null
  }
  state.streamError = null
  markTailDirty(sessionId)
}

/** pending 用户消息已被历史区落账记录接管,清理流式区气泡状态 */
function clearPendingUserMessage(sessionId: string) {
  const state = streams.get(sessionId)
  if (!state) return
  state.pendingUserMessage = null
  state.pendingImages = null
  state.pendingSentAt = null
}

function removePendingQueueItem(sessionId: string, index: number) {
  const state = streams.get(sessionId)
  if (state) state.pendingQueue.splice(index, 1)
}

/** 消费 BTW 队列队首：前一轮 reload 落账后由 SessionDetail 调用 */
async function consumePendingQueue(sessionId: string, cwd: string) {
  const state = streams.get(sessionId)
  if (!state || state.streaming || state.pendingQueue.length === 0) return
  const next = state.pendingQueue.shift()!
  await sendMessage(sessionId, cwd, next.message, next.opts)
}

/** 中断当前回复（发 interrupt 控制消息，不杀进程；
 *  stream-done 由 CLI interrupt 响应的 result 事件驱动，无需前端同步 finishStream。
 *  interrupt 失败时前端兜底收尾，防止永远卡在 streaming 状态） */
async function stopStreaming(sessionId: string) {
  try {
    await invoke('stop_streaming', { sessionId })
  } catch (_) {
    finishStream(sessionId)
  }
}

/** 驱逐某会话的前端传输态缓存。streams/turnIndex/tailTextAcc 等模块级 Map
 *  没有其他 delete 路径，长时运行 + 频繁开关会话下单调增长（审计遗留⑦）。
 *  调用方：closeSession 与 useWorkbench.teardownSession（会话离开工作台的
 *  真实关闭路径直接 invoke close_session，必须同步走这里，否则驱逐是死代码） */
export function evictSessionTransients(sessionId: string) {
  const state = streams.get(sessionId)
  if (state) {
    for (const t of state.streamingTurns) dropTurnTransients(t.messageId)
  }
  tailTextAcc.delete(sessionId)
  drainingStreams.delete(sessionId)
  processEpoch.delete(sessionId)
  autoLandedSessions.delete(sessionId)
  finishedDirty.delete(sessionId)
  lastSessionFlushAt.delete(sessionId)
  streamRenderSurfaces.delete(sessionId)
  if (activeRenderSessionId === sessionId) activeRenderSessionId = null
  streams.delete(sessionId)
}

/** 关闭会话进程（SIGTERM → 5s → SIGKILL），并驱逐该会话的前端传输态缓存 */
async function closeSession(sessionId: string) {
  try {
    await invoke('close_session', { sessionId })
  } catch (_) {
    // ignore
  }
  evictSessionTransients(sessionId)
}

export function useStreaming() {
  return {
    streams,
    getStream,
    sendMessage,
    retrySession,
    stopStreaming,
    closeSession,
    clearStreamingTurns,
    clearPendingUserMessage,
    removePendingQueueItem,
    consumePendingQueue,
    removeLandedTurns,
    demoteUnlandedTurns,
  }
}
