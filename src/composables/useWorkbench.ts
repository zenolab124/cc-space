import { ref, computed, watch, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import i18n from '../locales'
import { evictSessionTransients } from './useStreaming'
import { useRunners } from './useRunners'
import { fillColumnWidthsProportionally } from '@/utils/workbenchColumnLayout'
import { readMigratedStorage } from '../utils/storageMigrate'
import { resolveSessionRef } from '@/engines/directory'
import type { ProjectRef, SessionRef } from '@/engines/types'
import { clearEngineRunConfig } from '@/engines/runConfig'

/**
 * 工作台状态模型（v2.1.0 FR-001/002/004 + NFR-002）
 *
 * 核心心智:在工作台 = 激活,与运行状态无关;会话进出全显式。
 * 列与会话解耦（对比组口子,硬约束）:列对象引用会话 id 并预留 type 字段,
 * 本版唯一取值 'session',禁止把列实现为会话对象本身的属性。
 * 会话归属唯一:一个会话同一时刻至多属于一个工作台 tab。
 */

export interface WorkbenchColumn {
  id: string
  type: 'session'
  sessionId: string
  /** 展开序号(全局递增):软上限置换时收回最小者 */
  openedSeq: number
}

export interface RaceLane {
  id: string
  sessionId: string
  label: string
}

export interface RaceConfig {
  cwd: string
  lanes: RaceLane[]
  /** 首条赛马广播成功投递后锁定；已有会话不支持跨引擎热切换。 */
  engineSwitchLocked: boolean
}

export interface WorkbenchTab {
  id: string
  name: string
  /** 左列会话,加入顺序即显示顺序(任何状态变化不重排) */
  sessionIds: string[]
  /** 右区展开列(数组序 = 列序,可拖拽重排) */
  columns: WorkbenchColumn[]
  /** 各列像素宽度,与 columns 平行 */
  columnSizes: number[]
  /** 赛马模式配置。非 undefined 即赛马 Tab */
  race?: RaceConfig
}

export interface EngineDraft {
  reference: SessionRef
  project: ProjectRef
  engineName: string
  cwd: string
  /** thread/start / fork 时实际附着的渠道；避免空线程被重复 resume。 */
  attachedChannel: string | null
  /** thread/start / fork 时实际附着的受控能力；避免空线程被重复 resume。 */
  attachedCapabilityFingerprint?: string
  /** 仅当前 WebView/后端运行期有效；空线程尚未落盘，跨重启无法 resume。 */
  runtimeScope: string
}

interface WorkbenchState {
  tabs: WorkbenchTab[]
  activeTabId: string
  /** 「工作台 N」的 N:历史递增,关闭 tab 不回收 */
  tabSeq: number
  /** 展开序号计数 */
  openSeq: number
  /** 已选工作目录、尚未选择运行引擎的新任务占位(sessionId → cwd)。 */
  pendingTasks: Record<string, string>
  /**
   * 应用内新建、尚未落盘的草稿会话(sessionId → cwd)。
   * 首条消息经 CLI --session-id 落盘后由 pruneDrafts 清理;
   * 落盘前各视图据此合成「新会话」占位显示。
   */
  drafts: Record<string, string>
  /** 已由通用 runtime 创建、但 source 尚未返回摘要的新会话。 */
  engineDrafts: Record<string, EngineDraft>
  /**
   * 分叉意图(分叉出的 sessionId → 源 sessionId)。分叉不再预复制 JSONL,
   * 首条消息由 Rust 端以 --resume 源 --fork-session --session-id 新 spawn,
   * 落盘由 CLI 完成(历史行 sessionId 重写);落盘前垫底渲染源会话历史,
   * 落盘后随 pruneDrafts 一并收割
   */
  forkIntents: Record<string, string>
}

const STORAGE_KEY = 'monet-workbench'
const MIN_WIDTH_KEY = 'monet-min-column-width'
const LEGACY_STORAGE_KEY = 'cc-space-workbench' // 旧 key,一次性迁移读取用
const LEGACY_MIN_WIDTH_KEY = 'cc-space-min-column-width' // 旧 key,一次性迁移读取用
const DEFAULT_MIN_COLUMN_WIDTH = 360
const ABSOLUTE_MIN_COLUMN_WIDTH = 200

// performance.timeOrigin 属于当前页面生命周期：同一窗口内稳定，App 重启后更新。
// 不用 sessionStorage，WebKit 会跨 App 进程恢复它，无法识别已经失效的空线程。
const engineDraftRuntimeScope = String(performance.timeOrigin)

const minColumnWidth = ref(
  Math.max(ABSOLUTE_MIN_COLUMN_WIDTH, Number(readMigratedStorage(MIN_WIDTH_KEY, LEGACY_MIN_WIDTH_KEY)) || DEFAULT_MIN_COLUMN_WIDTH)
)

/** 右区四周边距与列间隙(与 WorkbenchColumns 的 PAD/GAP 一致) */
const COLUMN_GAP = 10

/**
 * 右区容器实测宽度:WorkbenchColumns 挂载后经 ResizeObserver 维护。
 * v-show 隐藏报 0 不更新(保留最后有效值);初始按窗口减 ActivityBar(48)+左列(256) 估算。
 */
const rightZoneWidth = ref(Math.max(minColumnWidth.value, window.innerWidth - 48 - 256))

export function setRightZoneWidth(w: number) {
  if (w <= 0) return
  rightZoneWidth.value = w
}

// 列宽自动放大只跟随窗口变大,不跟随容器变宽:
// 监控栏折叠等侧栏开合同样会让容器变宽,若据此放大并落盘列宽,
// 栏展开回来时没有对称的缩回逻辑,列区从此永久溢出。
// 侧栏开合只改变可视视口,持久化列宽保持不变。
let lastWindowWidth = window.innerWidth
window.addEventListener('resize', () => {
  const w = window.innerWidth
  if (w > lastWindowWidth) nextTick(redistributeOnGrow)
  lastWindowWidth = w
})

function containerFreeWidth(n: number): number {
  return rightZoneWidth.value - COLUMN_GAP * Math.max(0, n - 1) - COLUMN_GAP * 2
}

/** 窗口变大时,按比例放大各列填满(仅当前全部列已 fit 时触发) */
function redistributeOnGrow() {
  for (const tab of state.value.tabs) {
    if (tab.columns.length === 0) continue
    const free = containerFreeWidth(tab.columns.length)
    const total = tab.columnSizes.reduce((s, w) => s + w, 0)
    if (total <= 0 || total > free) continue
    const scale = free / total
    tab.columnSizes = tab.columnSizes.map(w => Math.max(minColumnWidth.value, Math.round(w * scale)))
  }
}

let idCounter = 0
function genId(prefix: string) {
  return `${prefix}-${++idCounter}-${Date.now().toString(36)}`
}

function equalSizes(n: number): number[] {
  if (n <= 0) return []
  const free = containerFreeWidth(n)
  const w = Math.max(minColumnWidth.value, Math.round(free / n))
  return Array(n).fill(w)
}

function createTabObject(seq: number): WorkbenchTab {
  return {
    id: genId('wbtab'),
    name: i18n.global.t('workbench.defaultTabName', { seq }),
    sessionIds: [],
    columns: [],
    columnSizes: [],
  }
}

function createInitialState(): WorkbenchState {
  const tab = createTabObject(1)
  return {
    tabs: [tab],
    activeTabId: tab.id,
    tabSeq: 1,
    openSeq: 0,
    pendingTasks: {},
    drafts: {},
    engineDrafts: {},
    forkIntents: {},
  }
}

// ---- 持久化(NFR-002):任一变更后同步落盘;损坏时回退默认并提示 ----

function saveState() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state.value))
  } catch (_) {}
}

