import { describe, expect, it } from 'vitest'
import { asyncTaskStopId, buildAsyncLedger } from '../../src/composables/useAsyncTasks'
import type { SessionRecord } from '../../src/types'

function taskRecords(): SessionRecord[] {
  return [
    {
      type: 'assistant',
      uuid: 'assistant-1',
      parent_uuid: null,
      session_id: 'session-1',
      timestamp: '2026-08-02T10:00:00Z',
      cwd: null,
      version: null,
      git_branch: null,
      is_sidechain: null,
      message: {
        id: 'message-1',
        message_type: 'message',
        role: 'assistant',
        content: [{
          type: 'tool_use',
          id: 'tool-1',
          name: 'Agent',
          input: { description: 'Inspect lifecycle' },
        }],
        model: null,
        stop_reason: null,
        usage: null,
      },
    },
    {
      type: 'user',
      uuid: 'user-1',
      parent_uuid: null,
      session_id: 'session-1',
      timestamp: '2026-08-02T10:00:01Z',
      cwd: null,
      version: null,
      git_branch: null,
      is_sidechain: null,
      message: {
        role: 'user',
        content: [{ type: 'tool_result', tool_use_id: 'tool-1', content: 'launched', is_error: false }],
      },
      async_meta: {
        background_task_id: null,
        status: 'async_launched',
        is_async: true,
        agent_id: 'agent-1',
        agent_type: 'general-purpose',
        resolved_model: null,
        description: 'Inspect lifecycle',
        task_id: null,
        task_type: null,
        workflow_name: null,
        run_id: null,
        summary: null,
        output_file: null,
        scheduled_for: null,
        timeout_ms: null,
        persistent: null,
        resumed_agent_id: null,
      },
      origin_kind: null,
    },
  ]
}

function completion(timestamp: string): SessionRecord {
  return {
    type: 'task_notification',
    timestamp,
    session_id: 'session-1',
    content: '<task-notification><task-id>agent-1</task-id><status>completed</status><result>done</result></task-notification>',
  }
}

describe('async task ledger', () => {
  it('settles a running task from the normalized notification record', () => {
    const running = buildAsyncLedger(taskRecords(), [], true)
    expect(running).toHaveLength(1)
    expect(running[0].state).toBe('running')
    expect(asyncTaskStopId(running[0])).toBe('agent-1')

    const completed = buildAsyncLedger([
      ...taskRecords(),
      completion('2026-08-02T10:00:02Z'),
    ], [], true)
    expect(completed).toHaveLength(1)
    expect(completed[0].state).toBe('completed')
    expect(completed[0].resultText).toBe('done')
    expect(asyncTaskStopId(completed[0])).toBeNull()
  })

  it('does not duplicate a task when both notification carriers are present', () => {
    const ledger = buildAsyncLedger([
      ...taskRecords(),
      completion('2026-08-02T10:00:02Z'),
      completion('2026-08-02T10:00:03Z'),
    ], [], true)
    expect(ledger).toHaveLength(1)
    expect(ledger[0].state).toBe('completed')
  })
})
