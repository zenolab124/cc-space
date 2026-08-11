import { describe, expect, it } from 'vitest'
import type { ContentBlock } from '@/types'
import {
  findPendingPermissionToolUseId,
  isOrchestrationTool,
  isOrchestrationToolSegment,
  isThinkingBlock,
  joinsToolRun,
  segmentToolBlocks,
  summarizeToolProcess,
  toolDisplayTitle,
  toolSummary,
} from '@/utils/toolDisplay'
import { deriveToolVisualState } from '@/composables/useToolDisplay'

function tool(
  id: string,
  name = 'Read',
  input: Record<string, unknown> = {},
): Extract<ContentBlock, { type: 'tool_use' }> {
  return { type: 'tool_use', id, name, input }
}

describe('tool display projection', () => {
  it('keeps source order and groups only consecutive tools', () => {
    const blocks: ContentBlock[] = [
      { type: 'text', text: 'before' },
      tool('a'),
      tool('b', 'Bash'),
      { type: 'text', text: 'between' },
      tool('c'),
    ]

    const segments = segmentToolBlocks(blocks)
    expect(segments.map(segment => segment.kind)).toEqual(['block', 'tools', 'block', 'tools'])
    expect(segments[1]).toMatchObject({ kind: 'tools', key: 'tools:a' })
    expect(segments[3]).toMatchObject({ kind: 'tools', key: 'tools:c' })
    if (segments[1].kind === 'tools') {
      expect(segments[1].tools.map(item => item.id)).toEqual(['a', 'b'])
    }
  })

  it('keeps thinking between tool processes as a separate foldable block', () => {
    const thinking: ContentBlock = { type: 'thinking', thinking: 'inspect the result' }
    const blocks: ContentBlock[] = [
      tool('a'),
      thinking,
      tool('b', 'Bash'),
      { type: 'text', text: 'done' },
      tool('c'),
    ]

    const segments = segmentToolBlocks(blocks)
    expect(segments.map(segment => segment.kind)).toEqual(['tools', 'block', 'tools', 'block', 'tools'])
    if (segments[0].kind === 'tools') {
      expect(segments[0].blocks).toEqual([blocks[0]])
      expect(segments[0].tools.map(item => item.id)).toEqual(['a'])
    }
    expect(segments[1]).toMatchObject({ kind: 'block', block: thinking })
    expect(segments[2]).toMatchObject({ kind: 'tools', tools: [expect.objectContaining({ id: 'b' })] })
    expect(segments[3]).toMatchObject({ kind: 'block', block: { type: 'text', text: 'done' } })
    expect(segments[4]).toMatchObject({ kind: 'tools', tools: [blocks[4]] })

    const leading = segmentToolBlocks([thinking, tool('d')])
    expect(leading.map(segment => segment.kind)).toEqual(['block', 'tools'])
    expect(leading[0]).toMatchObject({ kind: 'block', block: thinking })
    if (leading[1].kind === 'tools') expect(leading[1].blocks).toEqual([expect.objectContaining({ id: 'd' })])
    const thinkingOnly = segmentToolBlocks([thinking])[0]
    expect(leading[0].key).toBe(thinkingOnly.key)

    const standalone = segmentToolBlocks([thinking, { type: 'text', text: 'answer' }])
    expect(standalone.map(segment => segment.kind)).toEqual(['block', 'block'])
  })

  it('does not join assistant tool runs across thinking', () => {
    const thinking: ContentBlock = { type: 'thinking', thinking: 'continue' }
    expect(joinsToolRun([tool('a'), thinking], [thinking, tool('b')])).toBe(false)
    expect(joinsToolRun([tool('a')], [thinking, tool('b')])).toBe(false)
    expect(joinsToolRun([tool('a')], [{ type: 'text', text: 'answer' }, tool('b')])).toBe(false)
    expect(joinsToolRun([{ type: 'text', text: 'answer' }], [tool('b')])).toBe(false)
  })

  it('treats redacted thinking as an independent block boundary', () => {
    const redacted: ContentBlock = { type: 'redacted_thinking' }
    const segments = segmentToolBlocks([tool('a'), redacted, tool('b')])

    expect(isThinkingBlock(redacted)).toBe(true)
    expect(segments.map(segment => segment.kind)).toEqual(['tools', 'block', 'tools'])
    expect(joinsToolRun([tool('a')], [redacted, tool('b')])).toBe(false)
  })

  it('binds permission to the newest matching unresolved tool only', () => {
    const request = { toolName: 'Read', input: { file_path: '/tmp/a' } }
    const blocks = [
      [tool('history', 'Read', request.input)],
      [tool('active', 'Read', request.input)],
    ]

    expect(findPendingPermissionToolUseId(blocks, request, new Map([['history', true]]))).toBe('active')
    expect(findPendingPermissionToolUseId(blocks, request, new Map([['history', true], ['active', true]]))).toBeNull()
    expect(findPendingPermissionToolUseId(blocks, { ...request, toolName: 'Bash' }, new Map())).toBeNull()
  })

  it('uses a stable group key while a streaming group grows', () => {
    const first = segmentToolBlocks([tool('a')])[0]
    const grown = segmentToolBlocks([tool('a'), tool('b')])[0]
    expect(first.key).toBe(grown.key)
  })

  it('chooses concise summaries without changing tool input', () => {
    const block = tool('a', 'Bash', { command: 'pnpm test', description: 'Run focused tests' })
    if (block.type !== 'tool_use') throw new Error('invalid fixture')
    expect(toolSummary(block)).toBe('Run focused tests')
    expect(block.input).toEqual({ command: 'pnpm test', description: 'Run focused tests' })
  })

  it('summarizes a process by useful actions and representative details', () => {
    const blocks = [
      tool('a', 'Read', { file_path: '/workspace/src/App.vue' }),
      tool('b', 'Read', { file_path: '/workspace/src/main.ts' }),
      tool('c', 'Bash', { command: 'pnpm test', description: '运行测试' }),
      tool('d', 'WebSearch', { query: 'Open-Meteo weather API' }),
    ]
    const tools = blocks.filter(block => block.type === 'tool_use')

    expect(summarizeToolProcess(tools)).toEqual([
      { kind: 'read', name: 'Read', count: 2, detail: 'App.vue' },
      { kind: 'run', name: 'Bash', count: 1, detail: '运行测试' },
      { kind: 'web', name: 'WebSearch', count: 1, detail: 'Open-Meteo weather API' },
    ])
  })

  it('shortens long details and makes MCP names readable', () => {
    const blocks = [
      tool('a', 'Bash', { command: 'a'.repeat(50) }),
      tool('b', 'mcp__browser__navigate', { url: 'https://example.com/page' }),
    ]
    const tools = blocks.filter(block => block.type === 'tool_use')
    const summary = summarizeToolProcess(tools)

    expect(summary[0].detail).toHaveLength(36)
    expect(summary[0].detail.endsWith('…')).toBe(true)
    expect(summary[1]).toMatchObject({ kind: 'other', name: 'browser/navigate' })
  })

  it('summarizes programmatic orchestration as tool calls instead of JavaScript', () => {
    const tools = [
      { type: 'tool_use' as const, id: 'a', name: 'js', input: { value: 'code' }, _presentation: 'orchestration' as const },
      { type: 'tool_use' as const, id: 'b', name: 'js', input: { value: 'code' }, _presentation: 'orchestration' as const },
    ]

    expect(summarizeToolProcess(tools)).toEqual([
      { kind: 'orchestration', name: 'js', count: 2, detail: '' },
    ])
    expect(isOrchestrationToolSegment(tools)).toBe(true)
    expect(isOrchestrationToolSegment([tools[0], tool('regular', 'Read')])).toBe(false)
    expect(isOrchestrationToolSegment([])).toBe(false)
    const runtimeShape = {
      ...tool('runtime-shape', 'mcp__node_repl__js', { value: 'work()' }),
      _title: '识别 Monet 窗口',
      _presentation: 'orchestration' as const,
    }
    expect(isOrchestrationToolSegment([runtimeShape])).toBe(true)
    expect(toolDisplayTitle(runtimeShape)).toBe('识别 Monet 窗口')
    const archivedShape = {
      ...tool('archived-shape', 'js', { value: 'work()' }),
      _title: '检查 Monet 窗口',
    }
    expect(isOrchestrationTool(archivedShape)).toBe(false)
    expect(toolDisplayTitle(archivedShape)).toBe('检查 Monet 窗口')
    const plainTitle = tool('plain-title', 'custom', { title: '普通标题' })
    expect(isOrchestrationTool(plainTitle)).toBe(false)
    expect(toolDisplayTitle(plainTitle)).toBe('custom')

    const mixedSegments = segmentToolBlocks([
      tool('regular-before', 'Read'),
      tools[0],
      tool('regular-after', 'Bash'),
    ])
    expect(mixedSegments).toHaveLength(3)
    expect(mixedSegments.every(segment => segment.kind === 'tools')).toBe(true)
    expect(mixedSegments[1]).toMatchObject({
      kind: 'tools',
      tools: [expect.objectContaining({ id: 'a' })],
    })
  })
})

describe('tool visual state', () => {
  it('prioritizes permission and failure over streaming', () => {
    expect(deriveToolVisualState({ waitingPermission: true, streaming: true })).toBe('permission')
    expect(deriveToolVisualState({ result: { content: 'failed', is_error: true }, streaming: true })).toBe('error')
  })

  it('lets async terminal state override a successful launch placeholder', () => {
    const launchResult = { content: 'started', is_error: false }
    expect(deriveToolVisualState({ result: launchResult, asyncState: 'failed' })).toBe('error')
    expect(deriveToolVisualState({ result: launchResult, asyncState: 'killed' })).toBe('interrupted')
  })

  it('distinguishes background work, completion, and unknown history', () => {
    expect(deriveToolVisualState({ asyncState: 'running' })).toBe('background')
    expect(deriveToolVisualState({ result: { content: 'ok', is_error: false } })).toBe('done')
    expect(deriveToolVisualState({})).toBe('unknown')
  })
})
