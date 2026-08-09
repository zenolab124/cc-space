import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const source = readFileSync(
  fileURLToPath(new URL('../../src/composables/useWorkbench.ts', import.meta.url)),
  'utf8',
)

describe('standard-engine workbench drafts', () => {
  it('keeps pre-message references within the runtime that created them', () => {
    expect(source).toContain('runtimeScope: string')
    expect(source).toContain('String(performance.timeOrigin)')
    expect(source).not.toContain('sessionStorage.getItem(ENGINE_DRAFT_SCOPE_KEY)')
    expect(source).toContain('runtimeScope: engineDraftRuntimeScope')
    expect(source).toContain('draft.runtimeScope !== engineDraftRuntimeScope')
    expect(source).toContain('removeSession(sid)')
  })

  it('remembers the channel already attached by thread creation', () => {
    expect(source).toContain('attachedChannel: string | null')
    expect(source).toContain("typeof draft.attachedChannel === 'string'")
  })
})
