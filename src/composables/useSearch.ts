import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSessions } from './useSessions'
import { useUiState } from './useUiState'
import { listEngines } from '@/engines/client'
import { instanceKey } from '@/engines/identity'
import { projectUiId, sessionUiId } from '@/engines/integration'
import { resolveSession } from '@/engines/directory'
import type { EngineInstanceId, SessionRef } from '@/engines/types'

/** 全局搜索状态（模块级单例，同 useHomeStats 模式）*/

export interface SearchSnippet {
  uuid: string | null
  role: number // 0 = user, 1 = assistant
  timestamp: string | null
  text: string
}

export interface SearchHit {
  sessionId: string
  projectId: string
  title: string | null
  lastModified: number
  matchedIn: string[]
  totalMatches: number
  snippets: SearchSnippet[]
  engineName?: string
  engineInstance?: EngineInstanceId
}

export interface SearchResult {
  hits: SearchHit[]
  totalHits: number
  elapsedMs: number
}

export interface SearchStatus {
  state: 'building' | 'ready'
  indexedSessions: number
  totalSessions: number
}

const DEBOUNCE_MS = 300

const query = ref('')
const days30 = ref(false)
const titleOnly = ref(false)
const projectFilter = ref<string | null>(null)
const engineFilter = ref<string | null>(null)

const result = ref<SearchResult | null>(null)
const searching = ref(false)
const searchError = ref<string | null>(null)
const indexStatus = ref<SearchStatus | null>(null)

/** 跳转档案馆后待定位的命中消息（SessionDetail 消费后置 null）*/
const pendingScrollTarget = ref<{ sessionId: string; uuid: string } | null>(null)

let debounceTimer: ReturnType<typeof setTimeout> | null = null
let seq = 0

async function runSearch() {
  const q = query.value.trim()
  if (!q) {
    result.value = null
    searching.value = false
    return
  }
  const mySeq = ++seq
  const startedAt = performance.now()
  searching.value = true
  searchError.value = null
  try {
    const descriptors = await listEngines()
    const selected = descriptors.filter(descriptor => descriptor.enabled
      && (!engineFilter.value || instanceKey(descriptor.instance) === engineFilter.value))
    const tasks: Array<Promise<SearchHit[]>> = []
    for (const descriptor of selected) {
      tasks.push(invoke<Array<{ session: SessionRef; title: string | null; snippet: string }>>('engine_search', {
        query: { text: q, instance: descriptor.instance, limit: 100 },
      }).then(hits => hits.flatMap(hit => {
        const id = sessionUiId(hit.session)
        const summary = resolveSession(id)
        if (projectFilter.value && (!summary?.project_reference || projectUiId(summary.project_reference) !== projectFilter.value)) return []
        if (days30.value && summary && summary.last_modified * 1000 < Date.now() - 30 * 86400_000) return []
        if (titleOnly.value && !(hit.title ?? '').toLocaleLowerCase().includes(q.toLocaleLowerCase())) return []
        return [{
          sessionId: id,
          projectId: summary?.project_reference ? projectUiId(summary.project_reference) : '',
          title: hit.title,
          lastModified: summary?.last_modified ?? 0,
          matchedIn: ['content'],
          totalMatches: 1,
          snippets: [{ uuid: null, role: 1, timestamp: null, text: hit.snippet }],
          engineName: descriptor.displayName,
          engineInstance: descriptor.instance,
        }]
      })))
    }
    const settled = await Promise.allSettled(tasks)
    if (settled.length > 0 && settled.every(item => item.status === 'rejected')) {
      throw (settled[0] as PromiseRejectedResult).reason
    }
    const hits = settled.flatMap(item => item.status === 'fulfilled' ? item.value : [])
      .sort((left, right) => right.lastModified - left.lastModified)
    const r: SearchResult = { hits, totalHits: hits.length, elapsedMs: Math.round(performance.now() - startedAt) }
    if (mySeq !== seq) return // 竞态：只接受最新请求
    result.value = r
    // query 内部懒热(首查即首建),搜完顺手刷状态让"构建中"标签自愈为就绪
    if (indexStatus.value?.state !== 'ready') refreshStatus()
  } catch (e) {
    if (mySeq !== seq) return
    searchError.value = String(e)
  } finally {
    if (mySeq === seq) searching.value = false
  }
}

// as-you-type：查询词防抖；过滤器变化即时
watch(query, () => {
  if (debounceTimer) clearTimeout(debounceTimer)
  debounceTimer = setTimeout(runSearch, DEBOUNCE_MS)
})
watch([days30, titleOnly, projectFilter, engineFilter], runSearch)

async function refreshStatus() {
  try {
    indexStatus.value = await invoke<SearchStatus>('search_status')
  } catch (_) {}
}

/** 结果卡点击：跳档案馆打开会话，可选定位到命中消息 */
function goToHit(hit: SearchHit, uuid?: string | null) {
  const { selectSession } = useSessions()
  const { switchSection } = useUiState()
  pendingScrollTarget.value = uuid ? { sessionId: hit.sessionId, uuid } : null
  selectSession(hit.sessionId)
  switchSection('sessions')
}

export function useSearch() {
  return {
    query,
    days30,
    titleOnly,
    projectFilter,
    engineFilter,
    result,
    searching,
    searchError,
    indexStatus,
    pendingScrollTarget,
    runSearch,
    refreshStatus,
    goToHit,
  }
}
