import { describe, expect, it } from 'vitest'
import {
  resolveStreamRenderPriority,
  smoothTakeForElapsed,
  streamRenderInterval,
} from '../../src/lib/streamRenderPolicy'

describe('stream render policy', () => {
  it('prioritizes only the active visible surface', () => {
    expect(resolveStreamRenderPriority(true, true)).toBe('active')
    expect(resolveStreamRenderPriority(true, false)).toBe('visible')
    expect(resolveStreamRenderPriority(false, true)).toBe('hidden')
  })

  it('caps every session when the whole document is hidden', () => {
    expect(streamRenderInterval('active', true)).toBe(16)
    expect(streamRenderInterval('visible', true)).toBe(66)
    expect(streamRenderInterval('hidden', true)).toBe(250)
    expect(streamRenderInterval('active', false)).toBe(250)
  })

  it('drains the same proportion regardless of refresh cadence', () => {
    expect(smoothTakeForElapsed(300, 16, 300)).toBe(16)
    expect(smoothTakeForElapsed(300, 66, 300)).toBe(66)
    expect(smoothTakeForElapsed(300, 250, 300)).toBe(250)
  })
})
