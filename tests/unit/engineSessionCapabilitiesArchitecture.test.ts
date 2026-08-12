import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('standard engine session capabilities', () => {
  it('adds controlled capability IDs to every thread lifecycle request', () => {
    const client = source('../../src/engines/client.ts')

    expect(client).toContain('sessionCapabilities: collectSessionCapabilities()')
    expect(client).toMatch(/engine_attach_session[\s\S]{0,220}withSessionCapabilities\(options\)/)
    expect(client).toMatch(/engine_create_session[\s\S]{0,220}withSessionCapabilities\(options\)/)
    expect(client).toMatch(/engine_fork_session[\s\S]{0,220}withSessionCapabilities\(options\)/)
  })

  it('reattaches when the capability fingerprint changes and reports the feature', () => {
    const detail = source('../../src/components/engine/EngineSessionDetail.vue')

    expect(detail).toContain('attachedCapabilityFingerprint.value === capabilityFingerprint')
    expect(detail).toContain('attachedCapabilityFingerprint.value !== capabilityFingerprint')
    expect(detail).toContain(':features="htmlVisualEnabled ? [t(\'settings.htmlVisual\')] : []"')
  })
})
