import { ref, computed, watch, type Ref, type ComputedRef } from 'vue'
import { readMigratedStorage } from '../utils/storageMigrate'

/**
 * 会话级设置(模型/努力等级/渠道)按 sessionId 持久化到 localStorage。
 * key 格式严格遵循 PRD FR-006:`monet:session-settings:<sid>`
 */

export type EffortLevel = 'low' | 'medium' | 'high' | 'xhigh' | 'max'

/**
 * 努力设置值:五档 effort | ultracode(超档:xhigh + 自动多智能体 workflow,
 * 经 --settings 注入而非 --effort) | null(本会话未设置,按应用默认行为)
 */
export type EffortSetting = EffortLevel | 'ultracode' | null

/**
 * 渠道切换记账(本地横线):jsonl 不存渠道(同 effort 不可还原的先例),
 * 切换事件只在 Monet 自己的账本里,外部会话/清本地数据后无从还原
 */
export interface ChannelMark {
  /** 切换发生时历史区最后一条消息 uuid;会话起点切换为 null */
  afterUuid: string | null
  /** 切到的选择:'official' | 渠道 id | null(切回跟随默认) */
  channelId: string | null
  /** 切换时刻(ms) */
  at: number
}

export type PermissionMode = 'default' | 'plan' | 'acceptEdits' | 'auto' | 'bypassPermissions' | 'dontAsk'

export interface SessionSettings {
  /** null 表示未设置(回退使用 session 自带 model 字段) */
  modelId: string | null
  effort: EffortSetting
  /** 快速模式:null = 跟随 CLI/项目配置；boolean = 本会话显式覆盖 */
  fastMode: boolean | null
  /** 渠道选择:null = 跟随应用默认渠道;'official' = 强制官方;其他 = 渠道 id */
  channelId: string | null
  /** 渠道切换横线记账(按发生顺序) */
  channelMarks: ChannelMark[]
  /** 顾问模式:开启后主模型强制 sonnet + 经 --settings 注入 fable 顾问(见 settings-backlog 第 3 条) */
  advisor: boolean
  /** Chrome 集成(--chrome 启动参数):浏览器工具常驻吃上下文,默认关、按需开 */
  chrome: boolean
  /** 自定义 CLI 参数原始串(逃生舱:Monet 未原生支持的 flag 用户自行追加);
   *  协议级参数由 Rust 端黑名单剔除,变更经 needs_restart 重启生效 */
  extraArgs: string
  /** null = 跟随 CLI/项目配置；具体模式 = Monet 会话显式覆盖 */
  permissionMode: PermissionMode | null
}

// effort 为 null = 本会话未设置 → 按应用默认行为发送。应用默认当前实现为「不传
// --effort,由 CLI 自行决定」;将来由设置页配置(可选值含跟随 CLI/具体档位,见
// docs/settings-backlog.md),「跟随 CLI/默认值」不出现在会话级选项中
const VALID_PERMISSION_MODES: PermissionMode[] = ['default', 'plan', 'acceptEdits', 'auto', 'bypassPermissions', 'dontAsk']

export const DEFAULT_SETTINGS: SessionSettings = {
  modelId: null,
  effort: null,
  fastMode: null,
  channelId: null,
  channelMarks: [],
  advisor: false,
  chrome: false,
  extraArgs: '',
  permissionMode: null,
}

/** 顾问模式锁定的主模型(硬编码,未来设置页全局配置——见 docs/settings-backlog.md 第 3 条) */
export const ADVISOR_MAIN_MODEL = 'claude-sonnet-4-6'

const KEY_PREFIX = 'monet:session-settings:'
const LEGACY_KEY_PREFIX = 'cc-space:session-settings:' // 旧 key 前缀(更名前),一次性迁移读取用

function storageKey(sid: string): string {
  return `${KEY_PREFIX}${sid}`
}

function legacyStorageKey(sid: string): string {
  return `${LEGACY_KEY_PREFIX}${sid}`
}

