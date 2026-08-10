import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('工具组单层折叠', () => {
  it('分组模式由外层统一折叠,工具项不再创建第二层折叠', () => {
    const group = source('../../src/components/ToolProcessGroup.vue')
    const items = source('../../src/components/ToolProcessItems.vue')
    const item = source('../../src/components/ToolProcessItem.vue')

    expect(group).toContain('nested')
    expect(items).toContain('nested?: boolean')
    expect(items).toContain(':foldable="!props.nested"')
    expect(item).toContain('foldable?: boolean')
    expect(item).toContain('v-if="!orchestration && (!foldable || expanded)"')
  })
})

describe('已思考标签', () => {
  it('所有内置语言都不展示加密说明括号', () => {
    const localeDir = fileURLToPath(new URL('../../src/locales/', import.meta.url))
    for (const name of readdirSync(localeDir).filter(name => name.endsWith('.json'))) {
      const locale = JSON.parse(readFileSync(`${localeDir}/${name}`, 'utf8')) as {
        block?: { thinkingRedacted?: string }
      }
      expect(locale.block?.thinkingRedacted, name).not.toMatch(/[()（）]/)
    }
  })
})
