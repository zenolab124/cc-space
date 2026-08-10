import type {
  ConversationRecord,
  EngineSegment,
  EngineItemStatus,
  NormalizedRuntimeEvent,
  RuntimeEventEnvelope,
} from './types'

const OPTIMISTIC_RECORD = 'optimistic'

export interface RuntimeTimelineEffect {
  records: ConversationRecord[]
  changed: boolean
  completedTurnId: string | null
  error: string | null
  refreshActions: boolean
}

function cloneRecord(record: ConversationRecord): ConversationRecord {
  return {
    ...record,
    segments: record.segments.map(segment => ({ ...segment })),
    sourceMeta: { ...record.sourceMeta },
  }
}

function bindLatestOptimisticUser(records: ConversationRecord[], turnId: string): boolean {
  for (let index = records.length - 1; index >= 0; index--) {
    const record = records[index]
    if (
      record.role === 'user'
      && record.turnId === null
      && record.sourceMeta[OPTIMISTIC_RECORD] === true
    ) {
      record.turnId = turnId
      return true
    }
  }
  return false
}

function appendSegment(record: ConversationRecord, segment: EngineSegment): void {
  const last = record.segments[record.segments.length - 1]
  if (last?.kind === 'text' && segment.kind === 'text' && last.phase === segment.phase) {
    last.text += segment.text
    return
  }
  if (last?.kind === 'reasoning' && segment.kind === 'reasoning') {
    last.text += segment.text
    return
  }
  if (
    last?.kind === 'commandExecution'
    && segment.kind === 'commandExecution'
    && last.id === segment.id
  ) {
    if (!last.command) last.command = segment.command
    if (!last.cwd) last.cwd = segment.cwd
    last.output = `${last.output ?? ''}${segment.output ?? ''}` || null
    last.status = segment.status
    return
  }
  record.segments.push({ ...segment })
}

function completeItem(
  records: ConversationRecord[],
  turnId: string,
  itemId: string,
  status: EngineItemStatus,
): boolean {
  const record = records.find(item => item.turnId === turnId && item.id === itemId)
  if (!record) return false
  let changed = false
  for (const segment of record.segments) {
    if (
      (segment.kind === 'commandExecution' || segment.kind === 'fileChange')
      && segment.id === itemId
      && segment.status !== status
    ) {
      segment.status = status
      changed = true
    }
  }
  return changed
}

function ensureDeltaRecord(
  records: ConversationRecord[],
  envelope: RuntimeEventEnvelope,
  turnId: string,
  itemId: string,
  sourceMeta: Record<string, unknown>,
): ConversationRecord {
  let record = records.find(item => item.turnId === turnId && item.id === itemId)
  if (record) return record
  record = {
    id: itemId,
    session: envelope.session,
    turnId,
    parentId: null,
    role: 'assistant',
    timestamp: envelope.timestamp,
    segments: [],
    usage: null,
    sourceMeta: { ...sourceMeta },
  }
  records.push(record)
  return record
}

/**
 * 标准运行时事件的唯一时间线 reducer。所有事件分支都在这里穷举，避免新增协议事件
 * 被某个会话界面静默忽略。
 */
export function reduceRuntimeTimeline(
  current: ConversationRecord[],
  envelope: RuntimeEventEnvelope,
  sourceMeta: Record<string, unknown> = {},
): RuntimeTimelineEffect {
  const records = current.map(cloneRecord)
  const event: NormalizedRuntimeEvent = envelope.event
  let changed = false
  let completedTurnId: string | null = null
  let error: string | null = null
  let refreshActions = false

  switch (event.kind) {
    case 'turnStarted':
      changed = bindLatestOptimisticUser(records, event.turnId)
      break
    case 'itemDelta': {
      const record = ensureDeltaRecord(records, envelope, event.turnId, event.itemId, sourceMeta)
      appendSegment(record, event.segment)
      changed = true
      break
    }
    case 'itemCompleted':
      changed = completeItem(records, event.turnId, event.itemId, event.status)
      break
    case 'turnCompleted':
      changed = bindLatestOptimisticUser(records, event.turnId)
      completedTurnId = event.turnId
      error = event.error
      break
    case 'runtimeError':
      error = event.message
      break
    case 'runtimeExited':
    case 'capabilitiesChanged':
      refreshActions = true
      break
    case 'sessionAttached':
    case 'sessionDetached':
    case 'itemStarted':
    case 'interactionRequested':
    case 'interactionResolved':
      break
    default: {
      const exhaustive: never = event
      return exhaustive
    }
  }

  return { records, changed, completedTurnId, error, refreshActions }
}

export function optimisticUserSourceMeta(): Record<string, unknown> {
  return { [OPTIMISTIC_RECORD]: true }
}

function textOf(record: ConversationRecord): string {
  return record.segments
    .filter((segment): segment is Extract<EngineSegment, { kind: 'text' }> => segment.kind === 'text')
    .map(segment => segment.text)
    .join('\n')
    .trim()
}

/** 只摘除已经被历史时间线确认落账的直播记录；未落盘内容始终保留。 */
export function reconcileLiveRecords(
  persisted: ConversationRecord[],
  live: ConversationRecord[],
  completedTurnIds: ReadonlySet<string>,
): ConversationRecord[] {
  const persistedIds = new Set(persisted.map(record => `${record.turnId ?? ''}\u001f${record.id}`))
  const persistedUsers = new Map<string, Set<string>>()
  for (const record of persisted) {
    if (record.role !== 'user' || !record.turnId) continue
    const values = persistedUsers.get(record.turnId) ?? new Set<string>()
    values.add(textOf(record))
    persistedUsers.set(record.turnId, values)
  }

  return live.filter(record => {
    if (!record.turnId || !completedTurnIds.has(record.turnId)) return true
    if (persistedIds.has(`${record.turnId}\u001f${record.id}`)) return false
    if (record.role !== 'user' || record.sourceMeta[OPTIMISTIC_RECORD] !== true) return true
    return !persistedUsers.get(record.turnId)?.has(textOf(record))
  })
}

export function hasLiveTurn(records: ConversationRecord[], turnId: string): boolean {
  return records.some(record => record.turnId === turnId)
}
