import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { RunnerSnapshot, LogLine, RunnerCommand } from '@/types'

// --- 模块级单例状态 ---

/** sessionId -> 该会话的 runner 快照列表 */
const runners = ref<Map<string, RunnerSnapshot[]>>(new Map())

/** runnerId -> 日志行（ring buffer 上限 2000） */
const logs = ref<Map<string, LogLine[]>>(new Map())

/** projectCwd -> 该项目的候选命令（按项目分桶，多列不串项目） */
const commandsByProject = ref<Map<string, RunnerCommand[]>>(new Map())

/** sessionId -> 选中的 runner id（按会话分桶，多列不串台） */
const selectedRunnerIds = ref<Map<string, string | null>>(new Map())

/** 新跑单弹窗是否开启（悬浮面板点外关闭时豁免弹窗子树） */
const dialogOpen = ref(false)

// --- localStorage 持久化 ---

const STORAGE_KEY_PIN = 'monet-runner-pin'
const STORAGE_KEY_TAIL = 'monet-runner-tail-lines'
const VALID_TAIL_VALUES = [30, 100, 300, 1000] as const

function loadPin(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY_PIN) === 'true'
  } catch {
    return false
  }
}

function loadTailLines(): number {
  try {
    const raw = localStorage.getItem(STORAGE_KEY_TAIL)
    if (raw == null) return 100
    const n = Number.parseInt(raw, 10)
    return (VALID_TAIL_VALUES as readonly number[]).includes(n) ? n : 100
  } catch {
    return 100
  }
}

/** 面板是否钉住（不随会话切换自动收起） */
const runnerPinned = ref(loadPin())

/** 默认拉取尾部行数 */
const tailLinesDefault = ref(loadTailLines())

watch(runnerPinned, v => {
  try { localStorage.setItem(STORAGE_KEY_PIN, String(v)) } catch { /* 存储失败静默 */ }
})
watch(tailLinesDefault, v => {
  try { localStorage.setItem(STORAGE_KEY_TAIL, String(v)) } catch { /* 存储失败静默 */ }
})

// --- IPC 封装 ---

/** 拉取指定项目的候选命令 */
async function reloadCommandsForProject(cwd: string) {
  try {
    const list = await invoke<RunnerCommand[]>('runner_commands_list', { projectCwd: cwd })
    commandsByProject.value.set(cwd, list)
    commandsByProject.value = new Map(commandsByProject.value)
  } catch {
    // 静默
  }
}

/** 会话切换时加载候选命令（前端传会话 cwd 原文，Rust 端 canonicalize） */
function setCurrentProject(cwd: string | null) {
  if (cwd) reloadCommandsForProject(cwd)
}

/** 获取指定项目的候选命令 */
function getCommands(cwd: string): RunnerCommand[] {
  return commandsByProject.value.get(cwd) ?? []
}

/** 获取指定会话选中的 runner id */
function getSelectedRunner(sessionId: string): string | null {
  return selectedRunnerIds.value.get(sessionId) ?? null
}

/** 设置指定会话选中的 runner id */
function setSelectedRunner(sessionId: string, runnerId: string | null) {
  selectedRunnerIds.value.set(sessionId, runnerId)
  selectedRunnerIds.value = new Map(selectedRunnerIds.value)
}

// --- backfill 防并发 ---
const backfilling = new Set<string>()

/** seq 断裂时全量补齐日志（防并发：同一 runner 不重入） */
async function backfillLogs(runnerId: string) {
  if (backfilling.has(runnerId)) return
  backfilling.add(runnerId)
  try {
    const lines = await invoke<LogLine[]>('runner_tail', { runnerId, lines: 2000 })
    logs.value.set(runnerId, lines)
    logs.value = new Map(logs.value)
  } catch {
    // 补齐失败静默
  } finally {
    backfilling.delete(runnerId)
  }
}

