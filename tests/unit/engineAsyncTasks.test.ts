import { describe, expect, it } from 'vitest'
import { buildEngineAsyncTasks } from '../../src/engines/asyncTasks'
import type { ConversationRecord, SessionRef } from '../../src/engines/types'

const session: SessionRef = {
  engine: { engineId: 'codex', instanceId: 'default' },
  nativeId: 'parent',
}

function collabRecord(id: string, name: string, input: Record<string, unknown>): ConversationRecord {
  return {
    id,
    session,
    turnId: 'turn',
    parentId: null,
    role: 'tool',
    timestamp: '2026-08-04T00:00:00Z',
    segments: [{ kind: 'toolCall', id, name, input }],
    usage: null,
    sourceMeta: {},
  }
}

describe('buildEngineAsyncTasks', () => {
  it('groups Codex collaboration calls by child thread and follows agent status', () => {
    const records = [
      collabRecord('spawn', 'spawnAgent', {
        receiverThreadIds: ['child-1'],
        prompt: '审查多引擎边界\n补充测试',
        model: 'gpt-5.6-sol',
        reasoningEffort: 'high',
        status: 'completed',
        agentsStates: { 'child-1': { status: 'running', message: null } },
      }),
      collabRecord('wait', 'wait', {
        receiverThreadIds: ['child-1'],
        status: 'completed',
        agentsStates: { 'child-1': { status: 'completed', message: 'done' } },
      }),
    ]

    expect(buildEngineAsyncTasks(records)).toEqual([expect.objectContaining({
      threadId: 'child-1',
      title: '审查多引擎边界',
      model: 'gpt-5.6-sol',
      effort: 'high',
      state: 'completed',
      message: 'done',
    })])
  })
})
