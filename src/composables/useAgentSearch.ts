import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { SearchResult } from './useSearch'

interface SmartSearchResult extends SearchResult {
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
    const r = await invoke<SmartSearchResult>('smart_search', {
      question,
      filter: { projectId: null, days: null, titleOnly: false },
    })
    agentTermGroups.value = r.termGroups ?? []
    agentSummary.value = r.summary ?? null
    agentResult.value = r
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
