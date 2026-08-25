import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('工作台列头收缩优先级', () => {
  it('标题先占用剩余空间,搜索与关闭按钮不进入可裁切区', () => {
    const column = source('../../src/components/workbench/WorkbenchColumn.vue')
    const header = column.slice(column.indexOf('<!-- 列头 -->'), column.indexOf('<div class="flex-1 min-h-0">'))

    expect(column).toContain('class="workbench-column-optional-actions"')
    expect(column).toContain('class="workbench-column-close icon-btn')
    expect(header).toContain('class="workbench-column-engine-badge"')
    expect(header).toContain(':aria-label="engineName"')
    expect(header).toContain('i-simple-anthropic')
    expect(header).toContain('i-simple-openai')
    expect(column).toContain('width: 22px;')
    expect(column).toContain('padding: 3px;')
    expect(column).toContain('flex: 1 1 0;')
    expect(column).toContain('min-width: 60px;')
    expect(column).toContain('.workbench-column-optional-actions {')
    expect(column).toContain('overflow: hidden;')
    expect(column).toContain('.workbench-column-close { flex: 0 0 auto; }')
  })
})

describe('工作台列宽修饰键交互', () => {
  it('普通列与赛马列共用 Shift 批量调宽能力', () => {
    const resize = source('../../src/composables/useColumnResize.ts')
    const columns = source('../../src/components/workbench/WorkbenchColumns.vue')
    const race = source('../../src/components/workbench/RaceColumns.vue')

    expect(resize).toContain('const isShift = e.shiftKey')
    expect(resize).toContain('setMinColumnWidth(startMin + delta)')
    expect(resize).toContain('tab.columnSizes = tab.columnSizes.map(() => minColumnWidth.value)')
    expect(resize).toContain('return { dragging, shiftDragging, onDividerMouseDown }')
    expect(columns).toContain("{ 'divider-shift': shiftDragging }")
    expect(race).toContain("{ 'divider-shift': shiftDragging }")
  })
})
