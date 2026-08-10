import type { ContentBlock } from '@/types'

export type ToolUseBlock = Extract<ContentBlock, { type: 'tool_use' }>

export type ToolActionKind = 'read' | 'change' | 'search' | 'run' | 'web' | 'delegate' | 'skill' | 'orchestration' | 'other'

export interface ToolProcessSummaryItem {
  kind: ToolActionKind
  name: string
  count: number
  detail: string
}

export function isThinkingBlock(block: ContentBlock): boolean {
  return block.type === 'thinking' || block.type === 'redacted_thinking'
}

export type ToolBlockSegment =
  | { kind: 'block'; key: string; block: ContentBlock }
  | { kind: 'tools'; key: string; blocks: ContentBlock[]; tools: ToolUseBlock[] }

export function isToolUseBlock(block: ContentBlock): block is ToolUseBlock {
  return block.type === 'tool_use'
    && typeof (block as { id?: unknown }).id === 'string'
    && typeof (block as { name?: unknown }).name === 'string'
    && typeof (block as { input?: unknown }).input === 'object'
    && (block as { input?: unknown }).input !== null
}

function blockKey(block: ContentBlock, index: number): string {
  if (isToolUseBlock(block)) return `tool:${block.id}`
  if (block.type === 'tool_result') return `result:${block.tool_use_id}:${index}`
  return `${block.type}:${index}`
}

export function segmentToolBlocks(blocks: ContentBlock[]): ToolBlockSegment[] {
  const segments: ToolBlockSegment[] = []
  let pendingBlocks: ContentBlock[] = []
  let pendingTools: ToolUseBlock[] = []

  function flushTools() {
    if (pendingTools.length === 0) return
    segments.push({
      kind: 'tools',
      key: `tools:${pendingTools[0].id}`,
      blocks: pendingBlocks,
      tools: pendingTools,
    })
    pendingBlocks = []
    pendingTools = []
  }

  blocks.forEach((block, index) => {
    if (isToolUseBlock(block)) {
      pendingBlocks.push(block)
      pendingTools.push(block)
      return
    }
    if (isThinkingBlock(block)) {
      flushTools()
      segments.push({ kind: 'block', key: blockKey(block, index), block })
      return
    }
    flushTools()
    segments.push({ kind: 'block', key: blockKey(block, index), block })
  })
  flushTools()
  return segments
}

export function startsWithToolProcess(blocks: ContentBlock[]): boolean {
  return segmentToolBlocks(blocks)[0]?.kind === 'tools'
}

export function endsWithToolProcess(blocks: ContentBlock[]): boolean {
  const segments = segmentToolBlocks(blocks)
  return segments.length > 0 && segments[segments.length - 1].kind === 'tools'
}

export function joinsToolRun(previous: ContentBlock[], next: ContentBlock[]): boolean {
  return endsWithToolProcess(previous)
    && startsWithToolProcess(next)
    && !previous.some(isThinkingBlock)
    && !next.some(isThinkingBlock)
}

export function toolSummary(tool: ToolUseBlock): string {
  const input = tool.input ?? {}
  const candidate = [
    input.description,
    input.file_path,
    input.path,
    input.command,
    input.query,
    input.pattern,
    input.url,
    input.prompt,
    input.skill,
    input.name,
  ].find(value => typeof value === 'string' && value.trim().length > 0)

  if (typeof candidate !== 'string') return tool.name
  return candidate.replace(/\s+/g, ' ').trim()
}

export function isOrchestrationTool(tool: ToolUseBlock): boolean {
  return tool._presentation === 'orchestration'
}

function toolActionKind(tool: ToolUseBlock): ToolActionKind {
  if (isOrchestrationTool(tool)) return 'orchestration'
  switch (tool.name.toLowerCase()) {
    case 'read': return 'read'
    case 'edit':
    case 'write':
    case 'notebookedit': return 'change'
    case 'grep':
    case 'glob': return 'search'
    case 'bash': return 'run'
    case 'websearch':
    case 'webfetch': return 'web'
    case 'task':
    case 'agent':
    case 'workflow': return 'delegate'
    case 'skill': return 'skill'
    default: return 'other'
  }
}

function compactDetail(value: string, maxLength = 36): string {
  const normalized = value.replace(/\s+/g, ' ').trim()
  if (normalized.length <= maxLength) return normalized
  return `${normalized.slice(0, maxLength - 1).trimEnd()}…`
}

function lastPathPart(value: string): string {
  return value.split(/[/\\]/).filter(Boolean).pop() || value
}

function toolProcessDetail(tool: ToolUseBlock, kind: ToolActionKind): string {
  const input = tool.input ?? {}
  const firstString = (...values: unknown[]): string => {
    const value = values.find(item => typeof item === 'string' && item.trim().length > 0)
    return typeof value === 'string' ? value : ''
  }

  let detail = ''
  if (kind === 'read' || kind === 'change') {
    detail = lastPathPart(firstString(input.file_path, input.notebook_path, input.path))
  } else if (kind === 'run') {
    detail = firstString(input.description, input.command)
  } else if (kind === 'search') {
    detail = firstString(input.pattern, input.glob, input.query, input.path)
  } else if (kind === 'web') {
    detail = firstString(input.query, input.url)
  } else if (kind === 'delegate') {
    detail = firstString(input.description, input.prompt, input.name)
  } else if (kind === 'skill') {
    detail = firstString(input.skill, input.name)
  } else {
    const summary = toolSummary(tool)
    detail = summary === tool.name ? '' : summary
  }
  return compactDetail(detail)
}

function readableToolName(name: string): string {
  if (!name.startsWith('mcp__')) return name
  const [, server, ...toolParts] = name.split('__')
  return server && toolParts.length ? `${server}/${toolParts.join('/')}` : name
}

/**
 * 把同一执行过程压缩成按动作分组的概览。详情只保留首个代表项，
 * 让折叠行回答“做了什么”，完整输入仍由展开内容承载。
 */
export function summarizeToolProcess(tools: readonly ToolUseBlock[]): ToolProcessSummaryItem[] {
  const groups = new Map<string, ToolProcessSummaryItem>()
  for (const tool of tools) {
    const kind = toolActionKind(tool)
    const name = readableToolName(tool.name)
    const key = kind === 'other' ? `${kind}:${name}` : kind
    const existing = groups.get(key)
    if (existing) {
      existing.count += 1
      continue
    }
    groups.set(key, {
      kind,
      name,
      count: 1,
      detail: toolProcessDetail(tool, kind),
    })
  }
  return [...groups.values()]
}

export function findPendingPermissionToolUseId(
  blockLists: readonly ContentBlock[][],
  request: { toolName: string; input: Record<string, unknown> } | null,
  results: ReadonlyMap<string, unknown>,
): string | null {
  if (!request) return null
  for (let li = blockLists.length - 1; li >= 0; li--) {
    const blocks = blockLists[li]
    for (let bi = blocks.length - 1; bi >= 0; bi--) {
      const block = blocks[bi]
      if (
        isToolUseBlock(block)
        && block.name === request.toolName
        && sameToolInput(block.input, request.input)
        && !results.has(block.id)
      ) return block.id
    }
  }
  return null
}

export function sameToolInput(a: Record<string, unknown>, b: Record<string, unknown>): boolean {
  try {
    return JSON.stringify(a) === JSON.stringify(b)
  } catch {
    return false
  }
}