const VALID_EFFORTS: EffortLevel[] = ['low', 'medium', 'high', 'xhigh', 'max']
/** 可持久化的设置值(含 ultracode);null 不在列表内,序列化天然支持 */
const VALID_STORED: NonNullable<EffortSetting>[] = [...VALID_EFFORTS, 'ultracode']

/** 渠道 marks 宽松校验:坏条目丢弃,不作废整体(对齐 workbench drafts 增量字段先例) */
function sanitizeMarks(raw: unknown): ChannelMark[] {
  if (!Array.isArray(raw)) return []
  const marks: ChannelMark[] = []
  for (const m of raw) {
    if (!m || typeof m !== 'object') continue
    const { afterUuid, channelId, at } = m as Partial<ChannelMark>
    if (afterUuid !== null && typeof afterUuid !== 'string') continue
    if (channelId !== null && typeof channelId !== 'string') continue
    marks.push({
      afterUuid: afterUuid ?? null,
      channelId: channelId ?? null,
      at: typeof at === 'number' ? at : 0,
    })
  }
  return marks
}

function loadFromStorage(sid: string): SessionSettings {
  try {
    const raw = readMigratedStorage(storageKey(sid), legacyStorageKey(sid))
    if (!raw) return structuredClone(DEFAULT_SETTINGS)
    const parsed = JSON.parse(raw) as Partial<SessionSettings>
    const effort: EffortSetting = parsed.effort && VALID_STORED.includes(parsed.effort)
      ? parsed.effort
      : null
    const modelId = typeof parsed.modelId === 'string' && parsed.modelId.length > 0
      ? parsed.modelId
      : null
    const channelId = typeof parsed.channelId === 'string' && parsed.channelId.length > 0
      ? parsed.channelId
      : null
    const permissionMode: PermissionMode | null =
      typeof parsed.permissionMode === 'string' && VALID_PERMISSION_MODES.includes(parsed.permissionMode as PermissionMode)
        ? parsed.permissionMode as PermissionMode
        : null
    return {
      modelId,
      effort,
      fastMode: typeof parsed.fastMode === 'boolean' ? parsed.fastMode : null,
      channelId,
      channelMarks: sanitizeMarks(parsed.channelMarks),
      advisor: parsed.advisor === true,
      chrome: parsed.chrome === true,
      extraArgs: typeof parsed.extraArgs === 'string' ? parsed.extraArgs : '',
      permissionMode,
    }
  } catch (_) {
    return structuredClone(DEFAULT_SETTINGS)
  }
}

export function getSessionSettings(sid: string): SessionSettings {
  return loadFromStorage(sid)
}

function saveToStorage(sid: string, settings: SessionSettings) {
  try {
    localStorage.setItem(storageKey(sid), JSON.stringify(settings))
  } catch (_) {
    // 存储失败静默忽略,设置丢失不阻塞流程
  }
}

function removeFromStorage(sid: string) {
  try {
    localStorage.removeItem(storageKey(sid))
    // 显式 reset 时一并清掉旧 key,否则下次加载会被旧 key 迁移「复活」旧设置
    localStorage.removeItem(legacyStorageKey(sid))
  } catch (_) {}
}

/**
 * 只读某会话已存的渠道选择,不建 useSessionSettings 实例(供 SessionList 等
 * 列表场景的「在终端打开」带上会话渠道用)。返回 channelId 原值(null/official/id)
 */
export function readStoredChannelId(sid: string): string | null {
  return loadFromStorage(sid).channelId
}

/**
 * 新会话继承源会话的模型选择(渠道/模型/努力等级/快速模式/顾问)——列头加号新建与各分叉
 * 入口调用,延续「当前在用什么模型」的直觉。渠道必须随模型走:渠道语境外的
 * 模型 ID 可能无效(官方 alias vs 第三方映射)。工作方式类设置(chrome/extraArgs/
 * permissionMode)与渠道切换横线记账(channelMarks)不继承,新会话从默认开始
 */
