export const SESSION_CAPABILITY_IDS = ['artifact_preview', 'html_visual'] as const

export type SessionCapabilityId = (typeof SESSION_CAPABILITY_IDS)[number]

export interface SessionCapabilityState {
  artifactPreview: boolean
  htmlVisual: boolean
}

/** 按注册顺序生成受控能力 ID；IPC 不接受任意 prompt/env/MCP 定义。 */
export function resolveSessionCapabilities(state: SessionCapabilityState): SessionCapabilityId[] {
  const enabled = new Set<SessionCapabilityId>()
  if (state.artifactPreview) enabled.add('artifact_preview')
  if (state.htmlVisual) enabled.add('html_visual')
  return SESSION_CAPABILITY_IDS.filter(id => enabled.has(id))
}

export function sessionCapabilityFingerprint(ids: readonly SessionCapabilityId[]): string {
  const enabled = new Set(ids)
  return JSON.stringify(SESSION_CAPABILITY_IDS.filter(id => enabled.has(id)))
}
