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

  it('同时控制普通 sticky、虚拟 overlay 和流式 pending 消息', () => {
    const turn = source('../../src/components/session/ConversationTurn.vue')
    const detail = source('../../src/components/SessionDetail.vue')
    const standardDetail = source('../../src/components/engine/EngineSessionDetail.vue')

    expect(turn).toContain('turn.user.sticky && stickyUserPromptEnabled')
    expect(turn).not.toContain('conversation-user-ghost')
    expect(detail).toContain('if (!stickyUserPromptEnabled.value)')
    expect(detail).toContain('stickyUserPromptEnabled && stickyGroup?.user')
    expect(detail).toContain("'user-msg-sticky': stickyUserPromptEnabled")
    expect(detail).not.toContain(':hide-user=')
    expect(standardDetail).not.toContain(':hide-user=')
    const nativeVirtualHistory = detail.slice(
      detail.indexOf('v-for="vitem in messageVirtualizer.getVirtualItems()"'),
      detail.indexOf('<!-- shouldVirtualize=false'),
    )
    expect(nativeVirtualHistory).not.toContain('sticky-user')
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
