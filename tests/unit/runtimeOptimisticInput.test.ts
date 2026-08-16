import { computed } from 'vue'
import { afterEach, describe, expect, it } from 'vitest'
import {
  bindLatestRuntimeOptimisticInput,
  bindRuntimeOptimisticInput,
  clearRuntimeOptimisticInputs,
  reconcileRuntimeOptimisticInputs,
  removeRuntimeOptimisticInput,
  stageRuntimeOptimisticInput,
  useRuntimeOptimisticInputs,
} from '../../src/engines/runtimeOptimisticInput'
import { sessionUiId } from '../../src/engines/integration'
import type { ConversationRecord, SessionRef } from '../../src/engines/types'

const session: SessionRef = {
  engine: { engineId: 'fixture', instanceId: 'default' },
  nativeId: 'race-session',
}

afterEach(() => clearRuntimeOptimisticInputs(sessionUiId(session)))

describe('runtime optimistic input bridge', () => {
  it('stages and binds a race prompt before runtime output arrives', () => {
    const visible = useRuntimeOptimisticInputs(computed(() => session))
    const id = stageRuntimeOptimisticInput(session, 'compare this')

    expect(visible.value).toEqual([expect.objectContaining({
      id,
      role: 'user',
      turnId: null,
      segments: [{ kind: 'text', text: 'compare this' }],
    })])

    bindRuntimeOptimisticInput(session, id, 'turn-1')
    expect(visible.value[0].turnId).toBe('turn-1')
  })

  it('binds an early turnStarted event and removes failed input', () => {
    const visible = useRuntimeOptimisticInputs(computed(() => session))
    const id = stageRuntimeOptimisticInput(session, 'question')

    bindLatestRuntimeOptimisticInput(session, 'turn-early')
    expect(visible.value[0].turnId).toBe('turn-early')

    removeRuntimeOptimisticInput(session, id)
    expect(visible.value).toEqual([])
  })

  it('hands a bound optimistic prompt to persisted history', () => {
    const visible = useRuntimeOptimisticInputs(computed(() => session))
    const id = stageRuntimeOptimisticInput(session, 'landed')
    bindRuntimeOptimisticInput(session, id, 'turn-landed')
    const persisted: ConversationRecord[] = [{
      id: 'history-user',
      session,
      turnId: 'turn-landed',
      parentId: null,
      role: 'user',
      timestamp: '2026-08-16T00:00:00Z',
      segments: [{ kind: 'text', text: 'landed' }],
      usage: null,
      sourceMeta: {},
    }]

    reconcileRuntimeOptimisticInputs(session, persisted)
    expect(visible.value).toEqual([])
  })
})
