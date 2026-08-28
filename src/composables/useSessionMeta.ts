import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { SessionRef } from '@/engines/types'
import { sessionUiId } from '@/engines/integration'
import { resolveSessionRef } from '@/engines/directory'
import { listEngines } from '@/engines/client'
import { useTagRegistry } from '@/composables/useTagRegistry'

export interface SessionMeta {
  title?: string
  deleted?: boolean
  deletedAt?: string
  tags?: string[]
  starred?: boolean
  tagsManual?: boolean
  titleManual?: boolean
  summary?: string
}

const metaMap = ref<Record<string, SessionMeta>>({})
const titleGenerating = ref<Set<string>>(new Set())
const turnCounts = new Map<string, number>()
let loaded = false

async function loadAll() {
  await listEngines()
  const entries = await invoke<Array<{ session: SessionRef; metadata: SessionMeta }>>('get_all_meta_v2')
  metaMap.value = Object.fromEntries(entries.map(entry => [sessionUiId(entry.session), entry.metadata]))
  loaded = true
}

function shouldRefresh(sessionId: string, manualKey?: keyof SessionMeta): boolean {
  const meta = metaMap.value[sessionId]
  if (manualKey && meta?.[manualKey]) return false
  const turn = turnCounts.get(sessionId) ?? 1
  if (turn <= 5) return true
  return turn % 5 === 0
}

async function refreshTitle(session: SessionRef) {
  const sessionId = sessionUiId(session)
  if (!shouldRefresh(sessionId, 'titleManual')) return
  if (titleGenerating.value.has(sessionId)) return
  titleGenerating.value = new Set([...titleGenerating.value, sessionId])
  try {
    const { title, turnCount } = await invoke<{ title: string, turnCount: number }>('generate_title', { session })
    if (!turnCounts.has(sessionId)) {
      turnCounts.set(sessionId, turnCount)
    }
    metaMap.value = { ...metaMap.value, [sessionId]: { ...metaMap.value[sessionId], title } }
  } catch (e) {
    console.warn('[meta] 标题生成失败:', sessionId, e)
  } finally {
    const next = new Set(titleGenerating.value)
    next.delete(sessionId)
    titleGenerating.value = next
  }
}

async function refreshTags(session: SessionRef) {
  const sessionId = sessionUiId(session)
  if (!shouldRefresh(sessionId, 'tagsManual')) return
  try {
    const result = await invoke<{ tags: string[], skipped: boolean }>('generate_tags', { session })
    metaMap.value = {
      ...metaMap.value,
      [sessionId]: { ...metaMap.value[sessionId], tags: result.tags },
    }
    if (!result.skipped) void useTagRegistry().loadTags(true)
  } catch (e) {
    console.warn('[meta] 标签生成失败:', sessionId, e)
  }
}

async function refreshSummary(session: SessionRef, force = false): Promise<string | undefined> {
  const sessionId = sessionUiId(session)
  if (!force && !shouldRefresh(sessionId)) return undefined
  try {
    const summary = await invoke<string>('generate_summary', { session })
    metaMap.value = { ...metaMap.value, [sessionId]: { ...metaMap.value[sessionId], summary } }
    return summary
  } catch (e) {
    console.warn('[meta] 摘要生成失败:', sessionId, e)
    if (force) throw e
    return undefined
  }
}

/** 用户发送消息后调用——异步生成/修订标题、标签、摘要，不阻塞发送流程 */
export function triggerMetaGeneration(session: SessionRef) {
  const sessionId = sessionUiId(session)
  const turn = (turnCounts.get(sessionId) ?? 0) + 1
  turnCounts.set(sessionId, turn)
  refreshTitle(session)
  refreshTags(session)
  refreshSummary(session)
}

export function useSessionMeta() {
  if (!loaded) loadAll()

  function getMeta(sessionId: string): SessionMeta | undefined {
    return metaMap.value[sessionId]
  }

  async function updateMeta(sessionId: string, patch: SessionMeta, explicitSession?: SessionRef) {
    const nextPatch = { ...patch }
    if (nextPatch.title !== undefined) {
      nextPatch.titleManual = true
    }
    const session = explicitSession ?? resolveSessionRef(sessionId)
    const updated = session
      ? await invoke<SessionMeta>('update_meta_v2', { session, patch: nextPatch })
      : await invoke<SessionMeta>('update_meta', { sessionId, patch: nextPatch })
    metaMap.value = { ...metaMap.value, [sessionId]: updated }
    if (nextPatch.tags !== undefined) void useTagRegistry().loadTags(true)
    return updated
  }

  async function updateTags(sessionId: string, tags: string[], explicitSession?: SessionRef) {
    const session = explicitSession ?? resolveSessionRef(sessionId)
    if (!session) return updateMeta(sessionId, { tags })
    const updated = await invoke<SessionMeta>('update_session_tags', { session, tags })
    metaMap.value = { ...metaMap.value, [sessionId]: updated }
    void useTagRegistry().loadTags(true)
    return updated
  }

  return {
    metaMap,
    getMeta,
    updateMeta,
    updateTags,
    reloadMeta: loadAll,
    refreshTitle,
    refreshTags,
    refreshSummary,
    titleGenerating,
  }
}
