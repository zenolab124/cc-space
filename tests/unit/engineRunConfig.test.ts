import { afterEach, describe, expect, it } from 'vitest'
import {
  clearEngineRunConfig,
  engineRunConfig,
  engineRuntimeChannel,
  engineRuntimeOptions,
  evictEngineRunConfig,
  inheritEngineRunConfig,
  isFastServiceTierUnavailableError,
  resolveFastServiceTier,
  resolveInitialEngineChannel,
  setEngineRunConfig,
} from '../../src/engines/runConfig'

const sessionId = 'codex:test:thread-1'
const storage = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', {
  value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
    removeItem: (key: string) => storage.delete(key),
    clear: () => storage.clear(),
  },
  configurable: true,
})

afterEach(() => clearEngineRunConfig(sessionId))

describe('engine run config', () => {
  it('migrates a legacy inherited channel from the observed provider', () => {
    const stored = {
      model: null, effort: null, serviceTier: null, channelId: null, modelOverridden: false, effortOverridden: false,
    }

    expect(resolveInitialEngineChannel(stored, 'plus', 'observed')).toBe('observed')
    expect(resolveInitialEngineChannel(null, 'plus', 'observed')).toBe('plus')
    expect(resolveInitialEngineChannel(null, null, 'observed')).toBe('observed')
  })

  it('keeps model and effort scoped to a standard-engine session', () => {
    setEngineRunConfig(sessionId, {
      model: 'gpt-test', effort: 'high', serviceTier: 'priority', channelId: null, modelOverridden: true, effortOverridden: true,
    })

    expect(engineRunConfig(sessionId)).toEqual({
      model: 'gpt-test', effort: 'high', serviceTier: 'priority', channelId: null, modelOverridden: true, effortOverridden: true,
    })
    expect(engineRuntimeOptions(sessionId)).toEqual({ model: 'gpt-test' })
    expect(engineRuntimeChannel(sessionId)).toBeNull()
    expect(engineRunConfig('codex:test:thread-2')).toBeNull()
  })

  it('returns a defensive copy after storing config', () => {
    const config = {
      model: 'gpt-test', effort: 'low', serviceTier: null, channelId: 'proxy', modelOverridden: true, effortOverridden: true,
    }
    setEngineRunConfig(sessionId, config)
    config.effort = 'high'

    expect(engineRunConfig(sessionId)?.effort).toBe('low')
    expect(engineRuntimeOptions(sessionId)).toEqual({ model: 'gpt-test', channelId: 'proxy' })
    expect(engineRuntimeChannel(sessionId)).toBe('proxy')
  })

  it('restores the selected channel after the in-memory cache is evicted', () => {
    setEngineRunConfig(sessionId, {
      model: 'gpt-test', effort: 'high', serviceTier: null, channelId: 'proxy', modelOverridden: true, effortOverridden: true,
    })

    evictEngineRunConfig(sessionId)

    expect(engineRunConfig(sessionId)?.channelId).toBe('proxy')
    expect(engineRuntimeOptions(sessionId)).toEqual({ model: 'gpt-test', channelId: 'proxy' })
  })

  it('inherits lane settings without sharing mutable state', () => {
    const targetId = 'codex:test:thread-2'
    setEngineRunConfig(sessionId, {
      model: 'gpt-test', effort: 'medium', serviceTier: 'priority', channelId: 'proxy', modelOverridden: true, effortOverridden: true,
    })
    inheritEngineRunConfig(sessionId, targetId)
    setEngineRunConfig(sessionId, {
      model: 'gpt-test', effort: 'high', serviceTier: null, channelId: null, modelOverridden: true, effortOverridden: true,
    })

    expect(engineRunConfig(targetId)?.effort).toBe('medium')
    expect(engineRunConfig(targetId)?.serviceTier).toBe('priority')
    clearEngineRunConfig(targetId)
  })

  it('resolves Codex Fast from the model service-tier catalog', () => {
    const model = {
      serviceTiers: [
        { id: 'flex', name: 'Flex' },
        { id: 'priority', name: 'Fast', description: '1.5x speed' },
      ],
    }

    expect(resolveFastServiceTier(model)?.id).toBe('priority')
    expect(resolveFastServiceTier({ serviceTiers: [] })).toBeNull()
  })

  it('only retries explicit Fast-tier availability failures', () => {
    expect(isFastServiceTierUnavailableError('service tier priority is not enabled for this account')).toBe(true)
    expect(isFastServiceTierUnavailableError(new Error('Fast mode requires credits'))).toBe(true)
    expect(isFastServiceTierUnavailableError('network unavailable')).toBe(false)
  })
})
