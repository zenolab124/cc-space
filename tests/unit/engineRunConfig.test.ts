import { afterEach, describe, expect, it } from 'vitest'
import {
  clearEngineRunConfig,
  engineRunConfig,
  inheritEngineRunConfig,
  setEngineRunConfig,
} from '../../src/engines/runConfig'

const sessionId = 'codex:test:thread-1'

afterEach(() => clearEngineRunConfig(sessionId))

describe('engine run config', () => {
  it('keeps model and effort scoped to a standard-engine session', () => {
    setEngineRunConfig(sessionId, { model: 'gpt-test', effort: 'high' })

    expect(engineRunConfig(sessionId)).toEqual({ model: 'gpt-test', effort: 'high' })
    expect(engineRunConfig('codex:test:thread-2')).toBeNull()
  })

  it('returns a defensive copy after storing config', () => {
    const config = { model: 'gpt-test', effort: 'low' }
    setEngineRunConfig(sessionId, config)
    config.effort = 'high'

    expect(engineRunConfig(sessionId)?.effort).toBe('low')
  })

  it('inherits lane settings without sharing mutable state', () => {
    const targetId = 'codex:test:thread-2'
    setEngineRunConfig(sessionId, { model: 'gpt-test', effort: 'medium' })
    inheritEngineRunConfig(sessionId, targetId)
    setEngineRunConfig(sessionId, { model: 'gpt-test', effort: 'high' })

    expect(engineRunConfig(targetId)?.effort).toBe('medium')
    clearEngineRunConfig(targetId)
  })
})
