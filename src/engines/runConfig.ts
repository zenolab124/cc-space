export interface EngineRunConfig {
  model: string | null
  effort: string | null
  channelId: string | null
  modelOverridden: boolean
  effortOverridden: boolean
}

export interface EngineCapsuleModel {
  id: string
  label: string
  hidden?: boolean
  defaultEffort: string | null
  efforts: Array<{ id: string; description?: string | null }>
}

export interface EngineCapsuleConfig {
  engineId: string
  engineName: string
  channelId: string | null
  model: string | null
  effort: string | null
  modelOverridden: boolean
  effortOverridden: boolean
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
