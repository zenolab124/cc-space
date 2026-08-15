import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('工具调用分层折叠', () => {
  it('外层展开后先显示紧凑结果卡,点击结果卡才显示完整内容', () => {
    const group = source('../../src/components/ToolProcessGroup.vue')
    const list = source('../../src/components/ContentBlockList.vue')
    const items = source('../../src/components/ToolProcessItems.vue')
    const item = source('../../src/components/ToolProcessItem.vue')
    const resultCard = source('../../src/components/ToolResultPreviewCard.vue')

    expect(group).toContain('nested')
    expect(items).toContain('nested?: boolean')
    expect(items).toContain(':foldable="!props.nested"')
    expect(item).toContain('foldable?: boolean')
    expect(item).toContain('v-if="!orchestration && (!foldable || expanded)"')
    expect(list).toContain('v-else-if="isOrchestrationToolSegment(segment.tools)"')
    expect(item).toContain('v-if="foldable && canExpand"')
    expect(item).toContain('toolDisplayTitle(props.tool)')
    expect(item).toContain('const resultExpanded = ref(false)')
    expect(item).toContain('v-if="orchestration && result && (!foldable || expanded)"')
    expect(item).toContain('<ToolResultPreviewCard')
    expect(item).toContain(':expanded="resultExpanded"')
    expect(item).toContain('@toggle="toggleResult"')
    expect(item).toContain('if (!value) resultExpanded.value = false')
    expect(resultCard).toContain('tool-result-clamp-3')
    expect(resultCard).toContain("hasImages.value ? 'tool-result-clamp-2' : 'tool-result-clamp-3'")
    expect(resultCard.indexOf('class="tool-result-preview-images"')).toBeLessThan(
      resultCard.indexOf('class="tool-result-preview-text"'),
    )
    expect(resultCard).toContain("@click=\"emit('toggle', $event)\"")
    expect(item).toContain(':aria-expanded="expanded"')
    expect(item).toContain(':aria-controls="resultContentId"')
    expect(item).toContain('tool-fold-line:focus-visible')
    expect(item).toContain("font-size: 11px")
    expect(item).toContain("font-weight: 450")
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
