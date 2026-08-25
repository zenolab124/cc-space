import { describe, expect, it } from 'vitest'
import { reorderWorkbenchTabs } from '../../src/utils/workbenchTabs'

describe('workbench tab ordering', () => {
  it('moves a tab by stable ID without changing the tab objects', () => {
    const tabs = [{ id: 'a' }, { id: 'b' }, { id: 'c' }]
    const result = reorderWorkbenchTabs(tabs, 'a', 'c')

    expect(result.map(tab => tab.id)).toEqual(['b', 'c', 'a'])
    expect(result[2]).toBe(tabs[0])
  })

  it('leaves the order unchanged for unknown or identical IDs', () => {
    const tabs = [{ id: 'a' }, { id: 'b' }]

    expect(reorderWorkbenchTabs(tabs, 'missing', 'b')).toEqual(tabs)
    expect(reorderWorkbenchTabs(tabs, 'a', 'a')).toEqual(tabs)
  })
})
