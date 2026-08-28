import { describe, expect, it } from 'vitest'
import { latestChronologicalItem, newestFirst } from '@/utils/chronological'

describe('chronological collection order', () => {
  it('treats the source tail as the latest item', () => {
    expect(latestChronologicalItem(['oldest', 'middle', 'latest'])).toBe('latest')
    expect(latestChronologicalItem([])).toBeNull()
  })

  it('projects newest-first without mutating the source timeline', () => {
    const source = ['oldest', 'middle', 'latest']

    expect(newestFirst(source)).toEqual(['latest', 'middle', 'oldest'])
    expect(source).toEqual(['oldest', 'middle', 'latest'])
  })
})
