import { describe, it, expect } from 'vitest'
import { buildFileLedger, normalizeLedgerPath } from '../../src/composables/useFileLedger'
import type { SessionRecord } from '../../src/types'
import type { StreamingTurn } from '../../src/composables/useStreaming'

// —— 构造器:最小合法 assistant record / 流式 turn ——
let uid = 0
function toolUse(name: string, input: Record<string, unknown>) {
  return { type: 'tool_use' as const, id: `toolu_${++uid}`, name, input }
}
function assistantRecord(blocks: unknown[], ts: string, opts: { sidechain?: boolean } = {}): SessionRecord {
  return {
    type: 'assistant', uuid: `u-${++uid}`, parent_uuid: null, session_id: 's', timestamp: ts,
    cwd: null, version: null, git_branch: null, is_sidechain: opts.sidechain ?? false,
    message: { id: `msg-${uid}`, message_type: null, role: 'assistant', content: blocks as never, model: 'm', stop_reason: null, usage: null },
  } as SessionRecord
}
function turn(blocks: unknown[], mid = `live-${++uid}`): StreamingTurn {
  return { messageId: mid, content: blocks as never, live: true }
}

describe('入账规则', () => {
  it('Edit/Write/Read 入账,Bash/Glob/Grep 不入账;modified 粘性', () => {
    const recs = [assistantRecord([
      toolUse('Read', { file_path: '/a/f.ts' }),
      toolUse('Edit', { file_path: '/a/f.ts', old_string: 'x', new_string: 'y', replace_all: false }),
      toolUse('Bash', { command: 'sed -i "" s/x/y/ /a/other.ts' }),
      toolUse('Glob', { pattern: '**/*.ts' }),
      toolUse('Grep', { pattern: 'x', path: '/a' }),
    ], '2026-07-16T10:00:00Z')]
    const led = buildFileLedger(recs, [])
    expect(led).toHaveLength(1)
    expect(led[0].path).toBe('/a/f.ts')
    expect(led[0].modified).toBe(true)
    expect(led[0].readCount).toBe(1)
    expect(led[0].editCount).toBe(1)
    expect(led[0].ops.map(o => o.tool)).toEqual(['Read', 'Edit'])
  })

  it('同文件 2 读 3 改聚合为单条目、ops 时序保持', () => {
    const recs = [
      assistantRecord([toolUse('Read', { file_path: '/p/x.vue' })], '2026-07-16T10:00:00Z'),
      assistantRecord([
        toolUse('Edit', { file_path: '/p/x.vue', old_string: 'a', new_string: 'b' }),
        toolUse('Edit', { file_path: '/p/x.vue', old_string: 'c', new_string: 'd' }),
      ], '2026-07-16T10:01:00Z'),
      assistantRecord([toolUse('Read', { file_path: '/p/x.vue' }), toolUse('Write', { file_path: '/p/x.vue', content: 'l1\nl2' })], '2026-07-16T10:02:00Z'),
    ]
    const [e] = buildFileLedger(recs, [])
    expect(e.ops).toHaveLength(5)
    expect(e.readCount).toBe(2)
    expect(e.editCount).toBe(3)
    expect(e.lastTs).toBe('2026-07-16T10:02:00Z')
  })

  it('首个操作为 Write → createdByWrite;先 Read 后 Write 则不算新建', () => {
    const a = buildFileLedger([assistantRecord([toolUse('Write', { file_path: '/n/new.ts', content: 'x' })], '2026-07-16T10:00:00Z')], [])
    expect(a[0].createdByWrite).toBe(true)
    const b = buildFileLedger([assistantRecord([
      toolUse('Read', { file_path: '/n/old.ts' }),
      toolUse('Write', { file_path: '/n/old.ts', content: 'x' }),
    ], '2026-07-16T10:00:00Z')], [])
    expect(b[0].createdByWrite).toBe(false)
  })

  it('NotebookEdit 走 notebook_path 且计为修改', () => {
    const led = buildFileLedger([assistantRecord([toolUse('NotebookEdit', { notebook_path: '/nb/a.ipynb', new_source: 'x' })], '2026-07-16T10:00:00Z')], [])
    expect(led[0].path).toBe('/nb/a.ipynb')
    expect(led[0].modified).toBe(true)
  })
})

