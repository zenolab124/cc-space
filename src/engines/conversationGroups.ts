import type { ConversationRecord } from './types'

export interface EngineConversationGroup {
  key: string
  turnId: string | null
  records: ConversationRecord[]
}

export function groupConversationRecords(records: readonly ConversationRecord[]): EngineConversationGroup[] {
  const groups: EngineConversationGroup[] = []
  const keyOccurrences = new Map<string, number>()
  for (const record of records) {
    const current = groups[groups.length - 1]
    const startsNewTurn = !current
      || (!!record.turnId && record.turnId !== current.turnId)
      || (record.role === 'user' && current.records.some(item => item.role === 'user'))
    if (startsNewTurn) {
      const baseKey = record.turnId || record.id
      const occurrence = keyOccurrences.get(baseKey) ?? 0
      keyOccurrences.set(baseKey, occurrence + 1)
      groups.push({
        key: occurrence === 0 ? baseKey : `${baseKey}:${record.id}`,
        turnId: record.turnId,
        records: [record],
      })
    } else {
      current.records.push(record)
    }
  }
  return groups
}
