import { describe, expect, it } from 'vitest'
import { fillColumnWidthsProportionally } from '../../src/utils/workbenchColumnLayout'

describe('workbench column layout', () => {
  it('fills available width using the existing column proportions', () => {
    expect(fillColumnWidthsProportionally([400, 600], 1500)).toEqual([600, 900])
  })

  it('assigns rounding remainder without leaving a trailing gap', () => {
    const result = fillColumnWidthsProportionally([361, 360, 360], 1400)

    expect(result.reduce((sum, width) => sum + width, 0)).toBe(1400)
    expect(Math.max(...result) - Math.min(...result)).toBeLessThanOrEqual(2)
  })

  it('preserves every remaining width while horizontal overflow remains', () => {
    expect(fillColumnWidthsProportionally([520, 480, 460], 1000)).toEqual([520, 480, 460])
  })
})
