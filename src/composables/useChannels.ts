import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import i18n from '../locales'

export interface ChannelInfo {
  id: string
  name: string
  note: string | null
  baseUrl: string | null
  authMode: 'bearer' | 'none'
  authTokenMasked: string | null
  extraEnvKeys: string[]
  valid: boolean
  enabled: boolean
  protocol: string
  scope: string
  agentModel: string | null
  availableModels: string[]
  /** Monet 托管的模型角色映射键当前值(MODEL_ENV_KEYS 过滤自 env 块,明文回传) */
  modelEnv: Record<string, string>
  /** 渠道默认模型(official 存元数据;第三方即文件 env.ANTHROPIC_MODEL) */
  defaultModel: string | null
  /** 渠道默认思考强度:五档 | 'ultracode'(official 存元数据;第三方即文件顶层 effortLevel/ultracode) */
  defaultEffort: string | null
  /** 同一渠道可绑定多个引擎；每个引擎由自己的 adapter 解释配置。 */
  engineSupport: string[]
  claude: EngineConnectionInfo | null
  codex: CodexChannelInfo | null
}

export interface EngineConnectionInfo {
  baseUrl: string | null
  authMode: 'bearer' | 'none' | null
  authTokenMasked: string | null
  resolvedBaseUrl: string | null
  cachedBaseUrl: string | null
}

export interface CodexChannelInfo {
  mode: 'external' | 'managed'
  providerId: string
  baseUrl: string | null
  authMode: 'inherit' | 'bearer' | 'openai' | 'none'
  authTokenMasked: string | null
  defaultModel: string | null
  defaultEffort: string | null
  availableModels: string[]
  resolvedBaseUrl: string | null
  cachedBaseUrl: string | null
}

/** 通用会话外壳只消费这组字段；引擎原生渠道结构在此处适配。 */
export interface EngineChannelBindingInfo {
  providerId: string | null
  defaultModel: string | null
  defaultEffort: string | null
  availableModels: string[]
}

export const APPLE_FM_CHANNEL_ID = 'apple-fm'

export type SessionEngineId = 'claude-code' | 'codex'

interface ChannelListResult {
  channels: ChannelInfo[]
  defaultSessionChannels?: Partial<Record<SessionEngineId, string | null>>
  defaultSessionModels?: Partial<Record<SessionEngineId, string | null>>
  defaultSessionEfforts?: Partial<Record<SessionEngineId, string | null>>
  /** 兼容旧后端版本。 */
  defaultSessionChannel?: string | null
  defaultAgentEngine?: SessionEngineId
  defaultAgentChannels?: Partial<Record<SessionEngineId, string | null>>
  defaultAgentModels?: Partial<Record<SessionEngineId, string | null>>
  defaultAgentEfforts?: Partial<Record<SessionEngineId, string | null>>
  /** 兼容旧后端版本。 */
  defaultAgentChannel?: string | null
  defaultAgentModel?: string | null
  defaultAgentEffort?: string | null
}

export const OFFICIAL_CHANNEL_ID = 'official'
/** 官方直连:压制 CLI 配置中的第三方键,强制 Anthropic 官方 + OAuth(虚拟渠道,无文件) */
export const OFFICIAL_DIRECT_CHANNEL_ID = 'official-direct'

export function channelSupportsEngine(channel: ChannelInfo, engineId: string): boolean {
  return channel.engineSupport.includes(engineId)
}

export function engineChannelBinding(
  channel: ChannelInfo | null | undefined,
  engineId: string,
): EngineChannelBindingInfo | null {
  if (!channel || !channelSupportsEngine(channel, engineId)) return null
  if (engineId === 'codex' && channel.codex) {
    return {
      providerId: channel.codex.providerId,
      defaultModel: null,
      defaultEffort: null,
      availableModels: channel.availableModels,
    }
  }
  return null
}

export function engineProviderIdFromSource(
  sourceMeta: Record<string, unknown> | null | undefined,
  engineId: string,
): string | null {
  if (engineId === 'codex' && typeof sourceMeta?.modelProvider === 'string') {
    return sourceMeta.modelProvider
  }
  return null
}

