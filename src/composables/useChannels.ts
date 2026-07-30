import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import i18n from '../locales'

export interface ChannelInfo {
  id: string
  name: string
  note: string | null
  baseUrl: string | null
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
}

export const APPLE_FM_CHANNEL_ID = 'apple-fm'

interface ChannelListResult {
  channels: ChannelInfo[]
  defaultSessionChannel: string | null
  defaultAgentChannel: string | null
  defaultAgentModel: string | null
}

export const OFFICIAL_CHANNEL_ID = 'official'
/** 官方直连:压制 CLI 配置中的第三方键,强制 Anthropic 官方 + OAuth(虚拟渠道,无文件) */
export const OFFICIAL_DIRECT_CHANNEL_ID = 'official-direct'

const channels = ref<ChannelInfo[]>([])
const defaultSessionChannel = ref<string | null>(null)
const defaultAgentChannel = ref<string | null>(null)
const defaultAgentModel = ref<string | null>(null)

export async function refreshChannels(): Promise<void> {
  try {
    const r = await invoke<ChannelListResult>('list_channels')
    channels.value = r.channels
    defaultSessionChannel.value = r.defaultSessionChannel
    defaultAgentChannel.value = r.defaultAgentChannel
    defaultAgentModel.value = r.defaultAgentModel
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
}

async function saveChannel(payload: SaveChannelPayload): Promise<void> {
  await invoke('save_channel', {
    id: payload.id,
    name: payload.name,
    baseUrl: payload.baseUrl,
    authToken: payload.authToken ?? null,
    note: payload.note ?? null,
    protocol: payload.protocol ?? null,
    scope: payload.scope ?? null,
    agentModel: payload.agentModel ?? null,
    availableModels: payload.availableModels ?? null,
    modelEnv: payload.modelEnv ?? null,
    defaultEffort: payload.defaultEffort ?? null,
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

async function setDefaultSessionChannel(id: string | null): Promise<void> {
  await invoke('set_default_session_channel', { id })
  defaultSessionChannel.value = id
}

async function setDefaultAgentModel(channel: string | null, model: string | null): Promise<void> {
  await invoke('set_default_agent_model', { channel, model })
  defaultAgentChannel.value = channel
  defaultAgentModel.value = model
}

async function setAgentFeatureModel(key: string, channel: string | null, model: string | null): Promise<void> {
  await invoke('set_agent_feature_model', { key, channel, model })
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

async function revealToken(id: string): Promise<string | null> {
  if (revealedTokens.value[id]) return revealedTokens.value[id]
  try {
    const token = await invoke<string | null>('get_channel_token', { id })
    if (token) revealedTokens.value = { ...revealedTokens.value, [id]: token }
    return token
  } catch { return null }
}

function hideToken(id: string) {
  const { [id]: _, ...rest } = revealedTokens.value
  revealedTokens.value = rest
}

async function revealChannelsDir(): Promise<void> {
  await invoke('reveal_channels_dir')
}

interface ProbeResult {
  online: boolean
  status: string
  models: string[]
  latencyMs: number
}

const probeResults = ref<Record<string, ProbeResult>>({})
const probing = ref<Record<string, boolean>>({})

/** 表单值直探参数(新建未保存渠道的「获取模型列表」):齐传时 Rust 侧绕过渠道文件 */
export interface ProbeDraft {
  baseUrl: string
  token: string
  protocol: string
}

async function probeChannel(id: string, draft?: ProbeDraft): Promise<ProbeResult | null> {
  probing.value = { ...probing.value, [id]: true }
  try {
    const result = await invoke<ProbeResult>('probe_channel', {
      id,
      baseUrl: draft?.baseUrl ?? null,
      token: draft?.token ?? null,
      protocol: draft?.protocol ?? null,
    })
    probeResults.value = { ...probeResults.value, [id]: result }
    return result
  } catch {
    return null
  } finally {
    probing.value = { ...probing.value, [id]: false }
  }
}

async function probeAllChannels(): Promise<void> {
  // 内置虚拟渠道(official/official-direct)无文件无探测目标,跳过
  const ids = channels.value
    .filter(c => c.id !== OFFICIAL_CHANNEL_ID && c.id !== OFFICIAL_DIRECT_CHANNEL_ID)
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
    defaultSessionChannel,
    defaultAgentChannel,
    defaultAgentModel,
    probeResults,
    probing,
    agentPreferences,
    refreshChannels,
    saveChannel,
    setOfficialDefaults,
    deleteChannel,
    setChannelEnabled,
    setDefaultSessionChannel,
    setDefaultAgentModel,
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
