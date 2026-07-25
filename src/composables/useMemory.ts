import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { MemoryOverview, MemoryProject, MemoryEntry, MemoryDetail, MemoryType } from '@/types'

/**
 * 记忆数据源（v2.9.0 FR-006/FR-007/FR-008/FR-009）：
 * 模块级单例。overview 惰性加载 + 手动刷新；当前项目选择；type 筛选；
 * 当前选中记忆；详情加载；体检推导（孤儿/悬空/断链）。
 */

// ===== 常量 =====
export const MEMORY_STALE_DAYS = 90

// ===== 模块级状态（单例） =====
const overview = ref<MemoryOverview | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

// 当前选择的项目 projectDir
const selectedProjectDir = ref<string | null>(null)
// type 筛选
const filterType = ref<MemoryType | null>(null)
// 当前选中的文件
const selectedFile = ref<string | null>(null)
// 详情
const detail = ref<MemoryDetail | null>(null)
const detailLoading = ref(false)
const detailError = ref<string | null>(null)

let loadedOnce = false

// ===== 加载 =====
async function load() {
  loading.value = true
  error.value = null
  try {
    overview.value = await invoke<MemoryOverview>('get_memory_overview')
    // 默认选中 lastModified 最大的项目
    if (!selectedProjectDir.value && overview.value.projects.length > 0) {
      const sorted = [...overview.value.projects].sort((a, b) => b.lastModified - a.lastModified)
      selectedProjectDir.value = sorted[0].projectDir
    }
    // 重扫后选中项可能已在外部被删除/改名 —— 清掉，否则详情区留着幽灵内容
    if (selectedFile.value && !currentProject.value?.entries.some(e => e.file === selectedFile.value)) {
      selectedFile.value = null
      detail.value = null
    }
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

function ensureLoaded() {
  if (loadedOnce) return
  loadedOnce = true
  load()
}

/**
 * 进入记忆子页时调用：每次都重扫。
 * 记忆是外部可变数据（CLI 写、用户手改、其他项目的会话写），
 * 一次性快照必然陈旧；记忆库通常百来个小文件，全扫走 spawn_blocking 仅数十毫秒。
 */
function ensureFresh() {
  loadedOnce = true
  if (loading.value) return
  load()
}

async function refresh() {
  if (loading.value) return
  await load()
}

// ===== 当前项目 =====
const currentProject = computed<MemoryProject | null>(() => {
  if (!overview.value || !selectedProjectDir.value) return null
  return overview.value.projects.find(p => p.projectDir === selectedProjectDir.value) ?? null
})

// ===== 筛选后的 entries =====
const filteredEntries = computed<MemoryEntry[]>(() => {
  if (!currentProject.value) return []
  const entries = currentProject.value.entries
  if (!filterType.value) return entries
  return entries.filter(e => e.type === filterType.value)
})

// ===== type 计数 =====
const typeCounts = computed<Record<MemoryType | 'all', number>>(() => {
  const entries = currentProject.value?.entries ?? []
  const counts: Record<string, number> = { all: entries.length, project: 0, feedback: 0, user: 0, reference: 0, unknown: 0 }
  for (const e of entries) {
    counts[e.type] = (counts[e.type] ?? 0) + 1
  }
  return counts as Record<MemoryType | 'all', number>
})

// ===== 体检推导 =====

/** 孤儿文件：磁盘有、MEMORY.md 无索引（legacyIndex 时豁免） */
const orphanFiles = computed<MemoryEntry[]>(() => {
  const proj = currentProject.value
  if (!proj || proj.legacyIndex) return []
  return proj.entries.filter(e => !e.indexed)
})

/** 悬空引用：MEMORY.md 索引了但磁盘无文件 */
const danglingRefs = computed<string[]>(() => {
  return currentProject.value?.danglingRefs ?? []
})

/** wiki-link 断链：entry 的 wikiLinks 中目标不存在于本项目 entries */
interface BrokenLink {
  sourceFile: string
  slug: string
}
const brokenWikiLinks = computed<BrokenLink[]>(() => {
  const proj = currentProject.value
  if (!proj) return []
  // 本项目所有文件名去 .md 后的 slug 集合
  const slugSet = new Set(proj.entries.map(e => e.file.replace(/\.md$/, '')))
  const broken: BrokenLink[] = []
  for (const entry of proj.entries) {
    for (const slug of entry.wikiLinks) {
      if (!slugSet.has(slug)) {
        broken.push({ sourceFile: entry.file, slug })
      }
    }
  }
  return broken
})

/** 问题总数 */
const healthIssueCount = computed(() => {
  return orphanFiles.value.length + danglingRefs.value.length + brokenWikiLinks.value.length
})

// ===== 详情加载 =====
async function loadDetail(file: string) {
  if (!selectedProjectDir.value) return
  detailLoading.value = true
  detailError.value = null
  try {
    detail.value = await invoke<MemoryDetail>('get_memory_detail', {
      projectDir: selectedProjectDir.value,
      file,
    })
  } catch (e) {
    detailError.value = String(e)
  } finally {
    detailLoading.value = false
  }
}

// 选中文件变化时加载详情
watch(selectedFile, (file) => {
  detail.value = null
  detailError.value = null
  if (file) loadDetail(file)
})

// 切换项目时清空选中和详情
watch(selectedProjectDir, () => {
  selectedFile.value = null
  detail.value = null
  filterType.value = null
})

// ===== 选中记忆 entry =====
const selectedEntry = computed<MemoryEntry | null>(() => {
  if (!selectedFile.value || !currentProject.value) return null
  return currentProject.value.entries.find(e => e.file === selectedFile.value) ?? null
})

// ===== 保存 =====
async function saveMemory(content: string, expectedMtime: number): Promise<{ ok: boolean; error?: string }> {
  if (!selectedProjectDir.value || !selectedFile.value) return { ok: false, error: 'no_selection' }
  try {
    await invoke('save_memory', {
      projectDir: selectedProjectDir.value,
      file: selectedFile.value,
      content,
      expectedMtime,
    })
    // 保存成功后刷新 overview + 详情
    await refresh()
    if (selectedFile.value) await loadDetail(selectedFile.value)
    return { ok: true }
  } catch (e) {
    const msg = String(e)
    return { ok: false, error: msg }
  }
}

// ===== 删除 =====
async function deleteMemory(): Promise<{ ok: boolean; error?: string }> {
  if (!selectedProjectDir.value || !selectedFile.value) return { ok: false, error: 'no_selection' }
  try {
    await invoke('delete_memory', {
      projectDir: selectedProjectDir.value,
      file: selectedFile.value,
    })
    selectedFile.value = null
    detail.value = null
    await refresh()
    return { ok: true }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// ===== 打开 MEMORY.md =====
async function openMemoryIndex(): Promise<void> {
  if (!selectedProjectDir.value) return
  await invoke('open_memory_index', { projectDir: selectedProjectDir.value })
}

// ===== 总条数（供子导航计数；未加载为 null → 前端显示「…」） =====
const memoryCount = computed<number | null>(() => {
  if (!overview.value) return null
  return overview.value.projects.reduce((sum, p) => sum + p.count, 0)
})

// ===== Wiki-link 目标是否存在（供详情渲染用） =====
function isWikiLinkValid(slug: string): boolean {
  if (!currentProject.value) return false
  return currentProject.value.entries.some(e => e.file.replace(/\.md$/, '') === slug)
}

// ===== 导出 =====
// 注意：useMemory() 不自动加载——WorkshopView 常驻 mount（v-show 六视图架构），
// 调用即扫描会把 memory IO 提前到应用启动期。消费方在进入工坊域/子页时显式调 ensureLoaded()。
export function useMemory() {
  return {
    // 数据
    overview,
    loading,
    error,
    currentProject,
    filteredEntries,
    typeCounts,
    selectedProjectDir,
    filterType,
    selectedFile,
    selectedEntry,
    detail,
    detailLoading,
    detailError,
    // 体检
    orphanFiles,
    danglingRefs,
    brokenWikiLinks,
    healthIssueCount,
    // 动作
    ensureLoaded,
    ensureFresh,
    refresh,
    loadDetail,
    saveMemory,
    deleteMemory,
    openMemoryIndex,
    isWikiLinkValid,
    // 统计
    memoryCount,
  }
}
