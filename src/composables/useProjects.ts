import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { Project, SessionSummary } from '@/types'
import { tokenTotal } from '@/types'
import { listEngines, listProjects, listSessions } from '@/engines/client'
import { projectUiId, sessionUiId } from '@/engines/integration'
import type { EngineDescriptor, EngineProject, EngineSessionSummary } from '@/engines/types'
import { indexEngineSessions } from '@/engines/directory'
import { usesNativeSessionSurface } from '@/engines/integration'
import type { SourceChangeEnvelope } from '@/engines/events'

const projects = ref<Project[]>([])
/** 数据修订号:全量与增量刷新后都 +1。增量路径原地 mutate 不换 projects 引用,
 *  浅层 watch(projects) 收不到,需要跨刷新方式感知变更的一律 watch 这个 */
const projectsRevision = ref(0)
const selectedProjectIds = ref<Set<string>>(new Set())
const selectedEngineIds = ref<Set<string>>(new Set())
const loading = ref(false)
const error = ref<string | null>(null)
let watcherSetup = false

/** 档案馆按真实项目路径合并后的展示项；底层引擎项目仍保留在 projects 中。 */
interface ArchiveProject {
  id: string
  display_path: string
  sessions: SessionSummary[]
  session_count: number
  last_active: number | null
}

/** watcher 增量变更 payload（src-tauri/src/watcher.rs emit_pending_changes） */
interface SessionChange {
  projectId: string
  sessionId: string
}
interface ProjectsChangedPayload {
  full: boolean
  changes: SessionChange[]
}

/** 加载所有项目 */
async function loadProjects() {
  const hasCached = projects.value.length > 0
  if (!hasCached) loading.value = true
  error.value = null
  try {
    projects.value = await loadEngineProjects()
    projectsRevision.value++
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }

  // 首次加载后注册文件监控监听：按会话增量 patch，避免每秒全量整树替换
  // （docs/research/perf-audit-2026-07.md · P0-2）
  if (!watcherSetup) {
    watcherSetup = true
    listen<ProjectsChangedPayload>('projects-changed', (event) => {
      const payload = event.payload
      if (!payload || payload.full || !Array.isArray(payload.changes) || payload.changes.length === 0) {
        reloadProjectsSilently()
      } else {
        applySessionChanges(payload.changes)
      }
    })
    listen<SourceChangeEnvelope>('engine-source-change', event => {
      if (!usesNativeSessionSurface(event.payload.instance)) reloadProjectsSilently()
    })
  }
}

async function loadEngineProjects(): Promise<Project[]> {
  const descriptors = await listEngines()
  const results = await Promise.allSettled(descriptors.filter(descriptor => descriptor.enabled).map(loadEngineInstanceProjects))
  const loaded = results.flatMap(result => result.status === 'fulfilled' ? result.value : [])
  loaded.sort((a, b) => (b.last_active ?? 0) - (a.last_active ?? 0))
  if (!results.some(result => result.status === 'fulfilled')) {
    const firstError = results.find((result): result is PromiseRejectedResult => result.status === 'rejected')
    if (firstError) throw firstError.reason
  }
  indexEngineSessions(loaded)
  return loaded
}

async function loadEngineInstanceProjects(descriptor: EngineDescriptor): Promise<Project[]> {
  const sourceProjects = await listProjects(descriptor.instance)
  const mapped = await Promise.all(sourceProjects.map(async project => {
    const sessions = await listSessions(project.reference)
    return mapProject(descriptor, project, sessions)
  }))
  return mapped.filter(project => project.sessions.length > 0)
}

function mapProject(
  descriptor: EngineDescriptor,
  project: EngineProject,
  sessions: EngineSessionSummary[],
): Project {
  return {
    id: projectUiId(project.reference),
    native_id: project.reference.nativeId,
    reference: project.reference,
    engine: project.reference.engine,
    engine_name: descriptor.displayName,
    display_path: project.displayPath ?? project.displayName,
    source_path: project.displayPath,
    sessions: sessions.map(session => mapSession(descriptor, session)),
    session_count: sessions.length,
    last_active: epochSeconds(project.lastActive),
  }
}

