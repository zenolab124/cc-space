import { beforeEach, describe, expect, it } from 'vitest'
import {
  DEFAULT_SETTINGS,
  getSessionSettings,
} from '../../src/composables/useSessionSettings'

const KEY = 'monet:session-settings:test-session'
const values = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', {
  value: {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
    clear: () => values.clear(),
  },
  configurable: true,
})

describe('session permission inheritance migration', () => {
  beforeEach(() => localStorage.clear())

  it('新会话默认不生成权限覆盖', () => {
    expect(DEFAULT_SETTINGS.permissionMode).toBeNull()
    expect(getSessionSettings('test-session').permissionMode).toBeNull()
  })

  it('旧记录中的具体权限值按显式覆盖保留', () => {
    localStorage.setItem(KEY, JSON.stringify({ permissionMode: 'dontAsk' }))
    expect(getSessionSettings('test-session').permissionMode).toBe('dontAsk')
  })

  it('旧记录缺字段或值非法时迁移为跟随 CLI', () => {
    localStorage.setItem(KEY, JSON.stringify({ modelId: 'opus' }))
    expect(getSessionSettings('test-session').permissionMode).toBeNull()

    localStorage.setItem(KEY, JSON.stringify({ permissionMode: 'future-mode' }))
    expect(getSessionSettings('test-session').permissionMode).toBeNull()
  })
})
