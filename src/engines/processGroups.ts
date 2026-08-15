import type { ConversationRecord, EngineSegment } from './types'
import type { ContentBlock } from '@/types'
import type { ToolVisualState } from '@/composables/useToolDisplay'
import { usesInlineToolResult, type ToolResultData } from '@/utils/toolPair'

export interface EngineResponseSegmentEntry {
  key: string
  segment: EngineSegment
}

type EngineResponseRecord = Pick<ConversationRecord, 'id' | 'segments'>
  & Partial<Pick<ConversationRecord, 'sourceMeta'>>

export type EngineResponseBlock =
  | { kind: 'process'; key: string; entries: EngineResponseSegmentEntry[]; processGroupKey: string | null }
  | { kind: 'content'; key: string; entry: EngineResponseSegmentEntry }

export function isEngineThoughtSegment(segment: EngineSegment): boolean {
  return segment.kind === 'reasoning'
    || (segment.kind === 'text' && segment.phase === 'progress')
}

export function isRenderableEngineSegment(
  segment: EngineSegment,
  showThoughtProcess = true,
): boolean {
  if (!showThoughtProcess && isEngineThoughtSegment(segment)) return false
  if (segment.kind === 'unknown') return !!segment.summary?.trim()
  if (segment.kind === 'reasoning') {
    return segment.visibility === 'redacted' || !!segment.text.trim()
  }
  if (segment.kind === 'text') return !!segment.text.trim()
  return true
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
  records: EngineResponseRecord[],
  groupToolActivity = true,
  processGroupKeyOf?: (record: EngineResponseRecord) => string,
): EngineResponseBlock[] {
  const blocks: EngineResponseBlock[] = []
  let processBlock: Extract<EngineResponseBlock, { kind: 'process' }> | null = null

  for (const record of records) {
    const processGroupKey = processGroupKeyOf?.(record) ?? null
    record.segments.forEach((segment, index) => {
      const entry = { key: `${record.id}:${index}`, segment }
      if (groupToolActivity && isEngineProcessSegment(segment)) {
        if (!processBlock || processBlock.processGroupKey !== processGroupKey) {
          processBlock = {
            kind: 'process',
            key: `process:${entry.key}`,
            entries: [],
            processGroupKey,
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

export interface EngineProcessProjection {
  blocks: ContentBlock[]
  results: Map<string, ToolResultData>
  states: Map<string, ToolVisualState>
}

function objectInput(value: unknown): Record<string, unknown> {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return value as Record<string, unknown>
  }
  return { value }
}

const SHARED_TOOL_NAMES = new Map([
  ['read', 'Read'],
  ['edit', 'Edit'],
  ['write', 'Write'],
  ['grep', 'Grep'],
  ['glob', 'Glob'],
  ['bash', 'Bash'],
  ['websearch', 'WebSearch'],
  ['webfetch', 'WebFetch'],
  ['task', 'Task'],
  ['agent', 'Agent'],
  ['workflow', 'Workflow'],
  ['skill', 'Skill'],
])

function sharedToolName(name: string): string {
  return SHARED_TOOL_NAMES.get(name.toLowerCase()) ?? name
}

/**
 * 结果是否应由对应工具项消费，只由标准引擎的显式展示契约决定。
 * title 是文案，不是类型标记；不能用它猜测工具种类。
 */
function consumedResultCallIds(
  entries: readonly EngineResponseSegmentEntry[],
): Set<string> {
  return new Set(entries.flatMap(entry => {
    const segment = entry.segment
    if (segment.kind !== 'toolCall') return []
    const name = sharedToolName(segment.name)
    return segment.presentation === 'orchestration' || usesInlineToolResult(name)
      ? [segment.id]
      : []
  }))
}

function sharedToolInput(name: string, value: unknown): Record<string, unknown> {
  const input = objectInput(value)
  if (
    (name === 'Read' || name === 'Edit' || name === 'Write')
    && typeof input.path === 'string'
    && typeof input.file_path !== 'string'
  ) {
    return { ...input, file_path: input.path }
  }
  return input
}

function resultContent(value: unknown): string | ContentBlock[] {
  if (typeof value === 'string') return value
  if (
    Array.isArray(value)
    && value.every(item => item && typeof item === 'object' && typeof item.type === 'string')
  ) {
    return value as ContentBlock[]
  }
  try {
    return JSON.stringify(value, null, 2) ?? String(value)
  } catch (_) {
    return String(value)
  }
}

function visualState(status: string): ToolVisualState {
  if (status === 'completed') return 'done'
  if (status === 'failed' || status === 'declined') return 'error'
  if (status === 'interrupted') return 'interrupted'
  if (status === 'running' || status === 'pending') return 'running'
  return 'unknown'
}

/**
 * 把各引擎的中立工具 Segment 投影到 Claude 会话已经使用的 ContentBlock 契约。
 * 渲染层从这里开始完全复用同一套分组、折叠和工具卡片，不再维护引擎专属卡片。
 */
export function projectEngineProcessEntries(
  entries: EngineResponseSegmentEntry[],
  pairingEntries: readonly EngineResponseSegmentEntry[] = entries,
): EngineProcessProjection {
  const blocks: ContentBlock[] = []
  const results = new Map<string, ToolResultData>()
  const states = new Map<string, ToolVisualState>()
  const consumedResultIds = consumedResultCallIds(pairingEntries)

  for (const entry of entries) {
    const segment = entry.segment
    if (segment.kind === 'toolCall') {
      const name = sharedToolName(segment.name)
      blocks.push({
        type: 'tool_use',
        id: segment.id,
        name,
        input: sharedToolInput(name, segment.input),
        ...(segment.title ? { _title: segment.title } : {}),
        ...(segment.presentation ? { _presentation: segment.presentation } : {}),
      })
      continue
    }
    if (segment.kind === 'toolResult') {
      const content = resultContent(segment.content)
      results.set(segment.callId, {
        content,
        is_error: segment.isError,
        attachments: segment.attachments,
        recordUuid: null,
      })
      if (!consumedResultIds.has(segment.callId)) {
        blocks.push({
          type: 'tool_result',
          tool_use_id: segment.callId,
          content,
          is_error: segment.isError,
        })
      }
      continue
    }
    if (segment.kind === 'commandExecution') {
      blocks.push({
        type: 'tool_use',
        id: segment.id,
        name: 'Bash',
        input: {
          command: segment.command,
          ...(segment.cwd ? { cwd: segment.cwd } : {}),
        },
      })
      if (segment.output !== null) {
        results.set(segment.id, {
          content: segment.output,
          is_error: segment.status === 'failed' || segment.status === 'declined',
          recordUuid: null,
        })
      }
      states.set(segment.id, visualState(segment.status))
      continue
    }
    if (segment.kind === 'fileChange') {
      const changes = segment.changes.length > 0
        ? segment.changes
        : [{ path: '', kind: 'update', diff: null }]
      changes.forEach((change, index) => {
        const id = changes.length === 1 ? segment.id : `${segment.id}:${index}`
        blocks.push({
          type: 'tool_use',
          id,
          name: 'Edit',
          input: {
            file_path: change.path,
            change_kind: change.kind,
            ...(change.diff ? { new_string: change.diff } : {}),
          },
        })
        states.set(id, visualState(segment.status))
      })
    }
  }

  return { blocks, results, states }
}
