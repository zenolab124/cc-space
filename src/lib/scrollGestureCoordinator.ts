export type ScrollAxis = 'x' | 'y'

export interface ScrollHandoff {
  axis: ScrollAxis
  deltaX: number
  deltaY: number
  deltaMode: number
}

type ScrollSurfaceConsumer = (delta: number, handoff: ScrollHandoff) => void

const GESTURE_IDLE_MS = 160
const MAX_HANDOFF_DELTA = 2_000
const managedFrames = new Map<HTMLIFrameElement, HTMLElement>()
const scrollSurfaces = new WeakMap<HTMLElement, Partial<Record<ScrollAxis, ScrollSurfaceConsumer>>>()

let hostGestureTimer = 0
let listening = false

function setScrollShieldActive(shield: HTMLElement, active: boolean) {
  shield.toggleAttribute('data-monet-scroll-shield-active', active)
}

function endHostGesture() {
  if (hostGestureTimer) window.clearTimeout(hostGestureTimer)
  hostGestureTimer = 0
  managedFrames.forEach(shield => setScrollShieldActive(shield, false))
}

function beginHostGesture() {
  managedFrames.forEach(shield => setScrollShieldActive(shield, true))
  if (hostGestureTimer) window.clearTimeout(hostGestureTimer)
  hostGestureTimer = window.setTimeout(endHostGesture, GESTURE_IDLE_MS)
}

function onHostWheel(event: WheelEvent) {
  // Ctrl + wheel 在 WebKit 中也用于触控板捏合缩放，不能纳入滚动手势。
  if (event.ctrlKey || (!event.deltaX && !event.deltaY)) return
  beginHostGesture()
}

function installHostListeners() {
  if (listening || typeof window === 'undefined') return
  listening = true
  window.addEventListener('wheel', onHostWheel, { capture: true, passive: true })
  window.addEventListener('pointerdown', endHostGesture, { capture: true, passive: true })
  window.addEventListener('blur', endHostGesture)
}

function removeHostListeners() {
  if (!listening || typeof window === 'undefined') return
  listening = false
  endHostGesture()
  window.removeEventListener('wheel', onHostWheel, { capture: true })
  window.removeEventListener('pointerdown', endHostGesture, { capture: true })
  window.removeEventListener('blur', endHostGesture)
}

/**
 * 登记需要参与窗口级滚动手势协调的 iframe。
 * 代理节点常驻 iframe 上方但默认不参与命中；外层手势存续期间由它接住滚轮，
 * 避免依赖 WebKit 在滚动中途重新计算 iframe 自身的 pointer-events。
 */
export function registerManagedScrollFrame(frame: HTMLIFrameElement, shield: HTMLElement): () => void {
  managedFrames.set(frame, shield)
  if (hostGestureTimer) setScrollShieldActive(shield, true)
  installHostListeners()

  return () => {
    managedFrames.delete(frame)
    setScrollShieldActive(shield, false)
    if (managedFrames.size === 0) removeHostListeners()
  }
}

/** 登记可接收 iframe 边界滚动量的外层滚动面。 */
export function registerScrollSurface(
  element: HTMLElement,
  axis: ScrollAxis,
  consume: ScrollSurfaceConsumer,
): () => void {
  const surfaces = scrollSurfaces.get(element) ?? {}
  surfaces[axis] = consume
  scrollSurfaces.set(element, surfaces)

  return () => {
    const current = scrollSurfaces.get(element)
    if (!current || current[axis] !== consume) return
    delete current[axis]
    if (!current.x && !current.y) scrollSurfaces.delete(element)
  }
}

function normalizeDelta(handoff: ScrollHandoff): number {
  const raw = handoff.axis === 'x' ? handoff.deltaX : handoff.deltaY
  const unit = handoff.deltaMode === WheelEvent.DOM_DELTA_LINE
    ? 16
    : handoff.deltaMode === WheelEvent.DOM_DELTA_PAGE
      ? handoff.axis === 'x' ? window.innerWidth : window.innerHeight
      : 1
  return Math.max(-MAX_HANDOFF_DELTA, Math.min(MAX_HANDOFF_DELTA, raw * unit))
}

/**
 * 接收可信 sandbox 桥上报的边界滚动量，交给 iframe 外最近的同轴滚动面。
 * 返回 false 表示当前 DOM 层级没有登记对应方向的滚动面。
 */
export function handoffManagedFrameWheel(frame: HTMLIFrameElement, handoff: ScrollHandoff): boolean {
  if (!managedFrames.has(frame)) return false
  const delta = normalizeDelta(handoff)
  if (!Number.isFinite(delta) || Math.abs(delta) < 0.01) return false

  beginHostGesture()
  for (let node = frame.parentElement; node; node = node.parentElement) {
    const consume = scrollSurfaces.get(node)?.[handoff.axis]
    if (!consume) continue
    consume(delta, handoff)
    return true
  }
  return false
}