/** 启动跑单进程 */
async function spawnRunner(
  sessionId: string,
  cmd: string,
  cwd: string,
  alias?: string,
  env?: Record<string, string>,
  sourceCommandId?: string,
): Promise<RunnerSnapshot> {
  const snap = await invoke<RunnerSnapshot>('runner_spawn', { sessionId, cmd, cwd, alias, env, sourceCommandId })
  // 对账兜底：秒崩类终态事件若丢失，2s 后拉真实状态纠偏
  setTimeout(() => loadRunners(sessionId), 2000)
  // 立即插入到列表
  const list = runners.value.get(sessionId) ?? []
  const idx = list.findIndex(r => r.id === snap.id)
  if (idx >= 0) list[idx] = snap
  else list.push(snap)
  runners.value.set(sessionId, list)
  runners.value = new Map(runners.value)
  setSelectedRunner(sessionId, snap.id)
  return snap
}

/** 停止跑单进程 */
async function stopRunner(runnerId: string, graceful = true): Promise<void> {
  await invoke<void>('runner_stop', { runnerId, graceful })
  // 操作后对账：终态事件若丢失（HMR/监听断），UI 不至于永久停在旧状态
  reconcileBySessionOf(runnerId)
}

/** 由 runner id 反查归属会话并拉取真实状态对账；查不到则全量对账 */
function reconcileBySessionOf(runnerId: string) {
  const delay = 3500 // graceful stop 有 TERM→3s→KILL 链，等终态落定后再对账
  for (const [sid, list] of runners.value) {
    if (list.some(r => r.id === runnerId)) {
      setTimeout(() => loadRunners(sid), delay)
      return
    }
  }
  setTimeout(() => loadRunners(), delay)
}

/** 重启跑单进程 */
async function restartRunner(runnerId: string): Promise<RunnerSnapshot> {
  const snap = await invoke<RunnerSnapshot>('runner_restart', { runnerId })
  reconcileBySessionOf(runnerId)
  // 更新列表中的快照
  for (const [sid, list] of runners.value) {
    const idx = list.findIndex(r => r.id === runnerId)
    if (idx >= 0) {
      list[idx] = snap
      runners.value.set(sid, list)
      runners.value = new Map(runners.value)
      break
    }
  }
  return snap
}

/** 加载 runner 列表（可按会话过滤）——合并对账语义：后端为准替换桶 */
async function loadRunners(sessionId?: string): Promise<void> {
  try {
    const list = await invoke<RunnerSnapshot[]>('runner_list', { sessionId: sessionId ?? null })
    if (sessionId) {
      runners.value.set(sessionId, list)
    } else {
      // 全量加载，按 sessionId 分桶
      const map = new Map<string, RunnerSnapshot[]>()
      for (const s of list) {
        const arr = map.get(s.sessionId) ?? []
        arr.push(s)
        map.set(s.sessionId, arr)
      }
      runners.value = map
    }
    runners.value = new Map(runners.value)
  } catch {
    // Rust 端尚未就绪时静默
  }
}

/** 删除候选命令（需指定项目 cwd） */
async function removeCommand(cwd: string, id: string): Promise<void> {
  await invoke<void>('runner_command_remove', { projectCwd: cwd, id })
  const list = commandsByProject.value.get(cwd)
  if (list) {
    commandsByProject.value.set(cwd, list.filter(c => c.id !== id))
    commandsByProject.value = new Map(commandsByProject.value)
  }
}

/** 停止会话下所有 runner */
async function stopAllForSession(sessionId: string): Promise<void> {
  try {
    await invoke<void>('runner_session_stop_all', { sessionId })
  } catch {
    // 静默
  }
}

// --- 纯前端查询 ---

function getSessionRunners(sessionId: string): RunnerSnapshot[] {
  return runners.value.get(sessionId) ?? []
}

function getRunnerLogs(runnerId: string): LogLine[] {
  return logs.value.get(runnerId) ?? []
}

/** 运行中实例数 */
function runningCount(sessionId: string): number {
  const list = runners.value.get(sessionId) ?? []
  return list.filter(r => r.status === 'running' || r.status === 'starting').length
}

