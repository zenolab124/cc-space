import { describe, expect, it } from 'vitest'
import { createRuntimeDeltaShaper } from '../../src/engines/runtimeDeltaShaper'
import type { RuntimeEventEnvelope, SessionRef } from '../../src/engines/types'

const session: SessionRef = {
  engine: { engineId: 'codex', profileId: 'default', installationId: 'local' },
  nativeId: 'session-1',
}

function envelope(sequence: number, event: RuntimeEventEnvelope['event']): RuntimeEventEnvelope {
  return {
    session,
    runtimeId: 'runtime-1',
    generation: 1,
    sequence,
    timestamp: '2026-08-10T00:00:00Z',
    event,
  }
}

function createClock() {
  let now = 0
  let nextId = 1
  const timers = new Map<number, { at: number; callback: () => void }>()

  function schedule(callback: () => void, delay: number): number {
    const id = nextId++
    timers.set(id, { at: now + delay, callback })
    return id
  }

  function advance(ms: number) {
    const target = now + ms
    while (true) {
      const due = [...timers.entries()]
        .filter(([, timer]) => timer.at <= target)
        .sort((left, right) => left[1].at - right[1].at)[0]
      if (!due) break
      timers.delete(due[0])
      now = due[1].at
      due[1].callback()
    }
    now = target
  }

  return {
    now: () => now,
    schedule,
    cancel: (id: number) => timers.delete(id),
    advance,
  }
}

describe('runtime delta shaper', () => {
  it('warms up and drains text smoothly before completion events', () => {
    const clock = createClock()
    const delivered: RuntimeEventEnvelope[] = []
    const shaper = createRuntimeDeltaShaper({
      deliver: event => delivered.push(event),
      now: clock.now,
      schedule: clock.schedule,
      cancel: clock.cancel,
      documentVisible: () => true,
    })

    shaper.push(envelope(1, {
      kind: 'itemDelta',
      turnId: 'turn-1',
      itemId: 'item-1',
      segment: { kind: 'text', text: 'abcdefghij', phase: 'final' },
    }))
    shaper.push(envelope(2, {
      kind: 'itemCompleted',
      turnId: 'turn-1',
      itemId: 'item-1',
      status: 'completed',
    }))
    shaper.push(envelope(3, {
      kind: 'turnCompleted',
      turnId: 'turn-1',
      status: 'completed',
      error: null,
    }))

    expect(shaper.snapshot().pending).toBe(true)
    clock.advance(199)
    expect(delivered).toHaveLength(0)
    clock.advance(9)
    expect(delivered[0]?.event.kind).toBe('itemDelta')

    for (let index = 0; index < 100 && shaper.snapshot().pending; index++) clock.advance(16)

    const text = delivered
      .filter((entry): entry is RuntimeEventEnvelope & {
        event: Extract<RuntimeEventEnvelope['event'], { kind: 'itemDelta' }>
      } => entry.event.kind === 'itemDelta')
      .map(entry => entry.event.segment.kind === 'text' ? entry.event.segment.text : '')
      .join('')
    expect(text).toBe('abcdefghij')
    expect(delivered.slice(-2).map(entry => entry.event.kind)).toEqual([
      'itemCompleted',
      'turnCompleted',
    ])
    expect(shaper.snapshot()).toEqual({ pending: false, turnIds: new Set() })
  })

  it('flushes hidden documents in one low-frequency update', () => {
    const clock = createClock()
    const delivered: RuntimeEventEnvelope[] = []
    const shaper = createRuntimeDeltaShaper({
      deliver: event => delivered.push(event),
      now: clock.now,
      schedule: clock.schedule,
      cancel: clock.cancel,
      documentVisible: () => false,
    })

    shaper.push(envelope(1, {
      kind: 'itemDelta',
      turnId: 'turn-1',
      itemId: 'item-1',
      segment: { kind: 'text', text: 'complete buffer', phase: 'final' },
    }))
    clock.advance(249)
    expect(delivered).toHaveLength(0)
    clock.advance(1)

    expect(delivered).toHaveLength(1)
    expect(delivered[0]?.event).toMatchObject({
      kind: 'itemDelta',
      segment: { kind: 'text', text: 'complete buffer' },
    })
    expect(shaper.snapshot().pending).toBe(false)
  })

  it('preserves command metadata while merging output deltas', () => {
    const clock = createClock()
    const delivered: RuntimeEventEnvelope[] = []
    const shaper = createRuntimeDeltaShaper({
      deliver: event => delivered.push(event),
      now: clock.now,
      schedule: clock.schedule,
      cancel: clock.cancel,
      documentVisible: () => false,
    })

    shaper.push(envelope(1, {
      kind: 'itemDelta',
      turnId: 'turn-1',
      itemId: 'command-1',
      segment: {
        kind: 'commandExecution',
        id: 'command-1',
        command: 'pnpm test',
        cwd: '/workspace',
        output: 'first',
        status: 'running',
      },
    }))
    shaper.push(envelope(2, {
      kind: 'itemDelta',
      turnId: 'turn-1',
      itemId: 'command-1',
      segment: {
        kind: 'commandExecution',
        id: 'command-1',
        command: '',
        cwd: null,
        output: ' second',
        status: 'running',
      },
    }))
    clock.advance(250)

    const event = delivered[0]?.event
    expect(event?.kind).toBe('itemDelta')
    if (event?.kind !== 'itemDelta' || event.segment.kind !== 'commandExecution') return
    expect(event.segment).toMatchObject({
      command: 'pnpm test',
      cwd: '/workspace',
      output: 'first second',
    })
  })
})