const channels = ref<ChannelInfo[]>([])
const defaultSessionChannels = ref<Record<SessionEngineId, string | null>>({
  'claude-code': null,
  codex: null,
})
const defaultSessionModels = ref<Record<SessionEngineId, string | null>>({ 'claude-code': null, codex: null })
const defaultSessionEfforts = ref<Record<SessionEngineId, string | null>>({ 'claude-code': null, codex: null })
/** Claude Code 的兼容别名，供现有会话配置解析继续使用。 */
const defaultSessionChannel = computed(() => defaultSessionChannels.value['claude-code'])
const defaultAgentEngine = ref<SessionEngineId>('claude-code')
const defaultAgentChannels = ref<Record<SessionEngineId, string | null>>({ 'claude-code': null, codex: null })
const defaultAgentModels = ref<Record<SessionEngineId, string | null>>({ 'claude-code': null, codex: null })
const defaultAgentEfforts = ref<Record<SessionEngineId, string | null>>({ 'claude-code': null, codex: null })
const defaultAgentChannel = computed(() => defaultAgentChannels.value[defaultAgentEngine.value])
const defaultAgentModel = computed(() => defaultAgentModels.value[defaultAgentEngine.value])
const defaultAgentEffort = computed(() => defaultAgentEfforts.value[defaultAgentEngine.value])

export async function refreshChannels(): Promise<void> {
  try {
    const r = await invoke<ChannelListResult>('list_channels')
    channels.value = r.channels
    defaultSessionChannels.value = {
      'claude-code': r.defaultSessionChannels?.['claude-code']
        ?? r.defaultSessionChannel
        ?? null,
      codex: r.defaultSessionChannels?.codex ?? null,
    }
    defaultSessionModels.value = {
      'claude-code': r.defaultSessionModels?.['claude-code'] ?? null,
      codex: r.defaultSessionModels?.codex ?? null,
    }
    defaultSessionEfforts.value = {
      'claude-code': r.defaultSessionEfforts?.['claude-code'] ?? null,
      codex: r.defaultSessionEfforts?.codex ?? null,
    }
    defaultAgentEngine.value = r.defaultAgentEngine === 'codex' ? 'codex' : 'claude-code'
    defaultAgentChannels.value = {
      'claude-code': r.defaultAgentChannels?.['claude-code']
        ?? (defaultAgentEngine.value === 'claude-code' ? r.defaultAgentChannel : null)
        ?? null,
      codex: r.defaultAgentChannels?.codex
        ?? (defaultAgentEngine.value === 'codex' ? r.defaultAgentChannel : null)
        ?? null,
    }
    defaultAgentModels.value = {
      'claude-code': r.defaultAgentModels?.['claude-code']
        ?? (defaultAgentEngine.value === 'claude-code' ? r.defaultAgentModel : null)
        ?? null,
      codex: r.defaultAgentModels?.codex
        ?? (defaultAgentEngine.value === 'codex' ? r.defaultAgentModel : null)
        ?? null,
    }
    defaultAgentEfforts.value = {
      'claude-code': r.defaultAgentEfforts?.['claude-code']
        ?? (defaultAgentEngine.value === 'claude-code' ? r.defaultAgentEffort : null)
        ?? null,
      codex: r.defaultAgentEfforts?.codex
        ?? (defaultAgentEngine.value === 'codex' ? r.defaultAgentEffort : null)
        ?? null,
    }
  } catch {
    // 读取失败保留旧值
  }
}

export function channelDisplayName(id: string | null): string {
  if (!id || id === OFFICIAL_CHANNEL_ID) return i18n.global.t('channel.official')
  if (id === OFFICIAL_DIRECT_CHANNEL_ID) return i18n.global.t('channel.officialDirect')
  return channels.value.find(c => c.id === id)?.name ?? id
}

/** 「跟随 CLI」当前实际指向(副文案用):official = CLI 走官方登录;third-party 附 host */
export const cliEnvTarget = ref<{ kind: 'official' | 'third-party'; host: string | null }>({ kind: 'official', host: null })

export async function refreshCliEnvTarget(): Promise<void> {
  try {
    cliEnvTarget.value = await invoke<{ kind: 'official' | 'third-party'; host: string | null }>('get_cli_env_target')
  } catch { /* 探测失败保留旧值,副文案提示性质 */ }
}

export function resolveChannel(selected: string | null): string | null {
  if (selected === OFFICIAL_CHANNEL_ID) return null
  if (selected) return selected
  // 跟随默认:默认渠道被禁用时回落官方,不带着禁用渠道发送
  const id = defaultSessionChannel.value
  if (!id) return null
  const ch = channels.value.find(c => c.id === id)
  return ch && ch.enabled ? id : null
}

export interface SaveChannelPayload {
  id: string
  name: string
  baseUrl: string
  authMode?: 'bearer' | 'none'
  authToken?: string
  note?: string
  protocol?: string
  scope?: string
  agentModel?: string
  availableModels?: string[]
  /** 整命名空间替换语义:传对象=先移除全部 21 托管键再写非空值;不传/undefined=不动这些键 */
  modelEnv?: Record<string, string>
  /** 渠道默认思考强度:传字符串=按值重写(空串=清除),不传=不动(默认模型走 modelEnv.ANTHROPIC_MODEL) */
  defaultEffort?: string
  engineSupport?: string[]
  claude?: {
    baseUrl?: string
    authMode?: 'inherit' | 'bearer' | 'none'
    authToken?: string
    resolvedBaseUrl?: string
  }
  codex?: {
    mode: 'external' | 'managed'
    providerId: string
    baseUrl?: string
    authMode?: 'inherit' | 'bearer' | 'openai' | 'none'
    authToken?: string
    resolvedBaseUrl?: string
  }
}

