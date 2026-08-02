import type { ContentBlock } from '@/types'

export type ToolUseBlock = Extract<ContentBlock, { type: 'tool_use' }>

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
  let leadingThinking: Array<{ block: ContentBlock; index: number }> = []

  function flushLeadingThinking() {
    for (const item of leadingThinking) {
      segments.push({ kind: 'block', key: blockKey(item.block, item.index), block: item.block })
    }
    leadingThinking = []
  }

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
      if (pendingTools.length === 0 && leadingThinking.length > 0) {
        pendingBlocks.push(...leadingThinking.map(item => item.block))
        leadingThinking = []
      }
      pendingBlocks.push(block)
      pendingTools.push(block)
      return
    }
    if (block.type === 'thinking') {
      if (pendingTools.length > 0) pendingBlocks.push(block)
      else leadingThinking.push({ block, index })
      return
    }
    flushTools()
    flushLeadingThinking()
    segments.push({ kind: 'block', key: blockKey(block, index), block })
  })
  flushTools()
  flushLeadingThinking()
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
  return endsWithToolProcess(previous) && startsWithToolProcess(next)
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
