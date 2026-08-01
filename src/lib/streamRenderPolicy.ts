export type StreamRenderPriority = 'active' | 'visible' | 'hidden'

export const STREAM_RENDER_INTERVAL_MS: Record<StreamRenderPriority, number> = {
  active: 16,
  visible: 66,
  hidden: 250,
}

export function resolveStreamRenderPriority(visible: boolean, active: boolean): StreamRenderPriority {
  if (!visible) return 'hidden'
  return active ? 'active' : 'visible'
}

export function streamRenderInterval(priority: StreamRenderPriority, documentVisible: boolean): number {
  return documentVisible ? STREAM_RENDER_INTERVAL_MS[priority] : STREAM_RENDER_INTERVAL_MS.hidden
}

/**
 * 让低刷新率会话仍在相同平滑窗口内追平：刷新间隔越长，单次吐出的比例越大。
 */
export function smoothTakeForElapsed(bufferLength: number, elapsedMs: number, windowMs: number): number {
  if (bufferLength <= 0) return 0
  return Math.max(1, Math.ceil((bufferLength * Math.max(1, elapsedMs)) / windowMs))
}
