import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('工具调用分层折叠', () => {
  it('分组模式按回合压成最新标题,图片移入右侧堆叠', () => {
    const group = source('../../src/components/ToolProcessGroup.vue')
    const list = source('../../src/components/ContentBlockList.vue')
    const items = source('../../src/components/ToolProcessItems.vue')
    const item = source('../../src/components/ToolProcessItem.vue')
    const resultImages = source('../../src/components/ToolResultImages.vue')
    const processTrack = source('../../src/components/ToolProcessTrack.vue')
    const imageStack = source('../../src/components/ToolImageStack.vue')
    const nativeTurn = source('../../src/components/MessageGroup.vue')
    const standardTurn = source('../../src/components/engine/EngineConversationGroup.vue')
    const resultCard = source('../../src/components/ToolResultPreviewCard.vue')
    const inlineImage = source('../../src/components/blocks/BlockImage.vue')
    const assetImage = source('../../src/components/engine/EngineAssetImage.vue')

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
    expect(item).toContain('<ToolResultImages')
    expect(item).toContain('v-if="showImages && imageCount > 0"')
    expect(item.indexOf('<ToolResultImages')).toBeLessThan(
      item.indexOf('v-if="!orchestration && (!foldable || expanded)"'),
    )
    expect(resultImages).toContain('flex-wrap: wrap')
    expect(resultImages).toContain('flex: 0 1 240px')
    expect(resultImages).toContain('width: 240px')
    expect(resultImages).toContain('height: 160px')
    expect(resultImages).toContain('<BlockImage')
    expect(resultImages).toContain('<EngineAssetImage')
    expect(inlineImage).toContain('@click="expanded = true"')
    expect(assetImage).toContain('@click="openLightbox"')
    expect(group).toContain('latestOnly?: boolean')
    expect(group).toContain('toolDisplayTitle(latestTool)')
    expect(group).toContain(':show-images="showImages"')
    expect(processTrack).toContain('latest-only')
    expect(processTrack).toContain(':show-images="false"')
    expect(processTrack).toContain('<ToolImageStack')
    expect(imageStack).toContain('class="tool-image-stack-card"')
    expect(imageStack).toContain('<BlockImage')
    expect(imageStack).toContain('<EngineAssetImage')
    expect(imageStack).toContain('@container (max-width: 420px)')
    expect(nativeTurn).toContain('<ToolProcessTrack')
    expect(standardTurn).toContain('<ToolProcessTrack')
    expect(resultCard).toContain('tool-result-clamp-3')
    expect(resultCard).not.toContain('tool-result-preview-images')
    expect(resultCard).not.toContain('tool-result-clamp-2')
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
