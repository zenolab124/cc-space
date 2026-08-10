import type { ConversationRecord, EngineSegment } from './types'

export interface EngineResponseSegmentEntry {
  key: string
  segment: EngineSegment
}

type ToolCallSegment = Extract<EngineSegment, { kind: 'toolCall' }>
type ToolResultSegment = Extract<EngineSegment, { kind: 'toolResult' }>
type CommandSegment = Extract<EngineSegment, { kind: 'commandExecution' }>
type FileChangeSegment = Extract<EngineSegment, { kind: 'fileChange' }>

export type EngineProcessActivity =
  | {
    kind: 'tool'
    key: string
    id: string
    call: ToolCallSegment | null
    result: ToolResultSegment | null
  }
  | { kind: 'command'; key: string; id: string; segment: CommandSegment }
  | { kind: 'fileChange'; key: string; id: string; segment: FileChangeSegment }

export type EngineProcessVisualState = 'done' | 'running' | 'error' | 'interrupted' | 'unknown'

export type EngineResponseBlock =
  | { kind: 'process'; key: string; entries: EngineResponseSegmentEntry[] }
  | { kind: 'content'; key: string; entry: EngineResponseSegmentEntry }

export function isEngineThoughtSegment(segment: EngineSegment): boolean {
  return segment.kind === 'reasoning'
    || (segment.kind === 'text' && segment.phase === 'progress')
}

export function isEngineProcessSegment(segment: EngineSegment): boolean {
  return [
    'commandExecution',
    'fileChange',
    'toolCall',
    'toolResult',
  ].includes(segment.kind)
}

export function buildEngineResponseBlocks(
  records: Array<Pick<ConversationRecord, 'id' | 'segments'>>,
  groupToolActivity = true,
): EngineResponseBlock[] {
  const blocks: EngineResponseBlock[] = []
  let processBlock: Extract<EngineResponseBlock, { kind: 'process' }> | null = null

  for (const record of records) {
    record.segments.forEach((segment, index) => {
      const entry = { key: `${record.id}:${index}`, segment }
      if (groupToolActivity && isEngineProcessSegment(segment)) {
        if (!processBlock) {
          processBlock = {
            kind: 'process',
            key: `process:${entry.key}`,
            entries: [],
          }
          blocks.push(processBlock)
        }
        processBlock.entries.push(entry)
        return
      }
      processBlock = null
      blocks.push({ kind: 'content', key: entry.key, entry })
    })
  }

  return blocks
}

/** 将不同引擎的中立 Segment 收束为前端唯一消费的工具活动模型。 */
export function buildEngineProcessActivities(
  entries: EngineResponseSegmentEntry[],
): EngineProcessActivity[] {
  const activities: EngineProcessActivity[] = []
  const calls = new Map<string, Extract<EngineProcessActivity, { kind: 'tool' }>>()

  for (const entry of entries) {
    const segment = entry.segment
    if (segment.kind === 'toolCall') {
      const activity: Extract<EngineProcessActivity, { kind: 'tool' }> = {
        kind: 'tool',
        key: entry.key,
        id: segment.id,
        call: segment,
        result: null,
      }
      activities.push(activity)
      calls.set(segment.id, activity)
      continue
    }
    if (segment.kind === 'toolResult') {
      const call = calls.get(segment.callId)
      if (call && call.result === null) {
        call.result = segment
      } else {
        activities.push({
          kind: 'tool',
          key: entry.key,
          id: segment.callId || entry.key,
          call: null,
          result: segment,
        })
      }
      continue
    }
    if (segment.kind === 'commandExecution') {
      activities.push({ kind: 'command', key: entry.key, id: segment.id, segment })
      continue
    }
    if (segment.kind === 'fileChange') {
      activities.push({ kind: 'fileChange', key: entry.key, id: segment.id, segment })
    }
  }

  return activities
}

export function engineProcessActivityState(
  activity: EngineProcessActivity,
  active: boolean,
): EngineProcessVisualState {
  if (activity.kind === 'tool') {
    if (activity.result?.isError) return 'error'
    if (activity.result) return 'done'
    return activity.call && active ? 'running' : 'unknown'
  }

  const status = activity.segment.status
  if (status === 'completed') return 'done'
  if (status === 'failed' || status === 'declined') return 'error'
  if (status === 'interrupted') return 'interrupted'
  if (status === 'running' || status === 'pending') return 'running'
  return active ? 'running' : 'unknown'
}
