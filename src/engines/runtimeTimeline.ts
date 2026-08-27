import type {
  ConversationRecord,
  EngineSegment,
  EngineItemStatus,
  NormalizedRuntimeEvent,
  RuntimeEventEnvelope,
  RuntimeSnapshot,
  SessionRef,
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

function setTurnErrorRecord(
  records: ConversationRecord[],
  envelope: RuntimeEventEnvelope,
  turnId: string,
  message: string | null,
): boolean {
  const recordId = `turn-error-${turnId}`
  const index = records.findIndex(record =>
    record.id === recordId
    || (record.turnId === turnId && record.sourceMeta.turnError === true),
  )
  const normalized = message?.trim() ?? ''
  if (!normalized) {
    if (index < 0) return false
    records.splice(index, 1)
    return true
  }

  const record: ConversationRecord = {
    id: recordId,
    session: envelope.session,
    turnId,
    parentId: null,
    role: 'system',
    timestamp: envelope.timestamp,
    segments: [{ kind: 'text', text: normalized, phase: 'final' }],
    usage: null,
    sourceMeta: { turnError: true },
  }
  if (index < 0) records.push(record)
  else records[index] = record
  return true
}

/**
 * 视觉运行状态只按事件通道的有序生命周期收口。普通快照可补上活动 turn，
 * 但不能用提前到达的 idle 快照越过仍在排空的增量事件。
 */
export function reduceRuntimeVisualActivity(
  activeTurnId: string | null,
  event: NormalizedRuntimeEvent,
): string | null {
  switch (event.kind) {
    case 'turnStarted':
      return event.turnId
    case 'turnCompleted':
      return activeTurnId === event.turnId ? null : activeTurnId
    case 'runtimeError':
    case 'runtimeExited':
    case 'sessionDetached':
      return null
    default:
      return activeTurnId
  }
}

export function syncRuntimeVisualActivity(
  activeTurnId: string | null,
  snapshot: RuntimeSnapshot,
  authoritative = false,
): string | null {
  const snapshotTurnId = (
    snapshot.phase === 'running' || snapshot.phase === 'awaitingInteraction'
  ) ? snapshot.activeTurnId : null
  return authoritative ? snapshotTurnId : snapshotTurnId ?? activeTurnId
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
      changed = setTurnErrorRecord(records, envelope, event.turnId, event.error) || changed
      completedTurnId = event.turnId
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

export interface OptimisticUserImage {
  id: string
  dataUrl: string
  mediaType: string
}

/** 构造标准引擎统一使用的乐观用户记录，普通输入与赛马广播保持同一交接语义。 */
export function createOptimisticUserRecord(
  session: SessionRef,
  id: string,
  text: string,
  images: OptimisticUserImage[] = [],
  turnId: string | null = null,
): ConversationRecord {
  return {
    id,
    session,
    turnId,
    parentId: null,
    role: 'user',
    timestamp: new Date().toISOString(),
    segments: text ? [{ kind: 'text', text }] : [],
    usage: null,
    sourceMeta: {
      ...optimisticUserSourceMeta(),
      optimisticImages: images,
    },
  }
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
  // source reload 与运行时事件来自两条异步通道。即使 reload 已完成，较晚到达的
  // live delta 也必须在渲染前再次与历史对账，不能等下一次 reload 才消失。
  const unlandedLive = reconcileLiveRecords([...persisted], [...live])
  for (const record of unlandedLive) {
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

    if (record.role === 'user' && record.sourceMeta.optimisticPlacement !== 'tail') {
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
  // 历史源一旦给出 final，该 turn 已是完整权威快照。实时源可能仍在消化同一轮的
  // 文本或工具事件；必须整轮原子退场，逐条匹配会短暂拼出错误顺序和重复答复。
  const completedTurns = new Set(persisted.flatMap(record =>
    record.turnId
      && record.role === 'assistant'
      && record.segments.some(segment =>
        segment.kind === 'text' && segment.phase === 'final' && !!segment.text,
      )
      ? [record.turnId]
      : [],
  ))
  const unclaimedPersisted = [...persisted]
  return live.filter(record => {
    if (record.turnId && completedTurns.has(record.turnId)) return false
    const index = landedRecordIndex(unclaimedPersisted, record)
    if (index < 0) return true
    unclaimedPersisted.splice(index, 1)
    return false
  })
}

export function hasLiveTurn(records: ConversationRecord[], turnId: string): boolean {
  return records.some(record => record.turnId === turnId)
}
