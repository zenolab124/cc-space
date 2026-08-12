import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('workbench single-column layout', () => {
  it('fills the viewport without retaining a right-side resize overflow area', () => {
    const columns = source('../../src/components/workbench/WorkbenchColumns.vue')
    const sortable = source('../../src/components/workbench/SortableColumn.vue')

    expect(columns).toContain(':fill="activeTab.columns.length === 1"')
    expect(columns).toContain('v-if="activeTab.columns.length > 1"')
    expect(sortable).toContain("? { width: 'auto', flex: '1 1 0' }")
  })
})
