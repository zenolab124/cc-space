import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('引擎选择器响应式高度', () => {
  it('滚动容器不参与居中布局，纵向卡片行保持内容高度', () => {
    const panel = source('../../src/components/workbench/EngineChoicePanel.vue')

    expect(panel).not.toContain('px-5 py-8 flex items-center justify-center')
    expect(panel).toContain('class="engine-picker-content w-full max-w-2xl text-center"')
    expect(panel).toContain('min-height: 100%;')
    expect(panel).toContain('grid-auto-rows: max-content;')
    expect(panel).toContain('align-content: start;')
  })
})
