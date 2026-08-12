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

/** 用 turn/start 的权威返回值收口乐观用户消息，避免依赖通知与 source reload 的先后。 */
export function bindOptimisticUserTurn(
  records: ConversationRecord[],
  recordId: string,
  turnId: string,
): ConversationRecord[] {
  let changed = false
  const next = records.map(record => {
    if (
      record.id !== recordId
      || record.role !== 'user'
      || record.sourceMeta[OPTIMISTIC_RECORD] !== true
      || record.turnId === turnId
    ) return record
    changed = true
    return { ...record, turnId }
  })
  return changed ? next : records
}

/**
 * 历史源与实时源会独立收敛；按 turn 把实时记录插回历史槽位，不能简单把 live
 * 拼在末尾，否则局部落账时同一轮会被拆成“回复在上、用户在下”两个组。
 */
export function composeRuntimeTimeline(
  persisted: readonly ConversationRecord[],
  live: readonly ConversationRecord[],
): ConversationRecord[] {
  const merged = [...persisted]
  for (const record of live) {
    if (!record.turnId) {
      merged.push(record)
      continue
    }

    const sameTurnIndexes: number[] = []
    for (let index = 0; index < merged.length; index++) {
      if (merged[index].turnId === record.turnId) sameTurnIndexes.push(index)
    }
    if (sameTurnIndexes.length === 0) {
      merged.push(record)
      continue
    }

    if (record.role === 'user') {
      const firstResponse = sameTurnIndexes.find(index => merged[index].role !== 'user')
      const insertAt = firstResponse ?? sameTurnIndexes[sameTurnIndexes.length - 1] + 1
      merged.splice(insertAt, 0, record)
    } else {
      merged.splice(sameTurnIndexes[sameTurnIndexes.length - 1] + 1, 0, record)
    }
  }
  return merged
}

function textOf(record: ConversationRecord): string {
  return record.segments
    .filter((segment): segment is Extract<EngineSegment, { kind: 'text' }> => segment.kind === 'text')
    .map(segment => segment.text)
    .join('\n')
    .trim()
}

function textualFingerprint(record: ConversationRecord): string | null {
  if (record.segments.length === 0) return null
  const values: string[] = []
  for (const segment of record.segments) {
    if (segment.kind !== 'text' && segment.kind !== 'reasoning') return null
    values.push(`${segment.kind}\u001f${segment.text}`)
  }
  return `${record.role}\u001e${values.join('\u001d')}`
}

function userInputFingerprint(record: ConversationRecord): string | null {
  if (record.role !== 'user') return null
  const optimisticImages = Array.isArray(record.sourceMeta.optimisticImages)
    ? record.sourceMeta.optimisticImages.length
    : 0
  const landedAttachments = record.segments.filter(segment => segment.kind === 'attachment').length
  const attachmentCount = optimisticImages || landedAttachments
  const text = textOf(record)
  return text || attachmentCount > 0
    ? `${text}\u001f${attachmentCount}`
    : null
}

function landedRecordIndex(
  persisted: readonly ConversationRecord[],
  live: ConversationRecord,
): number {
  if (!live.turnId) return -1
  const exact = persisted.findIndex(record =>
    record.turnId === live.turnId && record.id === live.id,
  )
  if (exact >= 0) return exact

  const fingerprint = textualFingerprint(live)
  if (fingerprint) {
    const semantic = persisted.findIndex(record =>
      record.turnId === live.turnId
      && textualFingerprint(record) === fingerprint,
    )
    if (semantic >= 0) return semantic
  }

  if (live.sourceMeta[OPTIMISTIC_RECORD] !== true) return -1
  const userInput = userInputFingerprint(live)
  if (!userInput) return -1
  return persisted.findIndex(record =>
    record.turnId === live.turnId
    && userInputFingerprint(record) === userInput,
  )
}

/** 只摘除已经被历史时间线确认落账的直播记录；未落盘内容始终保留。 */
export function reconcileLiveRecords(
  persisted: ConversationRecord[],
  live: ConversationRecord[],
): ConversationRecord[] {
  const unclaimedPersisted = [...persisted]
  return live.filter(record => {
    const index = landedRecordIndex(unclaimedPersisted, record)
    if (index < 0) return true
    unclaimedPersisted.splice(index, 1)
    return false
  })
}

export function hasLiveTurn(records: ConversationRecord[], turnId: string): boolean {
  return records.some(record => record.turnId === turnId)
}
