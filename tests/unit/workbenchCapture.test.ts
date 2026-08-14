import { describe, expect, it } from 'vitest'
import {
  calculateNativeCaptureOffsets,
  calculatePanoramaLayout,
  workbenchCaptureFilename,
} from '../../src/utils/workbenchCapture'

describe('workbench panorama capture', () => {
  it('adds only the horizontally clipped width to the app shell', () => {
    expect(calculatePanoramaLayout(1200, 800, 900, 2100)).toMatchObject({
      width: 2400,
      height: 800,
      pixelRatio: 2,
    })
  })

  it('does not shrink the shell when columns fit in the viewport', () => {
    expect(calculatePanoramaLayout(1200, 800, 900, 700).width).toBe(1200)
  })

  it('reduces output scale before exceeding the canvas budget', () => {
    const layout = calculatePanoramaLayout(12_000, 1_000, 1_000, 12_000)
    expect(layout.pixelRatio).toBeLessThan(2)
    expect(layout.width * layout.pixelRatio).toBeLessThanOrEqual(14_000)
  })

  it('builds a filesystem-safe timestamped filename', () => {
    const now = new Date(2026, 7, 15, 9, 8, 7)
    expect(workbenchCaptureFilename('My / Workbench:*', undefined, now))
      .toBe('monet-My-Workbench-20260815-090807.png')
    expect(workbenchCaptureFilename('My / Workbench:*', 'native', now))
      .toBe('monet-My-Workbench-webkit-20260815-090807.png')
    expect(workbenchCaptureFilename('My / Workbench:*', 'canvas', now))
      .toBe('monet-My-Workbench-canvas-20260815-090807.png')
  })

  it('covers native snapshots from the first viewport through the maximum scroll', () => {
    expect(calculateNativeCaptureOffsets(900, 2100)).toEqual([0, 900, 1200])
    expect(calculateNativeCaptureOffsets(900, 700)).toEqual([0])
  })
})
