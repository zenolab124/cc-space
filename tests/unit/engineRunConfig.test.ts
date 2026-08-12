import { afterEach, describe, expect, it } from 'vitest'
import {
  clearEngineRunConfig,
  engineRunConfig,
  engineRuntimeChannel,
  engineRuntimeOptions,
  inheritEngineRunConfig,
  isFastServiceTierUnavailableError,
  resolveFastServiceTier,
  resolveInitialEngineChannel,
  setEngineRunConfig,
} from '../../src/engines/runConfig'

const sessionId = 'codex:test:thread-1'

afterEach(() => clearEngineRunConfig(sessionId))

describe('engine run config', () => {
  it('preserves an explicit official channel instead of falling back to the global default', () => {
    const stored = {
      model: null, effort: null, serviceTier: null, channelId: null, modelOverridden: false, effortOverridden: false,
    }

    expect(resolveInitialEngineChannel(stored, 'plus')).toBeNull()
    expect(resolveInitialEngineChannel(null, 'plus')).toBe('plus')
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
