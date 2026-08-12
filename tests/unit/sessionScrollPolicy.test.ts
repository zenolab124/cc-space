import { describe, expect, it } from 'vitest'
import {
  canApplyScrollFollowToken,
  captureScrollFollowToken,
  createScrollFollowState,
  hasUpwardScrollRange,
  shouldCompensateVirtualItemSizeChange,
  stableMessageGroupKey,
  transitionScrollFollow,
  type VirtualItemSizeChange,
} from '../../src/lib/sessionScrollPolicy'

describe('session scroll follow policy', () => {
  it('invalidates a queued follow write after the user detaches', () => {
    const following = createScrollFollowState()
    const queued = captureScrollFollowToken(following)

    const reading = transitionScrollFollow(following, 'detach')

    expect(reading).toEqual({ mode: 'reading', epoch: 1 })
    expect(canApplyScrollFollowToken(reading, queued)).toBe(false)
    expect(captureScrollFollowToken(reading)).toBeNull()
  })

  it('does not resume follow when streaming finishes', () => {
    const reading = transitionScrollFollow(createScrollFollowState(), 'detach')
    const finished = transitionScrollFollow(reading, 'stream-finished')

    expect(finished).toBe(reading)
    expect(finished.mode).toBe('reading')
    expect(finished.epoch).toBe(1)
  })

  it('accepts only the token issued by the latest explicit resume', () => {
    const initial = createScrollFollowState()
    const stale = captureScrollFollowToken(initial)
    const reading = transitionScrollFollow(initial, 'detach')
    const resumed = transitionScrollFollow(reading, 'resume')
    const current = captureScrollFollowToken(resumed)

    expect(canApplyScrollFollowToken(resumed, stale)).toBe(false)
    expect(canApplyScrollFollowToken(resumed, current)).toBe(true)
  })

  it('ignores upward wheel noise until the viewport can actually move upward', () => {
    expect(hasUpwardScrollRange({
      scrollTop: 0,
      scrollHeight: 400,
      clientHeight: 600,
    })).toBe(false)
    expect(hasUpwardScrollRange({
      scrollTop: 0,
      scrollHeight: 900,
      clientHeight: 600,
    })).toBe(false)
    expect(hasUpwardScrollRange({
      scrollTop: 300,
      scrollHeight: 900,
      clientHeight: 600,
    })).toBe(true)
  })
})

describe('stable message group key', () => {
  it('stays stable while a group grows and changes across sessions', () => {
    const initial = {
      user: { uuid: 'user-a' },
      responses: [{ uuid: 'response-a' }],
    }
    const grown = {
      user: { uuid: 'user-a' },
      responses: [{ uuid: 'response-a' }, { uuid: 'response-b' }],
    }

    expect(stableMessageGroupKey('session-a', initial, 3))
      .toBe(stableMessageGroupKey('session-a', grown, 3))
    expect(stableMessageGroupKey('session-a', grown, 3))
      .not.toBe(stableMessageGroupKey('session-b', grown, 3))
  })

  it('uses the first stable response identity before falling back to the index', () => {
    expect(stableMessageGroupKey(
      'session-a',
      { user: null, responses: [{ uuid: null }, { uuid: 'response-a' }] },
      2,
    )).toBe(stableMessageGroupKey(
      'session-a',
      { user: null, responses: [{ uuid: null }, { uuid: 'response-a' }, { uuid: 'response-b' }] },
      8,
    ))

    expect(stableMessageGroupKey('session-a', { user: null, responses: [] }, 2))
      .not.toBe(stableMessageGroupKey('session-a', { user: null, responses: [] }, 3))
  })
})

describe('virtual item size compensation policy', () => {
  const eligible: VirtualItemSizeChange = {
    scrollDirection: 'forward',
    upwardGestureActive: false,
    itemStart: 100,
    itemSize: 200,
    scrollOffset: 300,
    delta: 1_800,
  }

  it('allows a size correction only when the old item is fully above the viewport', () => {
    expect(shouldCompensateVirtualItemSizeChange(eligible)).toBe(true)
    expect(shouldCompensateVirtualItemSizeChange({
      ...eligible,
      scrollDirection: null,
    })).toBe(true)
    expect(shouldCompensateVirtualItemSizeChange({
      ...eligible,
      itemStart: 150,
    })).toBe(false)
  })

  it('rejects correction during backward scrolling or an upward gesture window', () => {
    expect(shouldCompensateVirtualItemSizeChange({
      ...eligible,
      scrollDirection: 'backward',
    })).toBe(false)
    expect(shouldCompensateVirtualItemSizeChange({
      ...eligible,
      upwardGestureActive: true,
    })).toBe(false)
  })

  it('preserves the reading anchor for an idle shrink and rejects invalid geometry', () => {
    expect(shouldCompensateVirtualItemSizeChange({
      ...eligible,
      delta: -100,
    })).toBe(true)
    expect(shouldCompensateVirtualItemSizeChange({
      ...eligible,
      scrollOffset: Number.NaN,
    })).toBe(false)
  })
})
