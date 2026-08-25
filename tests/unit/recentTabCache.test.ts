import { describe, expect, it } from 'vitest'
import { touchRecentTab } from '../../src/utils/recentTabCache'

describe('recent workbench tab cache', () => {
  it('keeps the active tab first and evicts the least recently visited tab', () => {
    const valid = new Set(['a', 'b', 'c', 'd', 'e'])

    expect(touchRecentTab(['d', 'c', 'b', 'a'], 'e', valid, 4))
      .toEqual(['e', 'd', 'c', 'b'])
  })

  it('moves a revisited tab to the front without duplicating it', () => {
    const valid = new Set(['a', 'b', 'c'])

    expect(touchRecentTab(['c', 'b', 'a'], 'a', valid, 4))
      .toEqual(['a', 'c', 'b'])
  })

  it('drops closed tabs before applying the cache limit', () => {
    const valid = new Set(['a', 'c', 'd'])

    expect(touchRecentTab(['c', 'b', 'a'], 'd', valid, 4))
      .toEqual(['d', 'c', 'a'])
  })

  it('returns an empty cache when no active tab can be mounted', () => {
    expect(touchRecentTab(['a'], 'missing', new Set(['a']), 4)).toEqual([])
    expect(touchRecentTab(['a'], 'a', new Set(['a']), 0)).toEqual([])
  })
})
