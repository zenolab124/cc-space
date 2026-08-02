export const SESSION_CAPABILITY_IDS = ['html_visual'] as const

export type SessionCapabilityId = (typeof SESSION_CAPABILITY_IDS)[number]

export interface SessionCapabilityState {
  htmlVisual: boolean
}

/** 按注册顺序生成受控能力 ID；IPC 不接受任意 prompt/env/MCP 定义。 */
export function resolveSessionCapabilities(state: SessionCapabilityState): SessionCapabilityId[] {
  const enabled = new Set<SessionCapabilityId>()
  if (state.htmlVisual) enabled.add('html_visual')
  return SESSION_CAPABILITY_IDS.filter(id => enabled.has(id))
}
