import { describe, expect, it } from 'vitest'
import {
  buildTextDiff,
  calculateTextDiffStats,
  foldTextDiffRows,
} from '@/utils/textDiff'

describe('text diff model', () => {
  it('builds numbered rows and word-level chunks from old and new text', () => {
    const model = buildTextDiff({
      oldText: 'alpha beta\ngamma\nsame\n',
      newText: 'alpha brave\ngamma\nadded\nsame\n',
    })

    expect(model.stats).toEqual({ additions: 2, deletions: 1 })
    const deleted = model.rows.find(row => row.kind === 'delete')
    const added = model.rows.find(row => row.kind === 'add' && row.newNumber === 1)
    expect(deleted).toMatchObject({ oldNumber: 1, newNumber: null })
    expect(deleted?.chunks).toContainEqual({ kind: 'delete', text: 'beta' })
    expect(added?.chunks).toContainEqual({ kind: 'add', text: 'brave' })
  })

  it('parses neutral unified diffs without treating headers as content', () => {
    const unifiedDiff = [
      '--- a/example.txt',
      '+++ b/example.txt',
      '@@ -10,2 +10,2 @@',
      '-old value',
      '+new value',
      ' unchanged',
      '',
    ].join('\n')
    const model = buildTextDiff({ unifiedDiff })

    expect(model.stats).toEqual({ additions: 1, deletions: 1 })
    expect(calculateTextDiffStats({ unifiedDiff })).toEqual(model.stats)
    expect(model.rows).toMatchObject([
      { kind: 'delete', oldNumber: 10, newNumber: null, text: 'old value' },
      { kind: 'add', oldNumber: null, newNumber: 10, text: 'new value' },
      { kind: 'context', oldNumber: 11, newNumber: 11, text: 'unchanged' },
    ])
  })

  it('folds long unchanged regions while keeping context around changes', () => {
    const oldText = Array.from({ length: 20 }, (_, index) => `line ${index + 1}`).join('\n')
    const newText = oldText.replace('line 10', 'changed line 10')
    const rows = buildTextDiff({ oldText, newText }).rows
    const folded = foldTextDiffRows(rows, 2)

    expect(folded.filter(row => row.kind === 'fold')).toHaveLength(2)
    expect(folded.some(row => row.kind === 'delete')).toBe(true)
    expect(folded.some(row => row.kind === 'add')).toBe(true)
  })
})
