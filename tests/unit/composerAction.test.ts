import { describe, expect, it } from 'vitest'
import { resolveComposerAction, shouldSubmitComposer } from '../../src/components/session/composerAction'

describe('shared session composer action', () => {
  it('uses the same send/stop switch for native and standard sessions', () => {
    expect(resolveComposerAction({ busy: false, hasContent: false, canSendWhileBusy: false })).toBe('send')
    expect(resolveComposerAction({ busy: true, hasContent: false, canSendWhileBusy: true })).toBe('stop')
    expect(resolveComposerAction({ busy: true, hasContent: true, canSendWhileBusy: true })).toBe('send')
  })

  it('does not expose a busy send action when the adapter cannot accept it', () => {
    expect(resolveComposerAction({ busy: true, hasContent: true, canSendWhileBusy: false })).toBe('stop')
  })

  it('submits plain Enter but leaves Shift+Enter and IME confirmation alone', () => {
    const key = (overrides: Partial<KeyboardEvent> = {}) => ({
      key: 'Enter',
      shiftKey: false,
      isComposing: false,
      keyCode: 13,
      ...overrides,
    }) as KeyboardEvent
    expect(shouldSubmitComposer(key())).toBe(true)
    expect(shouldSubmitComposer(key({ shiftKey: true }))).toBe(false)
    expect(shouldSubmitComposer(key({ isComposing: true }))).toBe(false)
    expect(shouldSubmitComposer(key({ keyCode: 229 }))).toBe(false)
  })
})