/** 修复历史版本落盘的畸形解码 cwd（`/X//rest` → `X:\rest`）：
 *  旧版把 Windows 编码目录名按 Unix 规则解码后持久化,存量草稿/赛道不清洗则
 *  发送时 spawn 仍报「工作目录不存在」,看似未修复 */
function sanitizeCwd(cwd: string): string {
  const m = cwd.match(/^\/([A-Za-z])\/\/(.*)$/)
  return m ? `${m[1]}:\\${m[2].replace(/\//g, '\\')}` : cwd
}

/** 本次启动修复的不一致处数(>0 时 App 弹「已修复」提示而非重置) */
export let stateRepairCount = 0

/** 单 tab 结构校验 + 局部修复:骨架非法返回 null(丢弃该 tab),
 *  列/lane 引用不一致则剔除该列/lane 并计数——一处孤儿列不应废掉全部工作台(#8) */
function repairTab(t: WorkbenchTab): WorkbenchTab | null {
  if (!t || typeof t.id !== 'string' || typeof t.name !== 'string') return null
  if (!Array.isArray(t.sessionIds) || !Array.isArray(t.columns) || !Array.isArray(t.columnSizes)) return null
  if (t.sessionIds.some(sid => typeof sid !== 'string')) return null

  // 列骨架非法 → 丢弃单列;列引用的会话不在左列 → 孤儿列,剔除(#7 产生的脏数据)
  const validColumns = t.columns.filter((c, i) => {
    const structOk = c && c.type === 'session' && typeof c.id === 'string'
      && typeof c.sessionId === 'string' && typeof c.openedSeq === 'number'
    const sizeOk = typeof t.columnSizes[i] === 'number' && Number.isFinite(t.columnSizes[i]) && t.columnSizes[i] >= 0
    const memberOk = structOk && t.sessionIds.includes(c.sessionId)
    return structOk && sizeOk && memberOk
  })
  if (validColumns.length !== t.columns.length || t.columnSizes.length !== t.columns.length) {
    stateRepairCount += t.columns.length - validColumns.length || 1
    t.columnSizes = validColumns.map((c) => {
      const oi = t.columns.indexOf(c)
      const s = t.columnSizes[oi]
      return typeof s === 'number' && Number.isFinite(s) && s >= 0 ? s : 0
    })
    t.columns = validColumns
  }

  if (t.race !== undefined) {
    if (!t.race || typeof t.race !== 'object' || typeof t.race.cwd !== 'string' || !t.race.cwd
      || !Array.isArray(t.race.lanes)) {
      delete t.race
      stateRepairCount += 1
    } else {
      t.race.cwd = sanitizeCwd(t.race.cwd)
      // 旧版本没有锁字段。恢复中的赛马无法判断是否已经广播过，按安全侧锁定。
      t.race.engineSwitchLocked = typeof t.race.engineSwitchLocked === 'boolean'
        ? t.race.engineSwitchLocked
        : true
      const validLanes = t.race.lanes.filter(lane =>
        lane && typeof lane.id === 'string' && typeof lane.sessionId === 'string'
        && typeof lane.label === 'string'
        && t.sessionIds.includes(lane.sessionId)
        && t.columns.some(c => c.sessionId === lane.sessionId))
      if (validLanes.length !== t.race.lanes.length) {
        stateRepairCount += t.race.lanes.length - validLanes.length
        t.race.lanes = validLanes
      }
      if (t.race.lanes.length === 0) delete t.race
    }
  }
  return t
}