export function inheritRunSettings(fromSid: string, toSid: string): void {
  const src = loadFromStorage(fromSid)
  // 源全默认:不写冗余存储条目
  if (src.modelId === null && src.effort === null && src.fastMode === null && src.channelId === null && !src.advisor) return
  saveToStorage(toSid, {
    ...structuredClone(DEFAULT_SETTINGS),
    modelId: src.modelId,
    effort: src.effort,
    fastMode: src.fastMode,
    channelId: src.channelId,
    advisor: src.advisor,
  })
}

export interface UseSessionSettingsReturn {
  /** 当前会话的设置(只读 computed) */
  settings: ComputedRef<SessionSettings>
  /** 设置模型 ID(null 表示清除) */
  setModel: (modelId: string | null) => void
  /** 设置努力等级(null 表示跟随 CLI,'ultracode' 为超档) */
  setEffort: (effort: EffortSetting) => void
  /** 设置快速模式；null 表示恢复跟随 CLI/项目配置 */
  setFastMode: (fastMode: boolean | null) => void
  /**
   * 切换渠道并记一条横线账(afterUuid = 切换时历史区最后一条消息 uuid)。
   * 与当前值相同的切换为 no-op,不产生横线
   */
  setChannel: (channelId: string | null, afterUuid: string | null) => void
  /** 开关顾问模式 */
  setAdvisor: (advisor: boolean) => void
  /** 开关 Chrome 集成 */
  setChrome: (chrome: boolean) => void
  /** 设置自定义 CLI 参数 */
  setExtraArgs: (extraArgs: string) => void
  /** 切换权限模式；null 表示恢复跟随 CLI */
  setPermissionMode: (mode: PermissionMode | null) => void
  /** 重置为默认并清除 localStorage */
  reset: () => void
}

/**
 * 按 sessionId 维护一份会话设置。
 * sessionId 切换时自动从 localStorage 加载,
 * 设置变化时自动写回。
 */
export function useSessionSettings(sessionId: Ref<string | null>): UseSessionSettingsReturn {
  const internal = ref<SessionSettings>(structuredClone(DEFAULT_SETTINGS))

  // sessionId 变化:重新加载
  watch(
    sessionId,
    (sid) => {
      if (sid) {
        internal.value = loadFromStorage(sid)
      } else {
        internal.value = structuredClone(DEFAULT_SETTINGS)
      }
    },
    { immediate: true },
  )

  // 设置变化:回写当前 sid
  watch(
    internal,
    (s) => {
      const sid = sessionId.value
      if (sid) saveToStorage(sid, s)
    },
    { deep: true },
  )

  function setModel(modelId: string | null) {
    internal.value = { ...internal.value, modelId }
  }

  function setEffort(effort: EffortSetting) {
    internal.value = { ...internal.value, effort }
  }

  function setFastMode(fastMode: boolean | null) {
    internal.value = { ...internal.value, fastMode }
  }

  function setChannel(channelId: string | null, afterUuid: string | null) {
    if (channelId === internal.value.channelId) return
    // 切渠道 = 换整套运行预设:清空本会话的模型/effort 覆盖,回落新渠道默认——
    // 否则旧渠道语境下选的模型 ID 在新渠道可能不存在(官方 alias vs 第三方映射)
    internal.value = {
      ...internal.value,
      channelId,
      modelId: null,
      effort: null,
      channelMarks: [
        ...internal.value.channelMarks,
        { afterUuid, channelId, at: Date.now() },
      ],
    }
  }

  function setAdvisor(advisor: boolean) {
    internal.value = { ...internal.value, advisor }
  }

  function setChrome(chrome: boolean) {
    internal.value = { ...internal.value, chrome }
  }

  function setExtraArgs(extraArgs: string) {
    internal.value = { ...internal.value, extraArgs: extraArgs.trim() }
  }

  function setPermissionMode(mode: PermissionMode | null) {
    internal.value = { ...internal.value, permissionMode: mode }
  }

  function reset() {
    const sid = sessionId.value
    if (sid) removeFromStorage(sid)
    internal.value = structuredClone(DEFAULT_SETTINGS)
  }

  const settings = computed<SessionSettings>(() => internal.value)

  return { settings, setModel, setEffort, setFastMode, setChannel, setAdvisor, setChrome, setExtraArgs, setPermissionMode, reset }
}
