import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const source = readFileSync(
  fileURLToPath(new URL('../../src/components/workbench/WorkbenchTabs.vue', import.meta.url)),
  'utf8',
)

describe('工作台 Tab 重命名输入框', () => {
  it('只在进入重命名后初始化一次选区,不在函数 ref 更新时重复 select', () => {
    const captureStart = source.indexOf('function captureEditInput')
    const captureEnd = source.indexOf('\n}', captureStart)
    const captureBody = source.slice(captureStart, captureEnd)

    expect(source).toContain("import { nextTick, ref } from 'vue'")
    expect(source).toContain('void nextTick(() => {')
    expect(source).toContain('editInputElement.value?.select()')
    expect(source).toContain(':ref="captureEditInput"')
    expect(captureBody).not.toContain('.focus()')
    expect(captureBody).not.toContain('.select()')
  })
})