/** 反序列化 + 逐 tab 修复:能救则救,全部 tab 均不可救才作废整体(触发重置) */
function loadState(): WorkbenchState | null {
  try {
    const raw = readMigratedStorage(STORAGE_KEY, LEGACY_STORAGE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<WorkbenchState>
    if (!parsed || typeof parsed !== 'object') return null
    if (!Array.isArray(parsed.tabs) || parsed.tabs.length < 1) return null
    if (typeof parsed.activeTabId !== 'string') return null

    const tabs = (parsed.tabs as WorkbenchTab[]).map(repairTab).filter((t): t is WorkbenchTab => t !== null)
    if (tabs.length === 0) return null
    if (tabs.length !== parsed.tabs.length) stateRepairCount += parsed.tabs.length - tabs.length

    let activeTabId = parsed.activeTabId
    if (!tabs.some(t => t.id === activeTabId)) {
      activeTabId = tabs[0].id
      stateRepairCount += 1
    }
    const pendingTasks: Record<string, string> = {}
    if (parsed.pendingTasks && typeof parsed.pendingTasks === 'object' && !Array.isArray(parsed.pendingTasks)) {
      for (const [key, value] of Object.entries(parsed.pendingTasks)) {
        if (typeof value === 'string' && value) pendingTasks[key] = sanitizeCwd(value)
      }
    }
    // drafts 为 v2.1.x 增量字段:旧数据缺省为 {},值非法则丢弃单条不作废整体
    const drafts: Record<string, string> = {}
    if (parsed.drafts && typeof parsed.drafts === 'object' && !Array.isArray(parsed.drafts)) {
      for (const [k, v] of Object.entries(parsed.drafts)) {
        if (typeof v === 'string' && v) drafts[k] = sanitizeCwd(v)
      }
    }
    const engineDrafts: Record<string, EngineDraft> = {}
    if (parsed.engineDrafts && typeof parsed.engineDrafts === 'object' && !Array.isArray(parsed.engineDrafts)) {
      for (const [key, value] of Object.entries(parsed.engineDrafts)) {
        const draft = value as Partial<EngineDraft>
        if (draft && typeof draft.cwd === 'string' && typeof draft.engineName === 'string'
          && draft.reference && typeof draft.reference.nativeId === 'string'
          && draft.reference.engine && typeof draft.reference.engine.engineId === 'string'
          && typeof draft.reference.engine.instanceId === 'string'
          && draft.project && typeof draft.project.nativeId === 'string') {
          engineDrafts[key] = {
            ...draft,
            cwd: sanitizeCwd(draft.cwd),
            attachedChannel: typeof draft.attachedChannel === 'string' ? draft.attachedChannel : null,
            attachedCapabilityFingerprint: typeof draft.attachedCapabilityFingerprint === 'string'
              ? draft.attachedCapabilityFingerprint
              : undefined,
            runtimeScope: typeof draft.runtimeScope === 'string' ? draft.runtimeScope : '',
          } as EngineDraft
        }
      }
    }
    // forkIntents 同为增量字段,同款宽松解析
    const forkIntents: Record<string, string> = {}
    if (parsed.forkIntents && typeof parsed.forkIntents === 'object' && !Array.isArray(parsed.forkIntents)) {
      for (const [k, v] of Object.entries(parsed.forkIntents)) {
        if (typeof v === 'string' && v) forkIntents[k] = v
      }
    }
    return {
      tabs,
      activeTabId,
      tabSeq: typeof parsed.tabSeq === 'number' ? parsed.tabSeq : tabs.length,
      openSeq: typeof parsed.openSeq === 'number' ? parsed.openSeq : 0,
      pendingTasks,
      drafts,
      engineDrafts,
      forkIntents,
    }
  } catch (_) {
    return null
  }
}

const loaded = loadState()

// ratio → pixel 迁移:旧版 columnSizes 为比例(均 < minColumnWidth.value),转为像素宽度
if (loaded) {
  for (const tab of loaded.tabs) {
    if (tab.columns.length > 0 && tab.columnSizes.length > 0 && Math.max(...tab.columnSizes) < minColumnWidth.value) {
      const free = Math.max(minColumnWidth.value, window.innerWidth - 48 - 256) - COLUMN_GAP * Math.max(0, tab.columns.length - 1) - COLUMN_GAP * 2
      tab.columnSizes = tab.columnSizes.map(r => Math.max(minColumnWidth.value, Math.round(r * free)))
    }
  }
}

/** 持久化损坏被重置(App 启动后弹瞬态 toast「工作台状态已重置」) */
export const stateWasReset = !!localStorage.getItem(STORAGE_KEY) && !loaded

// 重置前把原始快照备份留档,给用户/后续版本留恢复可能
if (stateWasReset) {
  try {
    localStorage.setItem(`${STORAGE_KEY}.corrupt-backup`, localStorage.getItem(STORAGE_KEY)!)
  } catch (_) {}
}

const state = ref<WorkbenchState>(loaded || createInitialState())

watch(state, saveState, { deep: true })

// ---- 派生 ----

const activeTab = computed<WorkbenchTab>(() => {
  return state.value.tabs.find(t => t.id === state.value.activeTabId) ?? state.value.tabs[0]
})

/** 重复打开时的高亮目标(背景闪烁 1 秒) */
const flashSessionId = ref<string | null>(null)
let flashTimer: number | null = null

function flashSession(sessionId: string) {
  flashSessionId.value = sessionId
  if (flashTimer !== null) clearTimeout(flashTimer)
  flashTimer = window.setTimeout(() => {
    flashSessionId.value = null
    flashTimer = null
  }, 1000)
}

/** 右区滚动聚焦请求(已展开列的幂等展开;消费方为 WorkbenchColumns) */
const focusColumnRequest = ref<{ sessionId: string; seq: number } | null>(null)
let focusSeq = 0

function requestFocusColumn(sessionId: string) {
  focusColumnRequest.value = { sessionId, seq: ++focusSeq }
}

// ---- 查询 ----

/** 查会话归属(唯一性):返回所在 tab 与是否已展开 */
function findSession(sessionId: string): { tab: WorkbenchTab; expanded: boolean } | null {
  for (const tab of state.value.tabs) {
    if (tab.sessionIds.includes(sessionId)) {
      return { tab, expanded: tab.columns.some(c => c.sessionId === sessionId) }
    }
  }
  return null
}

/** 会话是否在「当前激活 tab 的展开列」中(完成通知的可见性判定,FR-006) */
function isSessionVisibleInWorkbench(sessionId: string): boolean {
  return activeTab.value.columns.some(c => c.sessionId === sessionId)
}

// ---- tab 操作(FR-001) ----

function createTab(): WorkbenchTab {
  state.value.tabSeq += 1
  const tab = createTabObject(state.value.tabSeq)
  state.value.tabs.push(tab)
  state.value.activeTabId = tab.id
  return tab
}

/** 重命名:1–20 字符,超长截断,空名回退原名 */
function renameTab(tabId: string, name: string) {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (!tab) return
  const trimmed = name.trim().slice(0, 20)
  if (trimmed) tab.name = trimmed
}

/** 关闭 tab(连带清退其中全部会话)。最后一个 tab 不可关。调用方负责确认弹窗 */
function closeTab(tabId: string) {
  if (state.value.tabs.length <= 1) return
  const idx = state.value.tabs.findIndex(t => t.id === tabId)
  if (idx < 0) return
  const removed = state.value.tabs.splice(idx, 1)[0]
  if (state.value.activeTabId === tabId) {
    state.value.activeTabId = state.value.tabs[Math.max(0, idx - 1)].id
  }
  for (const sid of removed.sessionIds) teardownSession(sid)
}

function setActiveTab(tabId: string) {
  if (state.value.tabs.some(t => t.id === tabId)) {
    state.value.activeTabId = tabId
  }
}

function reorderSessions(tabId: string, fromIndex: number, toIndex: number) {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (!tab) return
  const n = tab.sessionIds.length
  if (fromIndex < 0 || fromIndex >= n || toIndex < 0 || toIndex >= n || fromIndex === toIndex) return
  const [moved] = tab.sessionIds.splice(fromIndex, 1)
  tab.sessionIds.splice(toIndex, 0, moved)
}

// ---- 赛马模式 ----

/**
 * 从已有会话发起赛马:原会话迁入新赛马 Tab 为 lane 1,
 * 再分叉一份为 lane 2。调用方负责先登记分叉意图(registerFork),
 * 落盘由首条消息时 CLI 原生 --fork-session 完成。
 */
function createRaceTab(sourceSessionId: string, cwd: string, forkedSessionId: string): WorkbenchTab {
  detachSessionFromTabs(sourceSessionId)

  state.value.tabSeq += 1
  const tab = createTabObject(state.value.tabSeq)
  tab.name = i18n.global.t('workbench.race.defaultTabName', { seq: state.value.tabSeq })

  const lanes: RaceLane[] = []
  for (const sid of [sourceSessionId, forkedSessionId]) {
    tab.sessionIds.push(sid)
    state.value.openSeq += 1
    tab.columns.push({
      id: genId('wbcol'),
      type: 'session',
      sessionId: sid,
      openedSeq: state.value.openSeq,
    })
    lanes.push({
      id: genId('lane'),
      sessionId: sid,
      label: i18n.global.t('workbench.race.laneLabel', { n: lanes.length + 1 }),
    })
  }

  tab.columnSizes = equalSizes(lanes.length)
  tab.race = { cwd, lanes, engineSwitchLocked: false }

  state.value.tabs.push(tab)
  state.value.activeTabId = tab.id
  return tab
}

/** 向赛马 Tab 追加一个分叉赛道。调用方负责先完成文件复制 */
function addRaceLane(tabId: string, forkedSessionId: string) {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (!tab?.race) return

  tab.sessionIds.push(forkedSessionId)
  state.value.openSeq += 1
  tab.columns.push({
    id: genId('wbcol'),
    type: 'session',
    sessionId: forkedSessionId,
    openedSeq: state.value.openSeq,
  })
  tab.race.lanes.push({
    id: genId('lane'),
    sessionId: forkedSessionId,
    label: i18n.global.t('workbench.race.laneLabel', { n: tab.race.lanes.length + 1 }),
  })
  tab.columnSizes = equalSizes(tab.columns.length)
}

/**
 * 原位替换工作台会话：保留 tab、lane、column 身份与列宽。
 * 通用引擎的空线程切换渠道/引擎时使用，替换完成后才关闭旧运行时。
 */
function replaceWorkbenchSession(sessionId: string, replacementSessionId: string): boolean {
  const found = findSession(sessionId)
  if (!found || sessionId === replacementSessionId) return false
  const { tab } = found
  if (tab.sessionIds.includes(replacementSessionId)) return false

  const lane = tab.race?.lanes.find(item => item.sessionId === sessionId) ?? null
  if (tab.race && (!lane || tab.race.engineSwitchLocked)) return false
  const sessionIndex = tab.sessionIds.indexOf(sessionId)
  if (sessionIndex < 0) return false

  if (lane) lane.sessionId = replacementSessionId
  for (const column of tab.columns) {
    if (column.sessionId === sessionId) column.sessionId = replacementSessionId
  }
  tab.sessionIds[sessionIndex] = replacementSessionId
  teardownSession(sessionId)
  delete state.value.pendingTasks[sessionId]
  delete state.value.drafts[sessionId]
  delete state.value.forkIntents[sessionId]
  return true
}

/** 原位替换赛道会话：额外校验调用方指定的赛马 Tab。 */
function replaceRaceLaneSession(tabId: string, sessionId: string, replacementSessionId: string): boolean {
  const found = findSession(sessionId)
  if (!found || found.tab.id !== tabId || !found.tab.race) return false
  return replaceWorkbenchSession(sessionId, replacementSessionId)
}

function lockRaceEngineSelection(tabId: string) {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (tab?.race) tab.race.engineSwitchLocked = true
}

/** 关闭赛马赛道:移除列 + lane;剩 1 条时自动解散为普通 Tab */
function removeRaceLane(tabId: string, sessionId: string) {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (!tab?.race) return

  tab.race.lanes = tab.race.lanes.filter(l => l.sessionId !== sessionId)
  const si = tab.sessionIds.indexOf(sessionId)
  if (si >= 0) tab.sessionIds.splice(si, 1)
  const ci = tab.columns.findIndex(c => c.sessionId === sessionId)
  if (ci >= 0) {
    // 同步原子移除,理由同 reclaimColumnWidth:延迟 splice 会持久化非法中间态
    suppressColumnTransition.value = true
    tab.columns.splice(ci, 1)
    tab.columnSizes = equalSizes(tab.columns.length)
    nextTick(() => { suppressColumnTransition.value = false })
  }
  if (tab.race.lanes.length <= 1) {
    delete tab.race
  }
  teardownSession(sessionId)
}

/** 重置所有赛道：保留赛道数、cwd 和每条赛道的设置（模型/强度/渠道），只清空会话 */
function resetRaceLanes(tabId: string, replacementSessionIds?: string[]) {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (!tab?.race) return
  const cwd = tab.race.cwd
  const oldLanes = tab.race.lanes
  if (replacementSessionIds && replacementSessionIds.length !== oldLanes.length) return

  const oldSettings: Array<{ sid: string; raw: string | null }> = oldLanes.map(lane => ({
    sid: lane.sessionId,
    raw: localStorage.getItem(`monet:session-settings:${lane.sessionId}`),
  }))

  tab.sessionIds = []
  tab.columns = []
  const lanes: RaceLane[] = []
  for (let i = 0; i < oldLanes.length; i++) {
    const sid = replacementSessionIds?.[i] ?? crypto.randomUUID()
    if (!replacementSessionIds) state.value.drafts[sid] = cwd
    tab.sessionIds.push(sid)
    state.value.openSeq += 1
    tab.columns.push({
      id: genId('wbcol'),
      type: 'session',
      sessionId: sid,
      openedSeq: state.value.openSeq,
    })
    lanes.push({
      id: genId('lane'),
      sessionId: sid,
      label: i18n.global.t('workbench.race.laneLabel', { n: i + 1 }),
    })
    if (oldSettings[i].raw) {
      localStorage.setItem(`monet:session-settings:${sid}`, oldSettings[i].raw!)
    }
  }
  tab.columnSizes = equalSizes(lanes.length)
  tab.race = { cwd, lanes, engineSwitchLocked: false }
  for (const lane of oldLanes) teardownSession(lane.sessionId)
}

function findLane(tab: WorkbenchTab, sessionId: string): RaceLane | null {
  return tab.race?.lanes.find(l => l.sessionId === sessionId) ?? null
}

// ---- 会话进出与展开(FR-002/004) ----

export type OpenResult =
  | { kind: 'added'; tabId: string; collapsedSessionIds: string[] }
  | { kind: 'existing'; tabId: string; collapsedSessionIds: string[] }

/**
 * 「在工作台打开」:加入当前激活 tab 并自动展开;
 * 已在某 tab 则切到该 tab 并高亮其左列卡片,不重复添加(唯一性)。
 */
function openSession(sessionId: string): OpenResult {
  const found = findSession(sessionId)
  if (found) {
    state.value.activeTabId = found.tab.id
    flashSession(sessionId)
    return { kind: 'existing', tabId: found.tab.id, collapsedSessionIds: [] }
  }
  const tab = activeTab.value
  tab.sessionIds.push(sessionId)
  const expanded = expandSession(tab.id, sessionId)
  return { kind: 'added', tabId: tab.id, collapsedSessionIds: expanded.collapsedSessionIds }
}

/**
 * 应用内新建会话(FR-002 增强,替代经终端链路):前端生成 UUID 登记草稿,
 * 加入当前激活 tab 并展开。首条消息由 Rust 端以 --session-id 新建落盘,
 * 之后 watcher 刷新 projects,草稿被 pruneDrafts 收割,显示自动切换真实数据。
 */
function createDraftSession(cwd: string): string {
  const sessionId = crypto.randomUUID()
  state.value.drafts[sessionId] = cwd
  openSession(sessionId)
  return sessionId
}

/** 先选择工作目录、再在正文区选择引擎的新任务占位。 */
function createPendingTask(cwd: string): string {
  const sessionId = crypto.randomUUID()
  state.value.pendingTasks[sessionId] = cwd
  openSession(sessionId)
  return sessionId
}

/** 将中立占位原位升级为 Claude Code 原生草稿，保留列与 sessionId。 */
function promotePendingTaskToDraft(sessionId: string): boolean {
  const cwd = state.value.pendingTasks[sessionId]
  if (!cwd) return false
  state.value.drafts[sessionId] = cwd
  delete state.value.pendingTasks[sessionId]
  return true
}

function pendingTaskCwd(sessionId: string): string | null {
  return state.value.pendingTasks[sessionId] ?? null
}

/** 登记原生引擎空白草稿但不改变 Tab；赛马切换会原位接管列归属。 */
function stageDraftSession(cwd: string, sessionId = crypto.randomUUID()): string {
  state.value.drafts[sessionId] = cwd
  return sessionId
}

/** 回滚尚未进入任何 Tab 的草稿，并关闭已创建的通用引擎运行时。 */
function discardStagedSession(sessionId: string) {
  teardownSession(sessionId)
  delete state.value.drafts[sessionId]
  delete state.value.forkIntents[sessionId]
  localStorage.removeItem(`monet:session-settings:${sessionId}`)
}

function registerEngineDraft(sessionId: string, draft: Omit<EngineDraft, 'runtimeScope'>) {
  state.value.engineDrafts[sessionId] = { ...draft, runtimeScope: engineDraftRuntimeScope }
  openSession(sessionId)
}

/** 登记通用引擎草稿但不改变 Tab；赛马在一次状态变更里自行接管列归属。 */
function stageEngineDraft(sessionId: string, draft: Omit<EngineDraft, 'runtimeScope'>) {
  state.value.engineDrafts[sessionId] = { ...draft, runtimeScope: engineDraftRuntimeScope }
}

function engineDraft(sessionId: string): EngineDraft | null {
  return state.value.engineDrafts[sessionId] ?? null
}

/** 草稿会话的 cwd(非草稿返回 null)。各视图据此合成「新会话」占位 */
function draftCwd(sessionId: string): string | null {
  return state.value.drafts[sessionId] ?? null
}

/**
 * 登记分叉:草稿(cwd 占位) + 意图(源 sessionId)。不复制文件,
 * 落盘由首条消息时 CLI 原生 --fork-session 完成
 */
function registerFork(forkedId: string, sourceId: string, cwd: string) {
  state.value.drafts[forkedId] = cwd
  state.value.forkIntents[forkedId] = sourceId
}

/** 分叉意图的源会话 id(非分叉草稿返回 null)。发送链路与垫底渲染据此取源 */
function forkSourceOf(sessionId: string): string | null {
  return state.value.forkIntents[sessionId] ?? null
}

/**
 * 草稿收割:已落盘(isPersisted)或已不在任何工作台(被关闭弃用)的草稿删除。
 * 由 App 层在 projects 刷新后调用。分叉意图同生命周期一并收割
 */
function pruneDrafts(isPersisted: (sessionId: string) => boolean) {
  for (const sid of Object.keys(state.value.pendingTasks)) {
    if (!findSession(sid)) delete state.value.pendingTasks[sid]
  }
  for (const sid of Object.keys(state.value.drafts)) {
    if (isPersisted(sid) || !findSession(sid)) {
      delete state.value.drafts[sid]
    }
  }
  for (const [sid, draft] of Object.entries(state.value.engineDrafts)) {
    if (isPersisted(sid) || !findSession(sid)) {
      delete state.value.engineDrafts[sid]
    } else if (draft.runtimeScope !== engineDraftRuntimeScope) {
      // thread/start 后、首条消息前没有 rollout。后端重启后旧 native id
      // 无法 thread/resume，继续保留只会形成永久禁用的工作台列。
      removeSession(sid)
    }
  }
  for (const sid of Object.keys(state.value.forkIntents)) {
    if (isPersisted(sid) || !findSession(sid)) {
      delete state.value.forkIntents[sid]
    }
  }
}

export interface ExpandResult {
  collapsedSessionIds: string[]
  focusedExisting: boolean
}

/**
 * 展开会话到右区:无容量上限,超出容器时横向滚动。
 * atIndex 指定插入列位(拖拽落点);缺省追加末尾。
 */
function expandSession(tabId: string, sessionId: string, atIndex?: number): ExpandResult {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (!tab || !tab.sessionIds.includes(sessionId)) {
    return { collapsedSessionIds: [], focusedExisting: false }
  }
  if (tab.columns.some(c => c.sessionId === sessionId)) {
    requestFocusColumn(sessionId)
    return { collapsedSessionIds: [], focusedExisting: true }
  }

  state.value.openSeq += 1
  const column: WorkbenchColumn = {
    id: genId('wbcol'),
    type: 'session',
    sessionId,
    openedSeq: state.value.openSeq,
  }
  const idx = atIndex === undefined ? tab.columns.length : Math.max(0, Math.min(atIndex, tab.columns.length))
  tab.columns.splice(idx, 0, column)
  tab.columnSizes = equalSizes(tab.columns.length)
  requestFocusColumn(sessionId)
  return { collapsedSessionIds: [], focusedExisting: false }
}

const suppressColumnTransition = ref(false)

/**
 * 移除列并智能回收宽度（同步原子）:
 * 状态变更必须一步完成——deep watch 随时可能落盘,任何"先改一半、
 * setTimeout 里补另一半"的写法都会让非法中间态(列引用已不在
 * sessionIds 的会话)被持久化,启动校验随即判损坏整体重置。
 */
function reclaimColumnWidth(tab: WorkbenchTab, removedIndex: number) {
  if (removedIndex < 0 || removedIndex >= tab.columns.length) return

  suppressColumnTransition.value = true
  tab.columns.splice(removedIndex, 1)
  tab.columnSizes.splice(removedIndex, 1)

  if (tab.columnSizes.length > 0) {
    const totalAfter = tab.columnSizes.reduce((s, w) => s + w, 0)
    const freeAfter = containerFreeWidth(tab.columnSizes.length)
    if (totalAfter < freeAfter) {
      tab.columnSizes = fillColumnWidthsProportionally(tab.columnSizes, freeAfter)
    }
  }

  nextTick(() => { suppressColumnTransition.value = false })
}

/** 收起列回左列(仍激活,「收起非退出」) */
function collapseColumn(tabId: string, sessionId: string) {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (!tab) return
  const idx = tab.columns.findIndex(c => c.sessionId === sessionId)
  if (idx < 0) return
  reclaimColumnWidth(tab, idx)
}

/** 退出工作台(左列 × / 列头 ×):从归属 tab 移除,展开列一并收回 */
function detachSessionFromTabs(sessionId: string) {
  for (const tab of state.value.tabs) {
    const i = tab.sessionIds.indexOf(sessionId)
    if (i >= 0) {
      tab.sessionIds.splice(i, 1)
      const ci = tab.columns.findIndex(c => c.sessionId === sessionId)
      if (ci >= 0) reclaimColumnWidth(tab, ci)
    }
  }
}

function removeSession(sessionId: string) {
  detachSessionFromTabs(sessionId)
  teardownSession(sessionId)
}

// ---- 右区列布局(FR-004/005) ----

function reorderColumns(tabId: string, fromIndex: number, toIndex: number) {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (!tab) return
  const n = tab.columns.length
  if (fromIndex < 0 || fromIndex >= n || toIndex < 0 || toIndex >= n || fromIndex === toIndex) return
  const [col] = tab.columns.splice(fromIndex, 1)
  tab.columns.splice(toIndex, 0, col)
  const [size] = tab.columnSizes.splice(fromIndex, 1)
  tab.columnSizes.splice(toIndex, 0, size)
}

/**
 * 拖动第 index 条分隔线(像素宽度模型):
 * - 最后一列:独立调整宽度(无右邻,拉宽触发滚动)
 * - 中间列:有余量时此消彼长;右列顶到 minColumnWidth.value 后独立拉宽
 */
function updateColumnSize(tabId: string, index: number, desiredLeftWidth: number) {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (!tab) return
  const sizes = tab.columnSizes
  if (index < 0 || index >= sizes.length) return
  const left = Math.max(minColumnWidth.value, Math.round(desiredLeftWidth))
  if (index === sizes.length - 1) {
    sizes[index] = left
    return
  }
  const combined = sizes[index] + sizes[index + 1]
  const rightFromZeroSum = combined - left
  if (rightFromZeroSum >= minColumnWidth.value) {
    sizes[index] = left
    sizes[index + 1] = rightFromZeroSum
  } else {
    sizes[index] = left
    sizes[index + 1] = minColumnWidth.value
  }
}

/** 会话离开工作台后,若不再被任何 tab 持有则关闭进程(断 Remote Control),
 *  并驱逐 useStreaming 传输态缓存(否则 streams/turnIndex 等模块级 Map 单调增长) */
function teardownSession(sessionId: string) {
  const stillReferenced = state.value.tabs.some(t => t.sessionIds.includes(sessionId))
  if (!stillReferenced) {
    const pendingTask = state.value.pendingTasks[sessionId]
    const engineSession = resolveSessionRef(sessionId) ?? state.value.engineDrafts[sessionId]?.reference
    if (engineSession) invoke('engine_close_session', { session: engineSession }).catch(() => {})
    else if (!pendingTask) invoke('close_session', { sessionId }).catch(() => {})
    // 会话彻底离开工作台 = 关闭语义:其挂载的运行命令一并停止(切走/收起不触发)
    useRunners().stopAllForSession(sessionId).catch(() => {})
    evictSessionTransients(sessionId)
    clearEngineRunConfig(sessionId)
    delete state.value.pendingTasks[sessionId]
    delete state.value.engineDrafts[sessionId]
  }
}

function resetColumnSizes(tabId: string) {
  const tab = state.value.tabs.find(t => t.id === tabId)
  if (!tab || tab.columns.length === 0) return
  tab.columnSizes = equalSizes(tab.columns.length)
}

function setMinColumnWidth(w: number) {
  const clamped = Math.max(ABSOLUTE_MIN_COLUMN_WIDTH, Math.round(w))
  minColumnWidth.value = clamped
  localStorage.setItem(MIN_WIDTH_KEY, String(clamped))
}

export function useWorkbench() {
  return {
    state,
    activeTab,
    minColumnWidth,
    flashSessionId,
    focusColumnRequest,
    findSession,
    isSessionVisibleInWorkbench,
    createTab,
    createRaceTab,
    addRaceLane,
    replaceWorkbenchSession,
    replaceRaceLaneSession,
    lockRaceEngineSelection,
    removeRaceLane,
    resetRaceLanes,
    findLane,
    renameTab,
    closeTab,
    setActiveTab,
    reorderSessions,
    openSession,
    createPendingTask,
    promotePendingTaskToDraft,
    pendingTaskCwd,
    createDraftSession,
    stageDraftSession,
    discardStagedSession,
    registerEngineDraft,
    stageEngineDraft,
    engineDraft,
    draftCwd,
    registerFork,
    forkSourceOf,
    pruneDrafts,
    expandSession,
    collapseColumn,
    removeSession,
    reorderColumns,
    updateColumnSize,
    resetColumnSizes,
    setMinColumnWidth,
    suppressColumnTransition,
  }
}
