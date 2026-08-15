import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('用户提问吸顶开关', () => {
  it('两个引擎默认开启并分别通过设置桥持久化', () => {
    const composable = source('../../src/composables/useStickyUserPrompt.ts')

    expect(composable).toContain("const DEFAULT_ENABLED = true")
    expect(composable).toContain("'claude-code': enabled")
    expect(composable).toContain('codex: enabled')
    expect(composable).toContain("const SETTING_KEY = 'stickyUserPromptByEngine'")
    expect(composable).toContain("key: SETTING_KEY")
    expect(composable).toContain('writeSetting(SETTING_KEY, value)')
    expect(composable).toContain('setStickyUserPromptFor(engineId: SessionReadingEngineId')
  })

  it('保留虚拟化并用真实卡片边界驱动悬浮层定位', () => {
    const turn = source('../../src/components/session/ConversationTurn.vue')
    const detail = source('../../src/components/SessionDetail.vue')
    const standardDetail = source('../../src/components/engine/EngineSessionDetail.vue')

    const userPosition = turn.indexOf('v-if="turn.user.visible"')
    const responsePosition = turn.indexOf('<AssistantResponseFrame')
    expect(userPosition).toBeGreaterThan(-1)
    expect(responsePosition).toBeGreaterThan(userPosition)
    expect(turn).not.toContain('position: sticky')
    expect(detail).toContain('const shouldVirtualize = computed(() => renderGroups.value.length')
    expect(standardDetail).toContain('const shouldVirtualize = computed(() => historicalGroups.value.length')
    expect(detail).toContain('ref="stickyOverlayElement"')
    expect(detail).toContain('ref="stickySurfaceElement"')
    expect(detail).toContain('const STICKY_CARD_GAP = 16')
    expect(detail).toContain('getBoundingClientRect().top <= restingTop + 0.5')
    expect(detail).toContain('nextCard.getBoundingClientRect().top - STICKY_CARD_GAP - restingBottom')
    expect(detail).toContain("'sticky-source-hidden'")
    expect(standardDetail).toContain(':data-conversation-index="conversationGroups.length - 1"')
    expect(standardDetail).toContain('ref="stickyOverlayElement"')
    expect(standardDetail).toContain('class="engine-sticky-user-surface"')
    expect(standardDetail).toContain('const STICKY_CARD_GAP = 16')
    expect(standardDetail).toContain('getBoundingClientRect().top <= restingTop + 0.5')
    expect(standardDetail).toContain('nextCard.getBoundingClientRect().top - STICKY_CARD_GAP - restingBottom')
    expect(standardDetail).toContain("'sticky-source-hidden'")
    expect(detail).not.toContain('surfaceBounds.height')
    expect(standardDetail).not.toContain('surfaceBounds.height')
  })

  it('在外观设置中按引擎提供独立开关', () => {
    const settings = source('../../src/views/SettingsView.vue')

    expect(settings).toContain('useStickyUserPrompt')
    expect(settings).toContain("$t('settings.stickyUserPrompt')")
    expect(settings).toContain(':aria-pressed="selectedStickyUserPrompt"')
    expect(settings).toContain('settings.stickyUserPromptOn')
    expect(settings).toContain('setStickyUserPromptFor(selectedReadingEngine.value')
    expect(settings).toContain('role="tablist"')
  })
})
