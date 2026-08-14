import { describe, expect, it } from 'vitest'
import {
  resolveSessionCapabilities,
  sessionCapabilityFingerprint,
  SESSION_CAPABILITY_IDS,
} from '@/features/sessionCapabilities'

describe('resolveSessionCapabilities', () => {
  it('keeps artifact preview enabled when HTML visual is disabled', () => {
    expect(resolveSessionCapabilities({ artifactPreview: true, htmlVisual: false }))
      .toEqual(['artifact_preview'])
  })

  it('returns the registered HTML capability when enabled', () => {
    expect(resolveSessionCapabilities({ artifactPreview: false, htmlVisual: true }))
      .toEqual(['html_visual'])
  })

  it('follows registry order without duplicate IDs', () => {
    const resolved = resolveSessionCapabilities({ artifactPreview: true, htmlVisual: true })

    expect(resolved).toEqual(SESSION_CAPABILITY_IDS)
    expect(new Set(resolved).size).toBe(resolved.length)
  })
})

describe('sessionCapabilityFingerprint', () => {
  it('serializes the ordered capability set for runtime attachment identity', () => {
    expect(sessionCapabilityFingerprint([])).toBe('[]')
    expect(sessionCapabilityFingerprint(['html_visual', 'html_visual'])).toBe('["html_visual"]')
    expect(sessionCapabilityFingerprint(['html_visual', 'artifact_preview']))
      .toBe('["artifact_preview","html_visual"]')
  })
})
