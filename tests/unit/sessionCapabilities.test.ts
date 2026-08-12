import { describe, expect, it } from 'vitest'
import {
  resolveSessionCapabilities,
  sessionCapabilityFingerprint,
  SESSION_CAPABILITY_IDS,
} from '@/features/sessionCapabilities'

describe('resolveSessionCapabilities', () => {
  it('returns no capabilities when HTML visual is disabled', () => {
    expect(resolveSessionCapabilities({ htmlVisual: false })).toEqual([])
  })

  it('returns the registered HTML capability when enabled', () => {
    expect(resolveSessionCapabilities({ htmlVisual: true })).toEqual(['html_visual'])
  })

  it('follows registry order without duplicate IDs', () => {
    const resolved = resolveSessionCapabilities({ htmlVisual: true })

    expect(resolved).toEqual(SESSION_CAPABILITY_IDS)
    expect(new Set(resolved).size).toBe(resolved.length)
  })
})

describe('sessionCapabilityFingerprint', () => {
  it('serializes the ordered capability set for runtime attachment identity', () => {
    expect(sessionCapabilityFingerprint([])).toBe('[]')
    expect(sessionCapabilityFingerprint(['html_visual', 'html_visual'])).toBe('["html_visual"]')
  })
})
