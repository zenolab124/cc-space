import { describe, expect, it } from 'vitest'
import { groupConversationRecords } from '../../src/engines/conversationGroups'
import type { ConversationRecord } from '../../src/engines/types'

function record(id: string, role: ConversationRecord['role'], turnId: string | null): ConversationRecord {
  return {
    id,
    session: { engine: { engineId: 'fixture', instanceId: 'default' }, nativeId: 'session' },
    turnId,
    parentId: null,
    role,
    timestamp: null,
    segments: [],
    usage: null,
    sourceMeta: {},
  }
}

describe('groupConversationRecords', () => {
  it('keeps records from the same turn together', () => {
    const groups = groupConversationRecords([
      record('user-1', 'user', 'turn-1'),
      record('assistant-1', 'assistant', 'turn-1'),
      record('tool-1', 'tool', 'turn-1'),
    ])

    expect(groups).toHaveLength(1)
    expect(groups[0].key).toBe('turn-1')
    expect(groups[0].records.map(item => item.id)).toEqual(['user-1', 'assistant-1', 'tool-1'])
  })

  it('starts a new group for a changed turn or a second user record', () => {
    const groups = groupConversationRecords([
      record('user-1', 'user', null),
      record('assistant-1', 'assistant', null),
      record('user-2', 'user', null),
      record('assistant-2', 'assistant', 'turn-2'),
    ])

    expect(groups.map(group => group.records.map(item => item.id))).toEqual([
      ['user-1', 'assistant-1'],
      ['user-2'],
      ['assistant-2'],
    ])
  })

  it('keeps keys unique when one turn contains multiple user records', () => {
    const groups = groupConversationRecords([
      record('user-1', 'user', 'turn-1'),
      record('assistant-1', 'assistant', 'turn-1'),
      record('user-2', 'user', 'turn-1'),
    ])

    expect(groups.map(group => group.key)).toEqual(['turn-1', 'turn-1:user-2'])
  })
})
