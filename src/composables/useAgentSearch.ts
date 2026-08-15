import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { SearchResult } from './useSearch'
import { listEngines } from '@/engines/client'
import { resolveSession } from '@/engines/directory'
import { instanceKey } from '@/engines/identity'
import { projectUiId, sessionUiId } from '@/engines/integration'
import type { SessionRef } from '@/engines/types'

interface SmartSearchResult {
  hits: Array<{ session: SessionRef; title: string | null; snippet: string }>
  totalHits: number
  elapsedMs: number
  termGroups: string[]
  summary: string | null
}

const agentResult = ref<SearchResult | null>(null)
const agentSearching = ref(false)
const agentError = ref<string | null>(null)
const agentTermGroups = ref<string[]>([])
const agentSummary = ref<string | null>(null)

/** Agent 生成的所有关键词（去重拍扁，供高亮用） */
const agentAllTerms = computed(() => {
  const set = new Set<string>()
  for (const group of agentTermGroups.value) {
    for (const w of group.split(/\s+/).filter(Boolean)) set.add(w)
  }
  return [...set]
})

async function startAgentSearch(question: string) {
  if (!question.trim()) return
  agentSearching.value = true
  agentError.value = null
  agentResult.value = null
  agentTermGroups.value = []
  agentSummary.value = null
  try {
    const [r, descriptors] = await Promise.all([
      invoke<SmartSearchResult>('smart_search', { question }),
      listEngines(),
    ])
    const engineNames = new Map(descriptors.map(descriptor => [
      instanceKey(descriptor.instance),
      descriptor.displayName,
    ]))
    const hits = r.hits.map(hit => {
      const sessionId = sessionUiId(hit.session)
      const session = resolveSession(sessionId)
      return {
        sessionId,
        projectId: session?.project_reference ? projectUiId(session.project_reference) : '',
        title: hit.title,
        lastModified: session?.last_modified ?? 0,
        matchedIn: ['content'],
        totalMatches: 1,
        snippets: [{ uuid: null, role: 1 as const, timestamp: null, text: hit.snippet }],
        engineName: engineNames.get(instanceKey(hit.session.engine)) ?? hit.session.engine.engineId,
        engineInstance: hit.session.engine,
      }
    }).sort((left, right) => right.lastModified - left.lastModified)
    agentTermGroups.value = r.termGroups ?? []
    agentSummary.value = r.summary ?? null
    agentResult.value = { hits, totalHits: r.totalHits, elapsedMs: r.elapsedMs }
  } catch (e) {
    agentError.value = String(e)
  } finally {
    agentSearching.value = false
  }
}

export function useAgentSearch() {
  return {
    agentResult,
    agentSearching,
    agentError,
    agentTermGroups,
    agentAllTerms,
    agentSummary,
    startAgentSearch,
  }
}
