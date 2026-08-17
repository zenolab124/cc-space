export interface EngineRunConfig {
  model: string | null
  effort: string | null
  serviceTier: string | null
  channelId: string | null
  modelOverridden: boolean
  effortOverridden: boolean
}

export interface EngineServiceTier {
  id: string
  name: string
  description?: string | null
}

export interface EngineCapsuleModel {
  id: string
  label: string
  hidden?: boolean
  defaultEffort: string | null
  efforts: Array<{ id: string; description?: string | null }>
  defaultServiceTier: string | null
  serviceTiers: EngineServiceTier[]
}

export interface EngineCapsuleConfig {
  engineId: string
  engineName: string
  showFastMode: boolean
  channelId: string | null
  channelOverridden: boolean
  channelPending: boolean
  observedChannelLabel: string | null
  model: string | null
  effort: string | null
  modelOverridden: boolean
  effortOverridden: boolean
  serviceTier: string | null
  fastTier: EngineServiceTier | null
  fastModeUnavailableReason: string | null
  defaultModel: string | null
  defaultEffort: string | null
  models: EngineCapsuleModel[]
}

const configs = new Map<string, EngineRunConfig>()
const STORAGE_KEY_PREFIX = 'monet:engine-run-config:'

function storageKey(sessionId: string): string {
  return `${STORAGE_KEY_PREFIX}${sessionId}`
}

function parseNullableString(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null
}

function loadEngineRunConfig(sessionId: string): EngineRunConfig | null {
  if (typeof localStorage === 'undefined') return null
  try {
    const raw = localStorage.getItem(storageKey(sessionId))
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<EngineRunConfig>
    return {
      model: parseNullableString(parsed.model),
      effort: parseNullableString(parsed.effort),
      serviceTier: parseNullableString(parsed.serviceTier),
      channelId: parseNullableString(parsed.channelId),
      modelOverridden: parsed.modelOverridden === true,
      effortOverridden: parsed.effortOverridden === true,
    }
  } catch (_) {
    return null
  }
}

function persistEngineRunConfig(sessionId: string, config: EngineRunConfig) {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(storageKey(sessionId), JSON.stringify(config))
  } catch (_) {
    // 存储失败不阻塞会话发送；本次进程内仍由内存配置继续工作。
  }
}

export function engineRunConfig(sessionId: string): EngineRunConfig | null {
  let config = configs.get(sessionId)
  if (!config) {
    config = loadEngineRunConfig(sessionId) ?? undefined
    if (config) configs.set(sessionId, config)
  }
  return config ? { ...config } : null
}

export function resolveInitialEngineChannel(
  stored: EngineRunConfig | null,
  defaultChannelId: string | null,
  observedChannelId: string | null,
): string | null {
  if (stored?.channelId) return stored.channelId
  if (stored) return observedChannelId
  return defaultChannelId ?? observedChannelId
}

export function setEngineRunConfig(sessionId: string, config: EngineRunConfig) {
  configs.set(sessionId, { ...config })
  persistEngineRunConfig(sessionId, config)
}

export function clearEngineRunConfig(sessionId: string) {
  configs.delete(sessionId)
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.removeItem(storageKey(sessionId))
  } catch (_) {
    // 清理失败不影响工作台关闭流程。
  }
}

/** 仅驱逐当前进程缓存；持久化配置留给下次恢复同一会话。 */
export function evictEngineRunConfig(sessionId: string) {
  configs.delete(sessionId)
}

export function engineRuntimeOptions(sessionId: string): Record<string, unknown> {
  const config = engineRunConfig(sessionId)
  if (!config) return {}
  return {
    ...(config.model ? { model: config.model } : {}),
    ...(config.channelId ? { channelId: config.channelId } : {}),
  }
}

export function engineRuntimeChannel(sessionId: string): string | null {
  return engineRunConfig(sessionId)?.channelId ?? null
}

export function inheritEngineRunConfig(sourceSessionId: string, targetSessionId: string) {
  const config = engineRunConfig(sourceSessionId)
  if (config) setEngineRunConfig(targetSessionId, config)
}

export function resolveFastServiceTier(
  model: Pick<EngineCapsuleModel, 'serviceTiers'> | null | undefined,
): EngineServiceTier | null {
  if (!model) return null
  return model.serviceTiers.find(tier => tier.id === 'priority')
    ?? model.serviceTiers.find(tier => tier.name.trim().toLowerCase() === 'fast')
    ?? null
}

/** 仅在上游明确拒绝快速服务档时重试，避免把普通网络错误误判成可安全重放。 */
export function isFastServiceTierUnavailableError(cause: unknown): boolean {
  const message = cause instanceof Error
    ? cause.message
    : typeof cause === 'string'
      ? cause
      : (() => {
          try { return JSON.stringify(cause) }
          catch { return '' }
        })()
  if (!/(?:service[ _-]?tier|priority|fast(?:[ _-]?mode)?)/i.test(message)) return false
  return /(?:unavailable|unsupported|not\s+(?:available|enabled|eligible|supported)|disabled|ineligible|not\s+entitled|requires?\s+(?:access|credits?)|quota|credit|usage\s+limit)/i.test(message)
}
