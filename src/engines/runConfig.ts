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

export function engineRunConfig(sessionId: string): EngineRunConfig | null {
  const config = configs.get(sessionId)
  return config ? { ...config } : null
}

export function resolveInitialEngineChannel(
  stored: EngineRunConfig | null,
  defaultChannelId: string | null,
): string | null {
  return stored ? stored.channelId : defaultChannelId
}

export function setEngineRunConfig(sessionId: string, config: EngineRunConfig) {
  configs.set(sessionId, { ...config })
}

export function clearEngineRunConfig(sessionId: string) {
  configs.delete(sessionId)
}

export function engineRuntimeOptions(sessionId: string): Record<string, unknown> {
  const config = configs.get(sessionId)
  if (!config) return {}
  return {
    ...(config.model ? { model: config.model } : {}),
    ...(config.channelId ? { channelId: config.channelId } : {}),
  }
}

export function engineRuntimeChannel(sessionId: string): string | null {
  return configs.get(sessionId)?.channelId ?? null
}

export function inheritEngineRunConfig(sourceSessionId: string, targetSessionId: string) {
  const config = configs.get(sourceSessionId)
  if (config) configs.set(targetSessionId, { ...config })
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
