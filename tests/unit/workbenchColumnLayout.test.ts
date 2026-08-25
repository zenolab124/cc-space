import { describe, expect, it } from 'vitest'
import {
  insertColumnWidth,
  removeColumnWidth,
  resizeColumnWidth,
} from '../../src/utils/workbenchColumnLayout'

describe('workbench column layout', () => {
  it('adds a new column without changing existing widths', () => {
    expect(insertColumnWidth([420, 560], 1, 360)).toEqual([420, 360, 560])
  })

  it('removes only the target column width', () => {
    expect(removeColumnWidth([420, 360, 560], 1)).toEqual([420, 560])
  })

  it('resizes only the target column and respects the minimum', () => {
    expect(resizeColumnWidth([420, 560], 0, 620, 360)).toEqual([620, 560])
    expect(resizeColumnWidth([420, 560], 1, 120, 360)).toEqual([420, 360])
  })
})
