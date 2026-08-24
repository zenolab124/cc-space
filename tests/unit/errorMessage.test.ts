import { describe, expect, it } from 'vitest'
import { errorMessage } from '../../src/utils/errorMessage'

describe('errorMessage', () => {
  it('extracts the message from a structured Tauri error', () => {
    expect(errorMessage({ kind: 'notFound', message: 'Asset was not found', retryable: false }, 'Unknown error'))
      .toBe('Asset was not found')
  })

  it('never renders an opaque object coercion', () => {
    expect(errorMessage({ kind: 'notFound' }, 'Unknown error')).toBe('{"kind":"notFound"}')
  })

  it('falls back for an unhelpful or unserializable value', () => {
    const circular: { self?: unknown } = {}
    circular.self = circular
    expect(errorMessage(circular, 'Unknown error')).toBe('Unknown error')
  })
})
