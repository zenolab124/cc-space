export const SESSION_READING_ENGINE_IDS = ['claude-code', 'codex'] as const

export type SessionReadingEngineId = typeof SESSION_READING_ENGINE_IDS[number]

export function isSessionReadingEngineId(value: string): value is SessionReadingEngineId {
  return SESSION_READING_ENGINE_IDS.includes(value as SessionReadingEngineId)
}
