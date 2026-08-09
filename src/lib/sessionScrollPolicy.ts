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
