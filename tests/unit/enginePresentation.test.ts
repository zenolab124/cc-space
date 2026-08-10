import { describe, expect, it } from 'vitest'
import { engineResponseMeta, resolveEnginePresentation } from '../../src/engines/presentation'
import type { ConversationRecord, SessionRef } from '../../src/engines/types'

const session: SessionRef = {
  engine: { engineId: 'codex', instanceId: 'default' },
  nativeId: 'session',
}

describe('engine presentation registry', () => {
  it('keeps engine identity decisions outside render components', () => {
    expect(resolveEnginePresentation('claude', 'Claude Code')).toEqual({
      accent: 'claude',
      displayName: 'Claude Code',
      showThoughtProcess: true,
    })
    expect(resolveEnginePresentation('codex', 'Codex')).toEqual({
      accent: 'codex',
      displayName: 'Codex',
      showThoughtProcess: false,
    })
  })

  it('gives a third engine a safe default without renderer changes', () => {
    expect(resolveEnginePresentation('future-agent', null)).toEqual({
      accent: 'primary',
      displayName: 'future-agent',
      showThoughtProcess: true,
    })
  })

  it('prefers per-turn engine metadata over the session fallback', () => {
    const records: ConversationRecord[] = [{
      id: 'response',
      session,
      turnId: 'turn-1',
      parentId: null,
      role: 'assistant',
      timestamp: '2026-08-04T10:00:00Z',
      segments: [{ kind: 'text', text: 'done' }],
      usage: {
        inputTokens: 50,
        outputTokens: 12,
        totalTokens: 62,
        cachedInputTokens: 30,
        cacheCreationInputTokens: 4,
      },
      sourceMeta: { model: 'gpt-5.6-sol', effort: 'high' },
    }]

    expect(engineResponseMeta(records, 'fallback-model')).toEqual({
      model: 'gpt-5.6-sol',
      usage: {
        input_tokens: 50,
        output_tokens: 12,
        cache_creation_input_tokens: 4,
        cache_read_input_tokens: 30,
      },
      calls: 1,
    })
  })
})
