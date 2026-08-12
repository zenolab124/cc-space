export type ScrollFollowMode = 'following' | 'reading'

export type ScrollFollowTransition = 'detach' | 'resume' | 'stream-finished'

export interface ScrollFollowState {
  mode: ScrollFollowMode
  epoch: number
}

export interface ScrollFollowToken {
  readonly epoch: number
}

export function createScrollFollowState(
  mode: ScrollFollowMode = 'following',
): ScrollFollowState {
  return { mode, epoch: 0 }
}

export function transitionScrollFollow(
  state: ScrollFollowState,
  transition: ScrollFollowTransition,
): ScrollFollowState {
  switch (transition) {
    case 'detach':
      return { mode: 'reading', epoch: state.epoch + 1 }
    case 'resume':
      return { mode: 'following', epoch: state.epoch + 1 }
    case 'stream-finished':
      return state
  }
}

export function captureScrollFollowToken(
  state: ScrollFollowState,
): ScrollFollowToken | null {
  return state.mode === 'following' ? { epoch: state.epoch } : null
}

export function canApplyScrollFollowToken(
  state: ScrollFollowState,
  token: ScrollFollowToken | null,
): boolean {
  return token !== null
    && state.mode === 'following'
    && token.epoch === state.epoch
}

export interface ScrollViewportGeometry {
  scrollTop: number
  scrollHeight: number
  clientHeight: number
}

/**
 * 判断视口是否确实还有向上阅读的空间。
 * 内容未溢出或仍停在顶部时，触控板噪声不应影响流式跟随。
 */
export function hasUpwardScrollRange(
  geometry: ScrollViewportGeometry,
  tolerance = 0.5,
): boolean {
  const { scrollTop, scrollHeight, clientHeight } = geometry
  if (
    !Number.isFinite(scrollTop)
    || !Number.isFinite(scrollHeight)
    || !Number.isFinite(clientHeight)
    || !Number.isFinite(tolerance)
    || tolerance < 0
  ) return false
  return scrollHeight - clientHeight > tolerance && scrollTop > tolerance
}

export interface ScrollFollowDetachCheck {
  geometry: ScrollViewportGeometry
  previousScrollTop: number
  upwardIntentAt: number
  downwardIntentAt: number
  now: number
  intentWindow: number
  bottomThreshold: number
}

/**
 * wheel 只记录方向，不直接关闭跟随。只有最新的用户意图是向上、
 * 视口确实向上移动且已离开底部阈值时，才进入阅读状态。
 */
export function shouldDetachScrollFollowAfterMovement(
  check: ScrollFollowDetachCheck,
): boolean {
  const {
    geometry,
    previousScrollTop,
    upwardIntentAt,
    downwardIntentAt,
    now,
    intentWindow,
    bottomThreshold,
  } = check
  if (
    !Number.isFinite(previousScrollTop)
    || !Number.isFinite(upwardIntentAt)
    || (!Number.isFinite(downwardIntentAt) && downwardIntentAt !== Number.NEGATIVE_INFINITY)
    || !Number.isFinite(now)
    || !Number.isFinite(intentWindow)
    || !Number.isFinite(bottomThreshold)
    || intentWindow < 0
    || bottomThreshold < 0
  ) return false

  const intentAge = now - upwardIntentAt
  const distanceFromBottom = Math.max(
    0,
    geometry.scrollHeight - geometry.scrollTop - geometry.clientHeight,
  )
  return geometry.scrollTop < previousScrollTop - 0.5
    && distanceFromBottom > bottomThreshold
    && hasUpwardScrollRange(geometry)
    && upwardIntentAt > downwardIntentAt
    && intentAge >= 0
    && intentAge <= intentWindow
}

interface MessageGroupKeyRecord {
  uuid?: string | null
}

export interface MessageGroupKeySource {
  user?: MessageGroupKeyRecord | null
  responses?: readonly MessageGroupKeyRecord[]
}

function nonEmptyUuid(record: MessageGroupKeyRecord | null | undefined): string | null {
  return typeof record?.uuid === 'string' && record.uuid.length > 0
    ? record.uuid
    : null
}

export function stableMessageGroupKey(
  sessionId: string,
  group: MessageGroupKeySource,
  fallbackIndex: number,
): string {
  const userUuid = nonEmptyUuid(group.user)
  const responseUuid = group.responses
    ?.map(nonEmptyUuid)
    .find((uuid): uuid is string => uuid !== null)
  const identity = userUuid
    ? `user:${userUuid}`
    : responseUuid
      ? `response:${responseUuid}`
      : `index:${fallbackIndex}`

  return JSON.stringify([sessionId, identity])
}

export type VirtualScrollDirection = 'forward' | 'backward' | null

export interface VirtualItemSizeChange {
  scrollDirection: VirtualScrollDirection
  upwardGestureActive: boolean
  itemStart: number
  itemSize: number
  scrollOffset: number
  delta: number
}

/**
 * 虚拟项测量变化能否补偿阅读锚点。旧测量盒必须完整位于视口上方；
 * 若跨越视口，尺寸变化发生在视觉锚点处或其下方，不应移动 scrollTop。
 */
export function shouldCompensateVirtualItemSizeChange(
  change: VirtualItemSizeChange,
): boolean {
  if (change.scrollDirection === 'backward') return false
  if (change.upwardGestureActive) return false
  if (change.delta === 0) return false
  if (
    !Number.isFinite(change.itemStart)
    || !Number.isFinite(change.itemSize)
    || !Number.isFinite(change.scrollOffset)
    || !Number.isFinite(change.delta)
    || change.itemSize < 0
  ) return false

  return change.itemStart + change.itemSize <= change.scrollOffset
}