function mapSession(descriptor: EngineDescriptor, session: EngineSessionSummary): SessionSummary {
  const cached = session.usage?.cachedInputTokens ?? 0
  const subagent = tokenUsageFromMeta(session.sourceMeta.subagentTokens)
  return {
    id: sessionUiId(session.reference),
    native_id: session.reference.nativeId,
    reference: session.reference,
    project_reference: session.project,
    engine: session.reference.engine,
    engine_name: descriptor.displayName,
    title: session.title,
    first_user_message: session.preview,
    model: session.model,
    git_branch: typeof session.sourceMeta.gitBranch === 'string' ? session.sourceMeta.gitBranch : null,
    cwd: session.cwd,
    version: typeof session.sourceMeta.version === 'string' ? session.sourceMeta.version : null,
    timestamp: session.createdAt,
    last_modified: epochSeconds(session.updatedAt) ?? 0,
    total_tokens: {
      input_tokens: session.usage?.inputTokens ?? 0,
      output_tokens: session.usage?.outputTokens ?? 0,
      cache_creation_input_tokens: session.usage?.cacheCreationInputTokens ?? 0,
      cache_read_input_tokens: cached,
    },
    subagent_tokens: subagent,
    file_size: numericMeta(session.sourceMeta.fileSize),
    message_count: typeof session.sourceMeta.messageCount === 'number' ? session.sourceMeta.messageCount : 0,
    context_window: typeof session.sourceMeta.contextWindow === 'number' ? session.sourceMeta.contextWindow : null,
    source_meta: { ...session.sourceMeta },
  }
}

function archiveProjectKey(project: Project): string {
  const sourcePath = project.source_path?.trim()
  if (sourcePath) return `path:${sourcePath.replace(/[\\/]+$/, '')}`
  // 没有可靠真实路径的引擎项目不能按展示名合并，避免多个未分类项目撞桶。
  return `source:${project.id}`
}

const archiveProjects = computed<ArchiveProject[]>(() => {
  const groups = new Map<string, ArchiveProject>()

  for (const project of projects.value) {
    const key = archiveProjectKey(project)
    const current = groups.get(key)
    if (current) {
      current.sessions.push(...project.sessions)
      current.session_count += project.session_count
      current.last_active = Math.max(current.last_active ?? 0, project.last_active ?? 0) || null
      continue
    }

    groups.set(key, {
      // 该 ID 只用于档案馆选择态，不参与任何引擎 IPC。
      id: `archive.${key}`,
      display_path: project.source_path ?? project.display_path,
      sessions: [...project.sessions],
      session_count: project.session_count,
      last_active: project.last_active,
    })
  }

  return [...groups.values()].sort((a, b) => (b.last_active ?? 0) - (a.last_active ?? 0))
})

function numericMeta(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : 0
}

function tokenUsageFromMeta(value: unknown): SessionSummary['subagent_tokens'] {
  const usage = value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {}
  return {
    input_tokens: numericMeta(usage.inputTokens),
    output_tokens: numericMeta(usage.outputTokens),
    cache_creation_input_tokens: numericMeta(usage.cacheCreationInputTokens),
    cache_read_input_tokens: numericMeta(usage.cachedInputTokens),
  }
}

function epochSeconds(value: string | null): number | null {
  if (!value) return null
  const milliseconds = Date.parse(value)
  return Number.isFinite(milliseconds) ? milliseconds / 1000 : null
}

// 数据代际：每次增量变异 +1。全量拉取在途期间若有增量落地，
// 扫描结果可能比已落地的增量陈旧，检测到代际变化则重拉一次（无条件应用，避免循环）
let dataGen = 0

/** 静默重新加载（不显示 loading 状态） */
async function reloadProjectsSilently() {
  try {
    const genAtStart = dataGen
    const result = await loadEngineProjects()
    if (dataGen !== genAtStart) {
      projects.value = await loadEngineProjects()
    } else {
      projects.value = result
    }
    projectsRevision.value++
  } catch (_) {
    // 静默失败
  }
}

