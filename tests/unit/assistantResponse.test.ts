import { describe, expect, it } from 'vitest'
import { summarizeAssistantResponse } from '@/utils/assistantResponse'

describe('assistant response summary', () => {
  it('aggregates every reported API call in one user turn', () => {
    const summary = summarizeAssistantResponse([
      {
        model: 'claude-opus-4-1',
        usage: {
          input_tokens: 10,
          cache_read_input_tokens: 20,
          cache_creation_input_tokens: 30,
          output_tokens: 40,
        },
      },
      {
        model: 'claude-opus-4-1',
        usage: {
          input_tokens: 1,
          cache_read_input_tokens: 2,
          cache_creation_input_tokens: 3,
          output_tokens: 4,
        },
      },
    ])

    expect(summary).toEqual({
      model: 'claude-opus-4-1',
      usage: {
        input_tokens: 11,
        cache_read_input_tokens: 22,
        cache_creation_input_tokens: 33,
        output_tokens: 44,
      },
      calls: 2,
    })
  })

  it('uses the latest real model and ignores synthetic zero-usage records', () => {
    const summary = summarizeAssistantResponse([
      { model: 'claude-sonnet-4', usage: { output_tokens: 5 } },
      { model: '<synthetic>', usage: { output_tokens: 0 } },
      { model: 'claude-opus-4-1' },
    ])

    expect(summary.model).toBe('claude-opus-4-1')
    expect(summary.calls).toBe(1)
    expect(summary.usage?.output_tokens).toBe(5)
  })

  it('omits usage when no call reports tokens', () => {
    expect(summarizeAssistantResponse([{ model: '<synthetic>', usage: null }])).toEqual({
      model: null,
      usage: null,
      calls: 0,
    })
  })
})
