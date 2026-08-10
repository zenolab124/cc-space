import { describe, expect, it } from 'vitest'
import { shouldIncludeRuntimeTailSegment } from '../../src/engines/runtimeState'

describe('engine runtime monitor tail', () => {
  it('applies the same thought visibility policy as the conversation surface', () => {
    expect(shouldIncludeRuntimeTailSegment('codex', {
      kind: 'reasoning',
      text: 'private reasoning',
      visibility: 'summary',
    })).toBe(false)
    expect(shouldIncludeRuntimeTailSegment('codex', {
      kind: 'text',
      text: 'working',
      phase: 'progress',
    })).toBe(false)
    expect(shouldIncludeRuntimeTailSegment('codex', {
      kind: 'text',
      text: 'final',
      phase: 'final',
    })).toBe(true)
    expect(shouldIncludeRuntimeTailSegment('claude-code', {
      kind: 'reasoning',
      text: 'visible summary',
      visibility: 'summary',
    })).toBe(true)
  })
})
