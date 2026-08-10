import { describe, expect, it } from 'vitest'
import {
  buildEngineProcessActivities,
  buildEngineResponseBlocks,
  engineProcessActivityState,
  isEngineProcessSegment,
  isEngineThoughtSegment,
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

  it('pairs a neutral tool call and result into one frontend activity', () => {
    const activities = buildEngineProcessActivities([
      { key: 'call', segment: { kind: 'toolCall', id: 'tool-1', name: 'read', input: { path: 'a.ts' } } },
      { key: 'result', segment: { kind: 'toolResult', callId: 'tool-1', content: 'ok', isError: false } },
      { key: 'command', segment: { kind: 'commandExecution', id: 'cmd-1', command: 'pnpm test', cwd: null, output: 'pass', status: 'completed' } },
    ])

    expect(activities).toHaveLength(2)
    expect(activities[0]).toMatchObject({
      kind: 'tool',
      id: 'tool-1',
      call: { name: 'read' },
      result: { content: 'ok' },
    })
    expect(engineProcessActivityState(activities[0], false)).toBe('done')
    expect(engineProcessActivityState(activities[1], false)).toBe('done')
  })

  it('keeps an unmatched result visible instead of dropping adapter data', () => {
    const activities = buildEngineProcessActivities([
      { key: 'result', segment: { kind: 'toolResult', callId: 'missing', content: 'failed', isError: true } },
    ])

    expect(activities[0]).toMatchObject({ kind: 'tool', call: null, result: { isError: true } })
    expect(engineProcessActivityState(activities[0], false)).toBe('error')
  })
})
