import { describe, expect, it } from 'vitest'
import {
  buildEngineResponseBlocks,
  isEngineProcessSegment,
  isEngineThoughtSegment,
  projectEngineProcessEntries,
} from '../../src/engines/processGroups'

describe('engine process groups', () => {
  it('groups tool activity while keeping thought, final and legacy text as ordered content', () => {
    const blocks = buildEngineResponseBlocks([{
      id: 'record',
      segments: [
        { kind: 'text', text: 'Checking files', phase: 'progress' },
        { kind: 'reasoning', text: 'Summary', visibility: 'summary' },
        { kind: 'toolCall', id: 'call', name: 'read', input: {} },
        { kind: 'text', text: 'Final response', phase: 'final' },
        { kind: 'text', text: 'Legacy response' },
      ],
    }])

    expect(blocks).toHaveLength(5)
    expect(blocks[0]).toMatchObject({ kind: 'content', entry: { segment: { kind: 'text', phase: 'progress' } } })
    expect(blocks[1]).toMatchObject({ kind: 'content', entry: { segment: { kind: 'reasoning' } } })
    expect(blocks[2]).toMatchObject({ kind: 'process' })
    expect(blocks[2].kind === 'process' && blocks[2].entries).toHaveLength(1)
    expect(blocks[3]).toMatchObject({
      kind: 'content',
      entry: { segment: { kind: 'text', phase: 'final' } },
    })
    expect(blocks[4]).toMatchObject({
      kind: 'content',
      entry: { segment: { kind: 'text', text: 'Legacy response' } },
    })
  })

  it('keeps visible content before the next tool disclosure to preserve ordering', () => {
    const blocks = buildEngineResponseBlocks([{
      id: 'record',
      segments: [
        { kind: 'text', text: 'First progress', phase: 'progress' },
        { kind: 'attachment', asset: { session: {} as never, nativeId: 'asset' }, mediaType: 'image/png', title: null },
        { kind: 'fileChange', id: 'change', changes: [], status: 'completed' },
      ],
    }])

    expect(blocks.map(block => block.kind)).toEqual(['content', 'content', 'process'])
  })

  it('does not infer process semantics from unmarked text', () => {
    expect(isEngineProcessSegment({ kind: 'text', text: 'Long but unmarked' })).toBe(false)
    expect(isEngineProcessSegment({ kind: 'text', text: 'Short progress', phase: 'progress' })).toBe(false)
  })

  it('identifies every thought-bearing segment so a presentation can hide it completely', () => {
    expect(isEngineThoughtSegment({ kind: 'text', text: 'Checking', phase: 'progress' })).toBe(true)
    expect(isEngineThoughtSegment({ kind: 'reasoning', text: 'Private reasoning', visibility: 'full' })).toBe(true)
    expect(isEngineThoughtSegment({ kind: 'text', text: 'Final', phase: 'final' })).toBe(false)
    expect(isEngineThoughtSegment({ kind: 'toolCall', id: 'call', name: 'read', input: {} })).toBe(false)
  })

  it('keeps existing per-item rendering for engines without process grouping', () => {
    const blocks = buildEngineResponseBlocks([{
      id: 'record',
      segments: [
        { kind: 'reasoning', text: 'Summary', visibility: 'summary' },
        { kind: 'toolCall', id: 'call', name: 'read', input: {} },
      ],
    }], false)

    expect(blocks.map(block => block.kind)).toEqual(['content', 'content'])
  })

  it('splits consecutive process blocks when their model display rules differ', () => {
    const blocks = buildEngineResponseBlocks([
      {
        id: 'model-a-call',
        sourceMeta: { model: 'model-a' },
        segments: [{ kind: 'toolCall', id: 'call-a', name: 'read', input: {} }],
      },
      {
        id: 'model-a-result',
        sourceMeta: { model: 'model-a' },
        segments: [{ kind: 'toolResult', callId: 'call-a', content: 'ok', isError: false }],
      },
      {
        id: 'model-b-call',
        sourceMeta: { model: 'model-b' },
        segments: [{ kind: 'toolCall', id: 'call-b', name: 'read', input: {} }],
      },
    ], true, record => record.sourceMeta?.model === 'model-b' ? 'cards' : 'grouped')

    expect(blocks).toHaveLength(2)
    expect(blocks[0]).toMatchObject({ kind: 'process', processGroupKey: 'grouped' })
    expect(blocks[0].kind === 'process' && blocks[0].entries).toHaveLength(2)
    expect(blocks[1]).toMatchObject({ kind: 'process', processGroupKey: 'cards' })
    expect(blocks[1].kind === 'process' && blocks[1].entries).toHaveLength(1)
  })

  it('projects neutral tools and commands into the shared content block contract', () => {
    const projection = projectEngineProcessEntries([
      { key: 'call', segment: { kind: 'toolCall', id: 'tool-1', name: 'read', input: { path: 'a.ts' } } },
      { key: 'result', segment: { kind: 'toolResult', callId: 'tool-1', content: 'ok', isError: false } },
      { key: 'command', segment: { kind: 'commandExecution', id: 'cmd-1', command: 'pnpm test', cwd: null, output: 'pass', status: 'completed' } },
    ])

    expect(projection.blocks).toEqual([
      {
        type: 'tool_use',
        id: 'tool-1',
        name: 'Read',
        input: { path: 'a.ts', file_path: 'a.ts' },
      },
      {
        type: 'tool_result',
        tool_use_id: 'tool-1',
        content: 'ok',
        is_error: false,
      },
      {
        type: 'tool_use',
        id: 'cmd-1',
        name: 'Bash',
        input: { command: 'pnpm test' },
      },
    ])
    expect(projection.results.get('tool-1')).toMatchObject({ content: 'ok', is_error: false })
    expect(projection.results.get('cmd-1')).toMatchObject({ content: 'pass', is_error: false })
    expect(projection.states.get('cmd-1')).toBe('done')
  })

  it('projects file changes as shared edit cards with explicit states', () => {
    const projection = projectEngineProcessEntries([{
      key: 'change',
      segment: {
        kind: 'fileChange',
        id: 'change-1',
        status: 'running',
        changes: [
          { path: 'src/App.vue', kind: 'update', diff: '+line' },
          { path: 'src/main.ts', kind: 'create', diff: null },
        ],
      },
    }])

    expect(projection.blocks).toEqual([
      expect.objectContaining({
        type: 'tool_use',
        id: 'change-1:0',
        name: 'Edit',
        input: { file_path: 'src/App.vue', change_kind: 'update', new_string: '+line' },
      }),
      expect.objectContaining({
        type: 'tool_use',
        id: 'change-1:1',
        name: 'Edit',
        input: { file_path: 'src/main.ts', change_kind: 'create' },
      }),
    ])
    expect([...projection.states.values()]).toEqual(['running', 'running'])
  })

  it('keeps an unmatched result visible instead of dropping adapter data', () => {
    const projection = projectEngineProcessEntries([
      { key: 'result', segment: { kind: 'toolResult', callId: 'missing', content: 'failed', isError: true } },
    ])

    expect(projection.blocks[0]).toMatchObject({
      type: 'tool_result',
      tool_use_id: 'missing',
      content: 'failed',
      is_error: true,
    })
    expect(projection.results.get('missing')).toMatchObject({ is_error: true })
  })

  it('lets shared inline-result cards consume their paired result', () => {
    const projection = projectEngineProcessEntries([
      { key: 'call', segment: { kind: 'toolCall', id: 'mcp-1', name: 'mcp__browser__navigate', input: { url: 'https://example.com' } } },
      { key: 'result', segment: { kind: 'toolResult', callId: 'mcp-1', content: 'opened', isError: false } },
    ])

    expect(projection.blocks).toEqual([
      expect.objectContaining({ type: 'tool_use', id: 'mcp-1', name: 'mcp__browser__navigate' }),
    ])
    expect(projection.results.get('mcp-1')).toMatchObject({ content: 'opened' })
  })

  it('preserves non-object tool input under a neutral value field', () => {
    const projection = projectEngineProcessEntries([
      { key: 'call', segment: { kind: 'toolCall', id: 'tool-1', name: 'custom', input: ['a', 'b'] } },
    ])

    expect(projection.blocks[0]).toMatchObject({
      type: 'tool_use',
      id: 'tool-1',
      input: { value: ['a', 'b'] },
    })
  })

  it('preserves orchestration presentation and result image attachments', () => {
    const attachment = {
      asset: { session: { engine: 'codex' } as never, nativeId: 'result:tool-result:1' },
      mediaType: 'image/png',
      title: null,
    }
    const projection = projectEngineProcessEntries([
      {
        key: 'call',
        segment: {
          kind: 'toolCall',
          id: 'tool-1',
          name: 'js',
          input: { value: 'program' },
          title: '检查界面',
          presentation: 'orchestration',
        },
      },
      {
        key: 'result',
        segment: {
          kind: 'toolResult',
          callId: 'tool-1',
          content: 'Window: "Monet"',
          isError: false,
          attachments: [attachment],
        },
      },
    ])

    expect(projection.blocks[0]).toMatchObject({
      type: 'tool_use',
      name: 'js',
      _title: '检查界面',
      _presentation: 'orchestration',
    })
    expect(projection.blocks).toHaveLength(1)
    expect(projection.results.get('tool-1')?.content).toBe('Window: "Monet"')
    expect(projection.results.get('tool-1')?.attachments).toEqual([attachment])
  })

  it('keeps an orchestration result embedded when visible content splits the process blocks', () => {
    const blocks = buildEngineResponseBlocks([{
      id: 'record',
      segments: [
        {
          kind: 'toolCall' as const,
          id: 'tool-1',
          name: 'exec',
          input: { value: 'work()' },
          title: '识别窗口',
          presentation: 'orchestration' as const,
        },
        { kind: 'reasoning' as const, text: '检查结果', visibility: 'summary' as const },
        { kind: 'toolResult' as const, callId: 'tool-1', content: 'Window: Monet', isError: false },
      ],
    }])
    const processBlocks = blocks.filter(block => block.kind === 'process')
    const pairingEntries = processBlocks.flatMap(block => block.entries)
    const projections = processBlocks.map(block =>
      projectEngineProcessEntries(block.entries, pairingEntries),
    )

    expect(projections.flatMap(projection => projection.blocks)).toEqual([
      expect.objectContaining({
        type: 'tool_use',
        id: 'tool-1',
        _title: '识别窗口',
        _presentation: 'orchestration',
      }),
    ])
    expect(projections[1].blocks).toEqual([])
    expect(projections[1].results.get('tool-1')?.content).toBe('Window: Monet')
  })

  it('does not infer orchestration from a title alone', () => {
    const projection = projectEngineProcessEntries([
      {
        key: 'call',
        segment: {
          kind: 'toolCall',
          id: 'regular-title',
          name: 'custom',
          input: { title: '普通标题' },
          title: '普通标题',
        },
      },
      {
        key: 'result',
        segment: {
          kind: 'toolResult',
          callId: 'regular-title',
          content: 'visible',
          isError: false,
        },
      },
    ])

    expect(projection.blocks).toEqual([
      expect.objectContaining({ type: 'tool_use', id: 'regular-title' }),
      expect.objectContaining({ type: 'tool_result', tool_use_id: 'regular-title' }),
    ])
  })
})
