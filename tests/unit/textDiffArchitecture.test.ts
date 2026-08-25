import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()

function source(relativePath: string): string {
  return fs.readFileSync(path.join(root, relativePath), 'utf8')
}

describe('shared text diff architecture', () => {
  it('uses one viewer for tool edits and the file ledger', () => {
    const toolEdit = source('src/components/blocks/tools/ToolEdit.vue')
    const ledger = source('src/components/FileLedgerTimeline.vue')

    expect(toolEdit).toContain("from '@/components/diff/TextDiffViewer.vue'")
    expect(toolEdit).toContain(':unified-diff="unifiedDiff"')
    expect(ledger).toContain("from '@/components/diff/TextDiffViewer.vue'")
    expect(ledger).toContain(':old-text="op.oldString"')
  })

  it('keeps standard-engine file changes in neutral unified-diff form', () => {
    const projection = source('src/engines/processGroups.ts')

    expect(projection).toContain('unified_diff: change.diff')
    expect(projection).not.toContain('new_string: change.diff')
  })
})
