import { describe, expect, it } from 'vitest'
import type { ContentBlock } from '@/types'
import {
  findPendingPermissionToolUseId,
  joinsToolRun,
  segmentToolBlocks,
  toolSummary,
} from '@/utils/toolDisplay'
import { deriveToolVisualState } from '@/composables/useToolDisplay'

function tool(id: string, name = 'Read', input: Record<string, unknown> = {}): ContentBlock {
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

  it('keeps thinking inside a tool process without reordering blocks', () => {
    const thinking: ContentBlock = { type: 'thinking', thinking: 'inspect the result' }
    const blocks: ContentBlock[] = [
      tool('a'),
      thinking,
      tool('b', 'Bash'),
      { type: 'text', text: 'done' },
      tool('c'),
    ]

    const segments = segmentToolBlocks(blocks)
    expect(segments.map(segment => segment.kind)).toEqual(['tools', 'block', 'tools'])
    if (segments[0].kind === 'tools') {
      expect(segments[0].blocks).toEqual([blocks[0], thinking, blocks[2]])
      expect(segments[0].tools.map(item => item.id)).toEqual(['a', 'b'])
    }
    expect(segments[1]).toMatchObject({ kind: 'block', block: { type: 'text', text: 'done' } })

    const leading = segmentToolBlocks([thinking, tool('d')])
    expect(leading).toHaveLength(1)
    if (leading[0].kind === 'tools') expect(leading[0].blocks).toEqual([thinking, expect.objectContaining({ id: 'd' })])

    const standalone = segmentToolBlocks([thinking, { type: 'text', text: 'answer' }])
    expect(standalone.map(segment => segment.kind)).toEqual(['block', 'block'])
  })

  it('joins adjacent assistant tool runs across trailing and leading thinking', () => {
    const thinking: ContentBlock = { type: 'thinking', thinking: 'continue' }
    expect(joinsToolRun([tool('a'), thinking], [thinking, tool('b')])).toBe(true)
    expect(joinsToolRun([tool('a')], [{ type: 'text', text: 'answer' }, tool('b')])).toBe(false)
    expect(joinsToolRun([{ type: 'text', text: 'answer' }], [tool('b')])).toBe(false)
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