async function saveChannel(payload: SaveChannelPayload): Promise<void> {
  await invoke('save_channel', {
    id: payload.id,
    name: payload.name,
    baseUrl: payload.baseUrl,
    authMode: payload.authMode ?? null,
    authToken: payload.authToken ?? null,
    note: payload.note ?? null,
    protocol: payload.protocol ?? null,
    scope: payload.scope ?? null,
    agentModel: payload.agentModel ?? null,
    availableModels: payload.availableModels ?? null,
    modelEnv: payload.modelEnv ?? null,
    defaultEffort: payload.defaultEffort ?? null,
    engineSupport: payload.engineSupport ?? null,
    claude: payload.claude ?? null,
    codex: payload.codex ?? null,
  })
  await refreshChannels()
}

/** official 渠道的默认模型/思考强度(全量替换语义,空/null = 清除) */
async function setOfficialDefaults(model: string | null, effort: string | null): Promise<void> {
  await invoke('set_official_defaults', { model, effort })
  await refreshChannels()
}

// 更名前(CC Space 时期)由已废弃的 useAppDefaults 写入的历史 key,保持原名不改为
// monet:——改名会读不到老用户既有数据,迁移失效。此 key 只读不写、迁移后即删除。
const LEGACY_APP_DEFAULTS_KEY = 'cc-space:app-defaults'

/**
 * 一次性迁移:旧「应用默认思考强度」(localStorage, useAppDefaults) → official 渠道默认。
 * official 已有显式配置时旧值直接丢弃;迁移后移除旧 key,幂等。
 */
export async function migrateLegacyAppDefaults(): Promise<void> {
  try {
    const raw = localStorage.getItem(LEGACY_APP_DEFAULTS_KEY)
    if (!raw) return
    const effort: unknown = JSON.parse(raw)?.effort
    if (typeof effort === 'string' && effort) {
      await refreshChannels()
      const official = channels.value.find(c => c.id === OFFICIAL_CHANNEL_ID)
      if (official && !official.defaultEffort) {
        await setOfficialDefaults(official.defaultModel, effort)
      }
    }
    localStorage.removeItem(LEGACY_APP_DEFAULTS_KEY)
  } catch {
    // 迁移失败不阻塞启动;旧 key 保留,下次启动重试
  }
}

export interface AgentFeaturePrefs {
  preferredChannel: string | null
  preferredModel: string | null
}

const agentPreferences = ref<Record<string, AgentFeaturePrefs>>({})

async function loadAgentPreferences(): Promise<void> {
  try {
    agentPreferences.value = await invoke<Record<string, AgentFeaturePrefs>>('get_agent_preferences')
  } catch {
    // ignore
  }
}

async function setDefaultSessionChannel(engine: SessionEngineId, id: string | null): Promise<void> {
  await invoke('set_default_session_channel', { engine, id })
  defaultSessionChannels.value = {
    ...defaultSessionChannels.value,
    [engine]: id,
  }
}

async function setDefaultSessionRuntime(
  engine: SessionEngineId,
  model: string | null,
  effort: string | null,
): Promise<void> {
  await invoke('set_default_session_runtime', { engine, model, effort })
  defaultSessionModels.value = { ...defaultSessionModels.value, [engine]: model }
  defaultSessionEfforts.value = { ...defaultSessionEfforts.value, [engine]: effort }
}

async function setDefaultAgentEngine(engine: SessionEngineId): Promise<void> {
  await invoke('set_default_agent_engine', { engine })
  defaultAgentEngine.value = engine
  await loadAgentPreferences()
}

async function setDefaultAgentModel(
  engine: SessionEngineId,
  channel: string | null,
  model: string | null,
): Promise<void> {
  await invoke('set_default_agent_model', { engine, channel, model })
  defaultAgentChannels.value = { ...defaultAgentChannels.value, [engine]: channel }
  defaultAgentModels.value = { ...defaultAgentModels.value, [engine]: model }
}

async function setDefaultAgentEffort(engine: SessionEngineId, effort: string | null): Promise<void> {
  await invoke('set_default_agent_effort', { engine, effort })
  defaultAgentEfforts.value = { ...defaultAgentEfforts.value, [engine]: effort }
}

