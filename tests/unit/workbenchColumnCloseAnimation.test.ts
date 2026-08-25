import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('workbench column animation', () => {
  it('keeps state removal atomic while animating the rendered column out', () => {
    const workbench = source('../../src/composables/useWorkbench.ts')
    const columns = source('../../src/components/workbench/WorkbenchColumns.vue')
    const remove = workbench.slice(
      workbench.indexOf('function removeColumnAt'),
      workbench.indexOf('/** 收起列回左列'),
    )

    expect(remove).toContain('tab.columns.splice(removedIndex, 1)')
    expect(remove).toContain('removeColumnWidth(tab.columnSizes, removedIndex)')
    expect(remove).not.toContain('setTimeout')
    expect(remove).not.toContain('suppressColumnTransition.value')
    expect(columns).toContain('name="workbench-column"')
    expect(columns).toContain('@after-leave="onColumnAfterLeave"')
    expect(columns).toContain('.workbench-column-leave-to')
  })

  it('animates column width changes and respects reduced motion', () => {
    const columns = source('../../src/components/workbench/WorkbenchColumns.vue')
    const sortable = source('../../src/components/workbench/SortableColumn.vue')

    expect(sortable).toContain('flex-grow 220ms')
    expect(columns).toContain('opacity 140ms ease')
    expect(columns).toContain('@media (prefers-reduced-motion: reduce)')
    expect(sortable).toContain('@media (prefers-reduced-motion: reduce)')
  })

  it('grows an entering column from zero without introducing an early gap', () => {
    const columns = source('../../src/components/workbench/WorkbenchColumns.vue')

    expect(columns).toContain('.workbench-column-enter-active')
    expect(columns).toContain('.workbench-column-enter-from')
    expect(columns).toContain('margin-left: -10px')
    expect(columns).toMatch(/\.workbench-column-enter-from\s*\{[\s\S]*?width: 0 !important;/)
    expect(columns).toMatch(/\.workbench-column-enter-from\s*\{[\s\S]*?flex-grow: 0 !important;/)
  })

  it('realigns the latest focused column after its enter animation', () => {
    const columns = source('../../src/components/workbench/WorkbenchColumns.vue')

    expect(columns).toContain('@after-enter="onColumnAfterEnter"')
    expect(columns).toContain(':data-workbench-session-id="col.sessionId"')
    expect(columns).toContain('column.dataset.workbenchSessionId !== request.sessionId')
    expect(columns).toContain('scrollColumnIntoView(request.sessionId)')
  })

  it('defers single-column fill and the empty state until leaving DOM is gone', () => {
    const columns = source('../../src/components/workbench/WorkbenchColumns.vue')

    expect(columns).toContain('const renderedColumnCount = computed(')
    expect(columns).toContain(':fill="renderedColumnCount === 1"')
    expect(columns).toContain('v-if="renderedColumnCount === 0"')
    expect(columns).toContain("{ flush: 'sync' }")
  })
})