/** 按会话增量更新项目树；遇到未知项目（新建项目目录）回退全量 */
async function applySessionChanges(changes: SessionChange[]) {
  for (const { projectId, sessionId } of changes) {
    const proj = projects.value.find(p =>
      (p.native_id ?? p.id) === projectId
      && (!p.engine || usesNativeSessionSurface(p.engine)))
    if (!proj) {
      reloadProjectsSilently()
      return
    }
    try {
      const summary = await invoke<SessionSummary | null>('get_session_summary', {
        projectId,
        sessionId,
      })
      dataGen++
      const idx = proj.sessions.findIndex(s => (s.native_id ?? s.id) === sessionId)
      if (!summary) {
        // 会话文件已删除
        if (idx >= 0) proj.sessions.splice(idx, 1)
        if (proj.sessions.length === 0) {
          // 与全量扫描一致：零会话项目不展示
          const pIdx = projects.value.findIndex(p => p.id === projectId)
          if (pIdx >= 0) projects.value.splice(pIdx, 1)
          continue
        }
      } else if (idx >= 0) {
        proj.sessions[idx] = {
          ...summary,
          reference: proj.sessions[idx].reference,
          project_reference: proj.sessions[idx].project_reference,
          engine: proj.sessions[idx].engine,
          engine_name: proj.sessions[idx].engine_name,
          native_id: summary.id,
        }
      } else {
        const engine = proj.engine
        const projectReference = proj.reference
        if (!engine || !projectReference) {
          reloadProjectsSilently()
          return
        }
        proj.sessions.push({
          ...summary,
          reference: { engine, nativeId: summary.id },
          project_reference: projectReference,
          engine,
          engine_name: proj.engine_name,
          native_id: summary.id,
        })
      }
      proj.session_count = proj.sessions.length
      proj.sessions.sort((a, b) => (b.last_modified ?? 0) - (a.last_modified ?? 0))
      proj.last_active = proj.sessions[0]?.last_modified ?? proj.last_active
    } catch (_) {
      // 单条失败不阻塞其余变更
    }
  }
  projects.value.sort((a, b) => (b.last_active ?? 0) - (a.last_active ?? 0))
  projectsRevision.value++
}

/** 切换项目选中状态(单选：点已选中的取消，点未选中的替换) */
function toggleProject(id: string) {
  selectedProjectIds.value = selectedProjectIds.value.has(id)
    ? new Set()
    : new Set([id])
}

/** 全选/全不选 */
function selectAllProjects(select: boolean) {
  if (select) {
    selectedProjectIds.value = new Set(archiveProjects.value.map(p => p.id))
  } else {
    selectedProjectIds.value = new Set()
  }
}

function engineFilterId(project: Project): string {
  return project.engine ? `${project.engine.engineId}/${project.engine.instanceId}` : 'legacy/default'
}

function toggleEngine(id: string) {
  const next = new Set(selectedEngineIds.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  selectedEngineIds.value = next
}

const engineOptions = computed(() => {
  const values = new Map<string, string>()
  for (const project of projects.value) {
    values.set(engineFilterId(project), project.engine_name ?? project.engine?.engineId ?? '—')
  }
  return [...values].map(([id, name]) => ({ id, name }))
})

/** 选中项目的会话（无选中时显示全部） */
const filteredSessions = computed<SessionSummary[]>(() => {
  const ids = selectedProjectIds.value
  const projectSource = ids.size > 0
    ? archiveProjects.value.filter(p => ids.has(p.id))
    : archiveProjects.value
  const engineIds = selectedEngineIds.value
  const sessions = projectSource.flatMap(p => p.sessions)
  if (engineIds.size === 0) return sessions
  return sessions.filter(session => {
    const engineId = session.engine
      ? `${session.engine.engineId}/${session.engine.instanceId}`
      : 'legacy/default'
    return engineIds.has(engineId)
  })
})

/** 侧边栏统计 */
const sidebarStats = computed(() => {
  const ps = archiveProjects.value
  const totalSessions = ps.reduce((sum, p) => sum + p.session_count, 0)
  const totalSize = ps.reduce(
    (sum, p) => sum + p.sessions.reduce((s, sess) => s + sess.file_size, 0),
    0,
  )
  return {
    projectCount: ps.length,
    sessionCount: totalSessions,
    totalSize,
  }
})

/** 会话列表统计（基于筛选后的会话） */
const sessionStats = computed(() => {
  const sessions = filteredSessions.value
  const totalTokens = sessions.reduce(
    (sum, s) => sum + tokenTotal(s.total_tokens),
    0,
  )
  const totalSize = sessions.reduce((sum, s) => sum + s.file_size, 0)
  // 活跃天数：去重日期
  const days = new Set(
    sessions
      .filter(s => s.last_modified)
      .map(s => new Date(s.last_modified * 1000).toDateString()),
  )
  return {
    sessionCount: sessions.length,
    totalTokens,
    totalSize,
    activeDays: days.size,
  }
})

export function useProjects() {
  return {
    projects,
    archiveProjects,
    projectsRevision,
    selectedProjectIds,
    selectedEngineIds,
    loading,
    error,
    loadProjects,
    toggleProject,
    selectAllProjects,
    toggleEngine,
    engineOptions,
    filteredSessions,
    sidebarStats,
    sessionStats,
  }
}
