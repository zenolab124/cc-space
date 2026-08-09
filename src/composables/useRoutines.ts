import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import i18n from '../locales'
import type { EngineInstanceId } from '../engines/types'
import { useNotifications } from './useNotifications'

export interface RoutineSource {
  /** 创建入口：ui | mcp */
  kind: string
  /** 发起会话的项目路径（MCP 场景） */
  project?: string
  /** MCP 客户端标识（如 claude-code 2.1.187） */
  client?: string
}

export interface RoutineDefinition {
  id: string
  name: string
  cronExpression: string
  originalText: string
  prompt: string
  engine: EngineInstanceId
  enabled: boolean
  createdAt: string
  lastRun: string | null
  nextRun: string | null
  /** 任务来源；旧数据无此字段 */
  source?: RoutineSource
}

export interface RoutineExecutionLog {
  routineId: string
  engine: EngineInstanceId
  startedAt: string
  finishedAt: string | null
  exitCode: number | null
  stdout: string
  stderr: string
  /** 落盘会话 ID（会话落盘开启时才有），据此打开完整会话 */
  sessionId?: string
  /** 用户手动终止（区别于执行失败）；旧日志无此字段 */
  cancelled?: boolean
}

export interface RoutineRow extends RoutineDefinition {
  lastExecution: RoutineExecutionLog | null
  isRunning: boolean
  /** 正在运行时的开始时刻（RFC3339），供显示已耗时 */
  runningStartedAt: string | null
}

const routines = ref<RoutineRow[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
let loaded = false

async function loadRoutines() {
  loading.value = true
  error.value = null
  try {
    routines.value = await invoke<RoutineRow[]>('get_routines')
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

async function ensureLoaded(force = false) {
  if (loaded && !force) return
  loaded = true
  await loadRoutines()
}

function refresh() {
  ensureLoaded(true)
}

interface RoutineExecutedPayload {
  routineId: string
  name: string
  exitCode: number | null
  durationMs: number
  cancelled?: boolean
}

function formatDuration(ms: number): string {
  const s = Math.round(ms / 1000)
  if (s < 60) return `${s}s`
  return `${Math.floor(s / 60)}m ${s % 60}s`
}

let listenerReady = false
async function initRoutineListener() {
  if (listenerReady) return
  listenerReady = true
  const { notifyTransient } = useNotifications()
  await listen('routine-started', () => {
    loadRoutines()
  })
  await listen<RoutineExecutedPayload>('routine-executed', (e) => {
    loadRoutines()
    const { name, exitCode, durationMs, cancelled } = e.payload
    if (cancelled) {
      notifyTransient(
        i18n.global.t('automation.runCancelled', { name }),
        formatDuration(durationMs),
      )
    } else if (exitCode === 0) {
      notifyTransient(
        i18n.global.t('automation.runDone', { name }),
        formatDuration(durationMs),
      )
    } else {
      notifyTransient(
        i18n.global.t('automation.runFailed', { name }),
        i18n.global.t('automation.runFailedHint'),
      )
    }
  })
  await listen('routines-changed', () => {
    loadRoutines()
  })
}

async function createRoutine(params: {
  name: string
  cronExpression: string
  originalText: string
  prompt: string
  engine: EngineInstanceId
  enabled: boolean
}): Promise<RoutineDefinition> {
  const result = await invoke<RoutineDefinition>('create_routine', params)
  await loadRoutines()
  return result
}

async function updateRoutine(
  id: string,
  patch: Partial<Pick<RoutineDefinition, 'name' | 'cronExpression' | 'originalText' | 'prompt' | 'engine' | 'enabled'>>,
): Promise<RoutineDefinition> {
  const result = await invoke<RoutineDefinition>('update_routine', { id, ...patch })
  await loadRoutines()
  return result
}

async function updateAllRoutineEngines(engine: EngineInstanceId): Promise<number> {
  const changed = await invoke<number>('update_all_routine_engines', { engine })
  routines.value = routines.value.map(routine => ({ ...routine, engine }))
  await loadRoutines()
  return changed
}

async function deleteRoutine(id: string): Promise<void> {
  await invoke('delete_routine', { id })
  await loadRoutines()
}

async function runNow(id: string): Promise<void> {
  await invoke('run_routine_now', { id })
  // 立即刷新出「运行中」状态，不等 routine-started 事件（双保险）
  await loadRoutines()
}

async function stopRoutine(id: string): Promise<void> {
  await invoke('stop_routine', { id })
  // 进程退出与状态归位由执行方收尾驱动（事件/watcher），此处不立刷——
  // TERM 生效前 marker 仍在，立刷只会读回「运行中」
}

async function getRoutineLogs(id: string, limit?: number): Promise<RoutineExecutionLog[]> {
  return invoke<RoutineExecutionLog[]>('get_routine_logs', { id, limit: limit ?? 20 })
}

async function parseNaturalSchedule(text: string): Promise<string> {
  return invoke<string>('parse_natural_schedule', { text })
}

export function useRoutines() {
  return {
    routines,
    loading,
    error,
    ensureLoaded,
    refresh,
    initRoutineListener,
    createRoutine,
    updateRoutine,
    updateAllRoutineEngines,
    deleteRoutine,
    runNow,
    stopRoutine,
    getRoutineLogs,
    parseNaturalSchedule,
  }
}