describe('容错与归一化', () => {
  it('畸形 input(缺 file_path / 非字符串 / 非对象)静默跳过', () => {
    const recs = [assistantRecord([
      toolUse('Edit', {}),
      toolUse('Edit', { file_path: 42 }),
      { type: 'tool_use', id: 't-x', name: 'Edit', input: null },
      toolUse('Edit', { file_path: '/ok.ts', old_string: 'a', new_string: 'b' }),
    ], '2026-07-16T10:00:00Z')]
    const led = buildFileLedger(recs, [])
    expect(led).toHaveLength(1)
    expect(led[0].path).toBe('/ok.ts')
  })

  it('路径写法差异聚为同一条目', () => {
    expect(normalizeLedgerPath('/a//b/./c/')).toBe('/a/b/c')
    const recs = [assistantRecord([
      toolUse('Read', { file_path: '/a/b/c.ts' }),
      toolUse('Edit', { file_path: '/a//b/./c.ts', old_string: 'x', new_string: 'y' }),
    ], '2026-07-16T10:00:00Z')]
    expect(buildFileLedger(recs, [])).toHaveLength(1)
  })

  it('sidechain 记录剔除', () => {
    const led = buildFileLedger([assistantRecord([toolUse('Edit', { file_path: '/s.ts', old_string: 'a', new_string: 'b' })], '2026-07-16T10:00:00Z', { sidechain: true })], [])
    expect(led).toHaveLength(0)
  })
})

describe('流式与去重', () => {
  it('流式 turn 的操作实时并入且排最前', () => {
    const recs = [assistantRecord([toolUse('Edit', { file_path: '/old.ts', old_string: 'a', new_string: 'b' })], '2026-07-16T10:00:00Z')]
    const t = turn([toolUse('Write', { file_path: '/live.ts', content: 'x' })])
    const led = buildFileLedger(recs, [t])
    expect(led).toHaveLength(2)
    expect(led[0].path).toBe('/live.ts')
  })

  it('同一 tool_use 双在 records 与 turns 时按 id 去重', () => {
    const shared = toolUse('Edit', { file_path: '/dup.ts', old_string: 'a', new_string: 'b' })
    const led = buildFileLedger(
      [assistantRecord([shared], '2026-07-16T10:00:00Z')],
      [turn([shared])],
    )
    expect(led).toHaveLength(1)
    expect(led[0].ops).toHaveLength(1)
  })
})

describe('NFR-001 性能基准', () => {
  it('3000 条 records 全量推导 ≤ 10ms', () => {
    const recs: SessionRecord[] = []
    for (let i = 0; i < 3000; i++) {
      recs.push(assistantRecord([
        toolUse(i % 3 === 0 ? 'Edit' : i % 3 === 1 ? 'Read' : 'Bash',
          i % 3 === 0
            ? { file_path: `/proj/file${i % 40}.ts`, old_string: 'a'.repeat(200), new_string: 'b'.repeat(220) }
            : i % 3 === 1
              ? { file_path: `/proj/file${i % 40}.ts` }
              : { command: 'echo x' }),
        { type: 'text', text: 'lorem '.repeat(50) },
      ], `2026-07-16T10:${String(i % 60).padStart(2, '0')}:00Z`))
    }
    // 先预热 JIT，再取 5 次中位数；单次冷启动墙钟会把共享 CI runner 的
    // 调度/JIT 抖动误判为算法退化，无法稳定表达这条性能契约。
    buildFileLedger(recs, [])
    const samples: number[] = []
    let led = buildFileLedger(recs, [])
    for (let i = 0; i < 5; i++) {
      const t0 = performance.now()
      led = buildFileLedger(recs, [])
      samples.push(performance.now() - t0)
    }
    samples.sort((a, b) => a - b)
    const median = samples[Math.floor(samples.length / 2)]
    expect(led.length).toBe(40)
    expect(median).toBeLessThanOrEqual(10)
  })
})
