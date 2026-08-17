import { describe, expect, it, vi } from 'vitest'
import { rebindDraftChannel, type RuntimeEngineDraft } from '../../src/engines/draftChannel'
import type { EngineRunConfig } from '../../src/engines/runConfig'

const engine = { engineId: 'codex', instanceId: 'default' }
const project = { engine, nativeId: 'project' }
const sourceReference = { engine, nativeId: 'thread-old' }
const replacementReference = { engine, nativeId: 'thread-new' }
const draft: RuntimeEngineDraft = {
  reference: sourceReference,
  project,
  engineName: 'Codex',
  cwd: '/workspace/project',
  attachedChannel: 'channel-a',
  attachedCapabilityFingerprint: '["html_visual"]',
}
const config: EngineRunConfig = {
  model: 'gpt-test',
  effort: 'high',
  serviceTier: null,
  channelId: 'channel-b',
  modelOverridden: true,
  effortOverridden: true,
}

function dependencies(replaceSession = vi.fn(() => true)) {
  return {
    createSession: vi.fn(async () => ({
      session: replacementReference,
      runtimeId: 'runtime-new',
      sourceMeta: { modelProvider: 'provider-b' },
      capabilityFingerprint: '[]',
    })),
    sessionId: vi.fn(() => 'session-new'),
    stageDraft: vi.fn(),
    saveConfig: vi.fn(),
    beforeReplace: vi.fn(),
    replaceSession,
    discardDraft: vi.fn(),
    replacementError: () => new Error('replace failed'),
  }
}

describe('standard-engine draft channel rebinding', () => {
  it('does nothing when the empty thread already uses the selected channel', async () => {
    const deps = dependencies()
    const result = await rebindDraftChannel({
      sessionId: 'session-old',
      draft,
      selectedChannel: 'channel-a',
      selectedCapabilityFingerprint: '["html_visual"]',
      options: { channelId: 'channel-a' },
      config,
    }, deps)

    expect(result).toBeNull()
    expect(deps.createSession).not.toHaveBeenCalled()
  })

  it('treats the built-in official id and zero-injection channel as equivalent', async () => {
    const deps = dependencies()
    const result = await rebindDraftChannel({
      sessionId: 'session-old',
      draft: { ...draft, attachedChannel: 'official' },
      selectedChannel: null,
      selectedCapabilityFingerprint: '["html_visual"]',
      options: {},
      config: { ...config, channelId: null },
    }, deps)

    expect(result).toBeNull()
    expect(deps.createSession).not.toHaveBeenCalled()
  })

  it('creates a replacement thread and atomically transfers draft state', async () => {
    const deps = dependencies()
    const result = await rebindDraftChannel({
      sessionId: 'session-old',
      draft,
      selectedChannel: 'channel-b',
      selectedCapabilityFingerprint: '["html_visual"]',
      options: { channelId: 'channel-b', model: 'gpt-test' },
      config,
    }, deps)

    expect(deps.createSession).toHaveBeenCalledWith(project, draft.cwd, {
      channelId: 'channel-b',
      model: 'gpt-test',
    })
    expect(deps.stageDraft).toHaveBeenCalledWith('session-new', expect.objectContaining({
      reference: replacementReference,
      attachedChannel: 'channel-b',
    }))
    expect(deps.saveConfig).toHaveBeenCalledWith('session-new', config)
    expect(deps.beforeReplace).toHaveBeenCalledWith(expect.objectContaining({ sessionId: 'session-new' }))
    expect(deps.replaceSession).toHaveBeenCalledWith('session-old', 'session-new')
    expect(deps.beforeReplace.mock.invocationCallOrder[0])
      .toBeLessThan(deps.replaceSession.mock.invocationCallOrder[0])
    expect(deps.discardDraft).not.toHaveBeenCalled()
    expect(result).toEqual({
      sessionId: 'session-new',
      reference: replacementReference,
      runtimeId: 'runtime-new',
      sourceMeta: { modelProvider: 'provider-b' },
      attachedChannel: 'channel-b',
      attachedCapabilityFingerprint: '[]',
    })
  })

  it('replaces an empty thread instead of resuming before rollout when capabilities change', async () => {
    const deps = dependencies()
    const result = await rebindDraftChannel({
      sessionId: 'session-old',
      draft,
      selectedChannel: 'channel-a',
      selectedCapabilityFingerprint: '[]',
      options: { channelId: 'channel-a' },
      config,
    }, deps)

    expect(deps.createSession).toHaveBeenCalledOnce()
    expect(result?.attachedCapabilityFingerprint).toBe('[]')
  })

  it('discards the replacement thread when workbench takeover fails', async () => {
    const deps = dependencies(vi.fn(() => false))
    await expect(rebindDraftChannel({
      sessionId: 'session-old',
      draft,
      selectedChannel: 'channel-b',
      selectedCapabilityFingerprint: '["html_visual"]',
      options: { channelId: 'channel-b' },
      config,
    }, deps)).rejects.toThrow('replace failed')

    expect(deps.discardDraft).toHaveBeenCalledWith('session-new')
  })
})
