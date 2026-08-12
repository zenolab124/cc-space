import { describe, expect, it } from 'vitest'
import {
  bindOptimisticUserTurn,
  composeRuntimeTimeline,
  hasLiveTurn,
  optimisticUserSourceMeta,
  reconcileLiveRecords,
  reduceRuntimeTimeline,
} from '../../src/engines/runtimeTimeline'
import type {
  ConversationRecord,
  NormalizedRuntimeEvent,
  RuntimeEventEnvelope,
  SessionRef,
} from '../../src/engines/types'

const session: SessionRef = {
  engine: { engineId: 'fixture', instanceId: 'default' },
  nativeId: 'session-1',
}

function envelope(event: NormalizedRuntimeEvent): RuntimeEventEnvelope {
  return {
    session,
    runtimeId: 'runtime-1',
    generation: 1,
    sequence: 1,
    timestamp: '2026-08-10T00:00:00Z',
    event,
  }
}

function record(overrides: Partial<ConversationRecord> = {}): ConversationRecord {
  return {
    id: 'record-1',
    session,
    turnId: 'turn-1',
    parentId: null,
    role: 'assistant',
    timestamp: '2026-08-10T00:00:00Z',
    segments: [],
    usage: null,
    sourceMeta: {},
    ...overrides,
  }
}

describe('standard runtime timeline reducer', () => {
  it('binds the optimistic user message to the normalized turn before deltas arrive', () => {
    const pending = record({
      id: 'pending-user-1',
      turnId: null,
      role: 'user',
      segments: [{ kind: 'text', text: 'hello' }],
      sourceMeta: optimisticUserSourceMeta(),
    })

    const started = reduceRuntimeTimeline([pending], envelope({ kind: 'turnStarted', turnId: 'turn-1' }))
    expect(started.changed).toBe(true)
    expect(started.records[0].turnId).toBe('turn-1')

    const delta = reduceRuntimeTimeline(started.records, envelope({
      kind: 'itemDelta',
      turnId: 'turn-1',
      itemId: 'assistant-1',
      segment: { kind: 'text', text: 'world', phase: 'final' },
    }))
    expect(delta.records.map(item => item.turnId)).toEqual(['turn-1', 'turn-1'])
  })

  it('keeps progress and final text separate and settles streamed command status', () => {
    let state = reduceRuntimeTimeline([], envelope({
      kind: 'itemDelta',
      turnId: 'turn-1',
      itemId: 'assistant-1',
      segment: { kind: 'text', text: 'checking', phase: 'progress' },
    })).records
    state = reduceRuntimeTimeline(state, envelope({
      kind: 'itemDelta',
      turnId: 'turn-1',
      itemId: 'assistant-1',
      segment: { kind: 'text', text: 'done', phase: 'final' },
    })).records
    expect(state[0].segments).toHaveLength(2)

    state = reduceRuntimeTimeline(state, envelope({
      kind: 'itemDelta',
      turnId: 'turn-1',
      itemId: 'command-1',
      segment: {
        kind: 'commandExecution',
        id: 'command-1',
        command: 'pnpm test',
        cwd: null,
        output: 'pass',
        status: 'running',
      },
    })).records
    const completed = reduceRuntimeTimeline(state, envelope({
      kind: 'itemCompleted',
      turnId: 'turn-1',
      itemId: 'command-1',
      status: 'completed',
    }))
    expect(completed.changed).toBe(true)
    expect(completed.records[1].segments[0]).toMatchObject({ status: 'completed' })
  })

  it('surfaces runtime failures and action capability changes', () => {
    const failed = reduceRuntimeTimeline([], envelope({
      kind: 'runtimeError',
      message: 'runtime failed',
      retryable: true,
    }))
    expect(failed.error).toBe('runtime failed')

    const changed = reduceRuntimeTimeline([], envelope({ kind: 'capabilitiesChanged' }))
    expect(changed.refreshActions).toBe(true)
  })

  it('scopes repeated item ids to their own turn', () => {
    const previous = record({
      id: 'shared-item',
      turnId: 'turn-1',
      segments: [{
        kind: 'commandExecution',
        id: 'shared-item',
        command: 'first',
        cwd: null,
        output: null,
        status: 'running',
      }],
    })
    const current = reduceRuntimeTimeline([previous], envelope({
      kind: 'itemDelta',
      turnId: 'turn-2',
      itemId: 'shared-item',
      segment: { kind: 'text', text: 'second', phase: 'final' },
    })).records

    expect(current).toHaveLength(2)
    expect(current.map(item => item.turnId)).toEqual(['turn-1', 'turn-2'])

    const completed = reduceRuntimeTimeline(current, envelope({
      kind: 'itemCompleted',
      turnId: 'turn-2',
      itemId: 'shared-item',
      status: 'completed',
    }))
    expect(completed.changed).toBe(false)
    expect(completed.records[0].segments[0]).toMatchObject({ status: 'running' })
  })
})

