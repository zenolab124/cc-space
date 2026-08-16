import { describe, expect, it } from 'vitest'
import {
  adapterBaseUrlCandidates,
  adapterEndpointUrl,
  preferredAdapterBaseUrl,
} from '../../src/utils/channelUrls'

describe('channel adapter URL resolution', () => {
  it('keeps the Anthropic root and removes a mistakenly pasted v1 suffix', () => {
    expect(preferredAdapterBaseUrl('https://proxy.example.com', 'claude-code'))
      .toBe('https://proxy.example.com')
    expect(preferredAdapterBaseUrl('https://proxy.example.com/v1', 'claude-code'))
      .toBe('https://proxy.example.com')
    expect(adapterEndpointUrl('https://proxy.example.com', 'claude-code'))
      .toBe('https://proxy.example.com/v1/messages')
  })

  it('prefers v1 for Responses while retaining the unversioned fallback', () => {
    expect(adapterBaseUrlCandidates('https://proxy.example.com', 'codex'))
      .toEqual(['https://proxy.example.com/v1', 'https://proxy.example.com'])
    expect(adapterEndpointUrl('https://proxy.example.com/v1', 'codex'))
      .toBe('https://proxy.example.com/v1/responses')
  })

  it('does not duplicate v1 or a pasted endpoint', () => {
    expect(adapterBaseUrlCandidates('https://proxy.example.com/api/v1/', 'codex'))
      .toEqual(['https://proxy.example.com/api/v1', 'https://proxy.example.com/api'])
    expect(preferredAdapterBaseUrl('https://proxy.example.com/v1/responses', 'codex'))
      .toBe('https://proxy.example.com/v1')
    expect(preferredAdapterBaseUrl('https://proxy.example.com/v1/messages', 'claude-code'))
      .toBe('https://proxy.example.com')
  })
})