async function setAgentFeatureModel(
  engine: SessionEngineId,
  key: string,
  channel: string | null,
  model: string | null,
): Promise<void> {
  await invoke('set_agent_feature_model', { engine, key, channel, model })
  agentPreferences.value = {
    ...agentPreferences.value,
    [key]: { preferredChannel: channel, preferredModel: model },
  }
}

async function deleteChannel(id: string): Promise<void> {
  await invoke('delete_channel', { id })
  await Promise.all([refreshChannels(), loadAgentPreferences()])
}

async function setChannelEnabled(id: string, enabled: boolean): Promise<void> {
  await invoke('set_channel_enabled', { id, enabled })
  await Promise.all([refreshChannels(), loadAgentPreferences()])
}

const revealedTokens = ref<Record<string, string>>({})

async function revealToken(id: string, engine = 'shared'): Promise<string | null> {
  const key = `${engine}:${id}`
  if (revealedTokens.value[key]) return revealedTokens.value[key]
  try {
    const token = await invoke<string | null>('get_channel_token', { id, engine })
    if (token) revealedTokens.value = { ...revealedTokens.value, [key]: token }
    return token
  } catch { return null }
}

function hideToken(id: string, engine = 'shared') {
  const key = `${engine}:${id}`
  const { [key]: _, ...rest } = revealedTokens.value
  revealedTokens.value = rest
}

async function revealChannelsDir(): Promise<void> {
  await invoke('reveal_channels_dir')
}

export interface ProbeResult {
  online: boolean
  status: string
  models: string[]
  latencyMs: number
  resolvedBaseUrl: string | null
  endpointUrl: string | null
}

const probeResults = ref<Record<string, ProbeResult>>({})
const probing = ref<Record<string, boolean>>({})
const activeProbeCounts = new Map<string, number>()

/** 表单值直探参数(新建未保存渠道的「获取模型列表」):齐传时 Rust 侧绕过渠道文件 */
export interface ProbeDraft {
  baseUrl: string
  token: string
  adapter: 'claude-code' | 'codex' | 'openai-chat'
}

async function probeChannel(id: string, draft?: ProbeDraft): Promise<ProbeResult | null> {
  activeProbeCounts.set(id, (activeProbeCounts.get(id) ?? 0) + 1)
  probing.value = { ...probing.value, [id]: true }
  try {
    const result = await invoke<ProbeResult>('probe_channel', {
      id,
      baseUrl: draft?.baseUrl ?? null,
      token: draft?.token ?? null,
      protocol: null,
      adapter: draft?.adapter ?? null,
    })
    probeResults.value = { ...probeResults.value, [id]: result }
    return result
  } catch {
    return null
  } finally {
    const remaining = (activeProbeCounts.get(id) ?? 1) - 1
    if (remaining > 0) activeProbeCounts.set(id, remaining)
    else activeProbeCounts.delete(id)
    probing.value = { ...probing.value, [id]: remaining > 0 }
  }
}

async function probeAllChannels(): Promise<void> {
  // 内置虚拟渠道(official/official-direct)无文件无探测目标,跳过
  const ids = channels.value
    .filter(c => c.id !== OFFICIAL_CHANNEL_ID
      && c.id !== OFFICIAL_DIRECT_CHANNEL_ID)
    .map(c => c.id)
  await Promise.allSettled(ids.map(id => probeChannel(id)))
}

export interface CcSwitchProvider {
  id: string
  name: string
  baseUrl: string | null
  hasToken: boolean
  category: string | null
  isCurrent: boolean
  notes: string | null
  alreadyImported: boolean
}

async function scanCcSwitch(): Promise<CcSwitchProvider[]> {
  return invoke<CcSwitchProvider[]>('scan_cc_switch')
}

async function importCcSwitch(ids: string[]): Promise<number> {
  const count = await invoke<number>('import_cc_switch', { ids })
  await refreshChannels()
  return count
}

export function useChannels() {
  return {
    channels,
    defaultSessionChannels,
    defaultSessionModels,
    defaultSessionEfforts,
    defaultSessionChannel,
    defaultAgentEngine,
    defaultAgentChannels,
    defaultAgentModels,
    defaultAgentEfforts,
    defaultAgentChannel,
    defaultAgentModel,
    defaultAgentEffort,
    probeResults,
    probing,
    agentPreferences,
    refreshChannels,
    saveChannel,
    setOfficialDefaults,
    deleteChannel,
    setChannelEnabled,
    setDefaultSessionChannel,
    setDefaultSessionRuntime,
    setDefaultAgentEngine,
    setDefaultAgentModel,
    setDefaultAgentEffort,
    setAgentFeatureModel,
    revealChannelsDir,
    probeChannel,
    probeAllChannels,
    revealedTokens,
    revealToken,
    hideToken,
    loadAgentPreferences,
    scanCcSwitch,
    importCcSwitch,
  }
}