describe('live history reconciliation', () => {
  it('binds the exact optimistic prompt to the authoritative started turn', () => {
    const previous = record({
      id: 'pending-user-previous',
      role: 'user',
      turnId: 'turn-1',
      sourceMeta: optimisticUserSourceMeta(),
    })
    const current = record({
      id: 'pending-user-current',
      role: 'user',
      turnId: null,
      sourceMeta: optimisticUserSourceMeta(),
    })

    const bound = bindOptimisticUserTurn(
      [previous, current],
      'pending-user-current',
      'turn-2',
    )
    expect(bound.map(item => item.turnId)).toEqual(['turn-1', 'turn-2'])
  })

  it('inserts a live prompt before a response that landed first', () => {
    const persisted = [
      record({ id: 'user-1', role: 'user', turnId: 'turn-1' }),
      record({ id: 'assistant-1', turnId: 'turn-1' }),
      record({ id: 'assistant-2', turnId: 'turn-2' }),
    ]
    const pendingUser = record({
      id: 'pending-user-2',
      role: 'user',
      turnId: 'turn-2',
      sourceMeta: optimisticUserSourceMeta(),
    })

    expect(composeRuntimeTimeline(persisted, [pendingUser]).map(item => item.id)).toEqual([
      'user-1',
      'assistant-1',
      'pending-user-2',
      'assistant-2',
    ])
  })

  it('does not render a late live copy after the normalized history record has landed', () => {
    const persisted = [
      record({ id: 'item-1', role: 'user', turnId: 'turn-2' }),
      record({
        id: 'exec-1',
        role: 'tool',
        turnId: 'turn-2',
        segments: [{
          kind: 'commandExecution',
          id: 'exec-1',
          command: 'check',
          cwd: null,
          output: null,
          status: 'completed',
        }],
      }),
      record({
        id: 'item-3',
        turnId: 'turn-2',
        segments: [{ kind: 'text', text: 'final answer', phase: 'final' }],
      }),
    ]
    const lateLiveCopy = record({
      id: 'msg-runtime-id',
      turnId: 'turn-2',
      segments: [{ kind: 'text', text: 'final answer', phase: 'final' }],
    })

    expect(composeRuntimeTimeline(persisted, [lateLiveCopy]).map(item => item.id)).toEqual([
      'item-1',
      'exec-1',
      'item-3',
    ])
  })

  it('hands a partially replayed final response to the complete persisted response', () => {
    const persisted = [record({
      id: 'item-3',
      turnId: 'turn-2',
      segments: [{
        kind: 'text',
        text: 'final answer with the complete remaining text',
        phase: 'final',
      }],
    })]
    const replayingPrefix = record({
      id: 'msg-runtime-id',
      turnId: 'turn-2',
      segments: [{ kind: 'text', text: 'final answer with the', phase: 'final' }],
    })

    expect(composeRuntimeTimeline(persisted, [replayingPrefix]).map(item => item.id)).toEqual([
      'item-3',
    ])
  })

  it('keeps a new live turn in prompt-then-response order', () => {
    const pendingUser = record({
      id: 'pending-user-2',
      role: 'user',
      turnId: 'turn-2',
      sourceMeta: optimisticUserSourceMeta(),
    })
    const liveAssistant = record({ id: 'assistant-2', turnId: 'turn-2' })

    expect(composeRuntimeTimeline([], [liveAssistant, pendingUser]).map(item => item.id)).toEqual([
      'pending-user-2',
      'assistant-2',
    ])
  })

  it('hands landed text records to history even when the source normalizes item ids', () => {
    const pendingUser = record({
      id: 'pending-user-1',
      role: 'user',
      segments: [{ kind: 'text', text: 'hello' }],
      sourceMeta: optimisticUserSourceMeta(),
    })
    const liveAssistant = record({
      id: 'runtime-agent-message',
      segments: [{ kind: 'text', text: 'world', phase: 'final' }],
    })
    const persisted = [
      record({ id: 'item-1', role: 'user', segments: [{ kind: 'text', text: 'hello' }] }),
      record({ id: 'item-2', segments: [{ kind: 'text', text: 'world' }] }),
    ]

    expect(reconcileLiveRecords(persisted, [pendingUser, liveAssistant])).toEqual([])
  })

  it('keeps unmatched and duplicate live text until each copy has landed', () => {
    const persisted = [record({ id: 'item-1', segments: [{ kind: 'text', text: 'same' }] })]
    const first = record({ id: 'runtime-1', segments: [{ kind: 'text', text: 'same', phase: 'final' }] })
    const second = record({ id: 'runtime-2', segments: [{ kind: 'text', text: 'same', phase: 'final' }] })
    const unmatched = record({ id: 'runtime-3', segments: [{ kind: 'text', text: 'new', phase: 'final' }] })

    expect(reconcileLiveRecords(persisted, [first, second, unmatched]))
      .toEqual([second, unmatched])
  })

  it('hands an optimistic image-only message to its landed attachment record', () => {
    const pendingImage = record({
      id: 'pending-image',
      role: 'user',
      segments: [],
      sourceMeta: {
        ...optimisticUserSourceMeta(),
        optimisticImages: [{ id: 'image-1' }],
      },
    })
    const persisted = [record({
      id: 'item-1',
      role: 'user',
      segments: [{
        kind: 'attachment',
        asset: { session, nativeId: 'image-1' },
        mediaType: 'image/png',
        title: null,
      }],
    })]

    expect(reconcileLiveRecords(persisted, [pendingImage])).toEqual([])
  })

  it('keeps unflushed records and removes them only after history confirms landing', () => {
    const pendingUser = record({
      id: 'pending-user-1',
      role: 'user',
      segments: [{ kind: 'text', text: 'hello' }],
      sourceMeta: optimisticUserSourceMeta(),
    })
    const liveAssistant = record({
      id: 'assistant-1',
      segments: [{ kind: 'text', text: 'world', phase: 'final' }],
    })
    expect(reconcileLiveRecords([], [pendingUser, liveAssistant])).toHaveLength(2)

    const persisted = [
      record({ id: 'user-1', role: 'user', segments: [{ kind: 'text', text: 'hello' }] }),
      record({ id: 'assistant-1', segments: [{ kind: 'text', text: 'world', phase: 'final' }] }),
    ]
    const reconciled = reconcileLiveRecords(persisted, [pendingUser, liveAssistant])
    expect(reconciled).toEqual([])
    expect(hasLiveTurn(reconciled, 'turn-1')).toBe(false)
  })

  it('does not reconcile an item against the same id from another turn', () => {
    const live = record({
      id: 'shared-item',
      turnId: 'turn-2',
      segments: [{ kind: 'text', text: 'new', phase: 'final' }],
    })
    const persisted = [record({
      id: 'shared-item',
      turnId: 'turn-1',
      segments: [{ kind: 'text', text: 'old', phase: 'final' }],
    })]

    expect(reconcileLiveRecords(persisted, [live])).toEqual([live])
  })
})
