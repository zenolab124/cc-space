import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import {
  normalizeCliSettingsCwd,
  readCliDefaults,
  refreshCliDefaults,
} from '../../src/composables/useCliDefaults'

describe('useCliDefaults cwd isolation', () => {
  beforeEach(() => {
    invoke.mockReset()
  })

  it('规范化分隔符与尾随斜杠', () => {
    expect(normalizeCliSettingsCwd(' C:\\repo\\app\\ ')).toBe('C:/repo/app')
    expect(normalizeCliSettingsCwd('C:\\')).toBe('C:/')
    expect(normalizeCliSettingsCwd('/repo/app///')).toBe('/repo/app')
    expect(normalizeCliSettingsCwd(null)).toBe('')
  })

  it('不同 cwd 使用独立摘要', async () => {
    invoke
      .mockResolvedValueOnce({ model: 'project-a', effort_level: null, ultracode: false, fast_mode: false, fast_mode_per_session_opt_in: false, permission_mode: null })
      .mockResolvedValueOnce({ model: 'project-b', effort_level: 'max', ultracode: true, fast_mode: true, fast_mode_per_session_opt_in: false, permission_mode: 'plan' })

    await refreshCliDefaults('/repo/a')
    await refreshCliDefaults('/repo/b')

    expect(readCliDefaults('/repo/a').model).toBe('project-a')
    expect(readCliDefaults('/repo/b')).toMatchObject({ model: 'project-b', ultracode: true })
  })

  it('读取失败只保留同 cwd 旧值，不借用其他项目', async () => {
    invoke.mockResolvedValueOnce({
      model: 'persisted',
      effort_level: null,
      ultracode: false,
      fast_mode: false,
      fast_mode_per_session_opt_in: false,
      permission_mode: null,
    })
    await refreshCliDefaults('/repo/persisted')
    invoke.mockRejectedValue(new Error('offline'))

    expect(await refreshCliDefaults('/repo/persisted')).toEqual(readCliDefaults('/repo/persisted'))
    expect(await refreshCliDefaults('/repo/new')).toEqual({
      model: null,
      effort_level: null,
      ultracode: false,
      fast_mode: false,
      fast_mode_per_session_opt_in: false,
      permission_mode: null,
    })
  })
})
