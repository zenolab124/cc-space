import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

function source(path: string): string {
  return readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
}

describe('用户提问吸顶开关', () => {
  it('默认开启并通过设置桥持久化', () => {
    const composable = source('../../src/composables/useStickyUserPrompt.ts')

    expect(composable).toContain("const DEFAULT_ENABLED = true")
    expect(composable).toContain("key: SETTING_KEY")
    expect(composable).toContain('writeSetting(SETTING_KEY, value)')
  })

  it('只控制独立悬浮层，不改变用户消息正文的文档流定位', () => {
    const turn = source('../../src/components/session/ConversationTurn.vue')
    const detail = source('../../src/components/SessionDetail.vue')
    const standardDetail = source('../../src/components/engine/EngineSessionDetail.vue')

    const userPosition = turn.indexOf('<div v-if="turn.user.visible">')
    const responsePosition = turn.indexOf('<AssistantResponseFrame')
    expect(userPosition).toBeGreaterThan(-1)
    expect(responsePosition).toBeGreaterThan(userPosition)
    expect(turn).not.toContain('position: sticky')
    expect(turn).not.toContain('stickyUserPromptEnabled')
    expect(detail).toContain('if (!stickyUserPromptEnabled.value)')
    expect(detail).toContain('stickyUserPromptEnabled && (stickyPending || stickyGroup?.user)')
    expect(detail).toContain('ref="pendingStickyRef"')
    expect(detail).not.toContain('user-msg-sticky')
    expect(detail).not.toContain(':hide-user=')
    expect(standardDetail).not.toContain(':hide-user=')
    expect(detail).not.toMatch(/\n\s+sticky-user(?:\s|>)/)
    expect(standardDetail).not.toMatch(/\n\s+sticky-user(?:\s|>)/)
    expect(standardDetail).toContain(':data-conversation-index="conversationGroups.length - 1"')
  })

  it('在外观设置中提供可持久化的全局开关', () => {
    const settings = source('../../src/views/SettingsView.vue')

    expect(settings).toContain('useStickyUserPrompt')
    expect(settings).toContain("$t('settings.stickyUserPrompt')")
    expect(settings).toContain(':aria-pressed="stickyUserPromptEnabled"')
    expect(settings).toContain('settings.stickyUserPromptOn')
    expect(settings).toContain('setStickyUserPrompt')
  })
})
