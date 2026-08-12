import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('standard engine session capabilities', () => {
  it('adds controlled capability IDs to every thread lifecycle request', () => {
    const client = source('../../src/engines/client.ts')

    expect(client).toContain('sessionCapabilities: capabilities')
    expect(client.match(/withSessionCapabilities\(options\)/g)).toHaveLength(3)
    expect(client).toContain('capabilityFingerprint: configured.capabilityFingerprint')
  })

  it('reattaches when the capability fingerprint changes and reports the feature', () => {
    const detail = source('../../src/components/engine/EngineSessionDetail.vue')

    expect(detail).toContain('attachedCapabilityFingerprint.value === capabilityFingerprint')
    expect(detail).toContain('attachedCapabilityFingerprint.value !== capabilityFingerprint')
    expect(detail).toContain('draft.attachedCapabilityFingerprint\n          ?? capabilityFingerprint')
    expect(detail).toContain(':features="htmlVisualEnabled ? [t(\'settings.htmlVisual\')] : []"')
  })
})
