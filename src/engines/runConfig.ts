export interface EngineRunConfig {
  model: string | null
  effort: string | null
}

const configs = new Map<string, EngineRunConfig>()

export function engineRunConfig(sessionId: string): EngineRunConfig | null {
  const config = configs.get(sessionId)
  return config ? { ...config } : null
}

export function setEngineRunConfig(sessionId: string, config: EngineRunConfig) {
  configs.set(sessionId, { ...config })
}

export function clearEngineRunConfig(sessionId: string) {
  configs.delete(sessionId)
}

export function inheritEngineRunConfig(sourceSessionId: string, targetSessionId: string) {
  const config = configs.get(sourceSessionId)
  if (config) configs.set(targetSessionId, { ...config })
}