/** 是否有崩溃的 runner */
function hasCrashed(sessionId: string): boolean {
  const list = runners.value.get(sessionId) ?? []
  return list.some(r => r.status === 'crashed' || r.status === 'spawn-failed')
}

// --- 事件监听初始化（listenersReady 模式） ---

const LOG_RING_BUFFER_MAX = 2000

let listenersReady = false

export async function initRunnerListeners(): Promise<void> {
  if (listenersReady) return
  listenersReady = true

  // runner-status 事件：裸 RunnerSnapshot payload，健壮性守卫
  await listen<RunnerSnapshot>('runner-status', (event) => {
    const snap = event.payload
    if (!snap?.id || !snap?.sessionId) return
    const list = runners.value.get(snap.sessionId) ?? []
    const idx = list.findIndex(r => r.id === snap.id)
    if (idx >= 0) list[idx] = snap
    else list.push(snap)
    runners.value.set(snap.sessionId, list)
    runners.value = new Map(runners.value)
  })

  // runner-log 事件：防御性去重排序 + 断裂检测
  listen<{ runnerId: string; lines: LogLine[] }>('runner-log', (event) => {
    const { runnerId, lines: newLines } = event.payload
    if (!newLines?.length) return
    const existing = logs.value.get(runnerId) ?? []

    // 快路径：严格递增且无断裂（Rust 单通道 seq 递增的常态路径）
    if (existing.length > 0) {
      const lastSeq = existing[existing.length - 1].seq
      const firstNewSeq = newLines[0].seq
      if (firstNewSeq === lastSeq + 1) {
        const merged = [...existing, ...newLines]
        const trimmed = merged.length > LOG_RING_BUFFER_MAX
          ? merged.slice(merged.length - LOG_RING_BUFFER_MAX)
          : merged
        logs.value.set(runnerId, trimmed)
        logs.value = new Map(logs.value)
        return
      }
      // 断裂（跳跃）
      if (firstNewSeq > lastSeq + 1) {
        backfillLogs(runnerId)
        return
      }
    } else if (newLines[0].seq > 0) {
      // 首批事件起始 seq > 0：中途接入，补齐历史
      backfillLogs(runnerId)
      return
    }

    // 慢路径：乱序或重叠——去重 + 排序 + 断裂检测
    const seenSeqs = new Set(existing.map(l => l.seq))
    const fresh = newLines.filter(l => !seenSeqs.has(l.seq))
    if (fresh.length === 0) return
    const merged = [...existing, ...fresh].sort((a, b) => a.seq - b.seq)
    for (let i = 1; i < merged.length; i++) {
      if (merged[i].seq > merged[i - 1].seq + 1) {
        backfillLogs(runnerId)
        return
      }
    }
    const trimmed = merged.length > LOG_RING_BUFFER_MAX
      ? merged.slice(merged.length - LOG_RING_BUFFER_MAX)
      : merged
    logs.value.set(runnerId, trimmed)
    logs.value = new Map(logs.value)
  })

  // runner-commands-changed 事件：刷新所有已跟踪项目的候选命令
  listen<{ project: string }>('runner-commands-changed', () => {
    setTimeout(() => {
      for (const cwd of commandsByProject.value.keys()) {
        reloadCommandsForProject(cwd)
      }
    }, 50)
  })

  // 初始全量水合
  await loadRunners()
}

// --- 组合式函数导出 ---

export function useRunners() {
  return {
    runners,
    logs,
    commandsByProject,
    dialogOpen,
    runnerPinned,
    tailLinesDefault,
    VALID_TAIL_VALUES,
    setCurrentProject,
    getCommands,
    getSelectedRunner,
    setSelectedRunner,
    spawnRunner,
    stopRunner,
    restartRunner,
    loadRunners,
    removeCommand,
    stopAllForSession,
    getSessionRunners,
    getRunnerLogs,
    runningCount,
    hasCrashed,
    backfillLogs,
  }
}
