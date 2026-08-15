import { describe, expect, it } from 'vitest'
import type { ChannelInfo } from '../../src/composables/useChannels'
import type { SessionSettings } from '../../src/composables/useSessionSettings'
import {
  resolveRunConfig,
  type RunConfigSnapshot,
} from '../../src/composables/useRunConfig'

const official: ChannelInfo = {
  id: 'official',
  name: 'Official',
  note: null,
  baseUrl: null,
  authTokenMasked: null,
  extraEnvKeys: [],
  valid: true,
  enabled: true,
  protocol: 'anthropic',
  scope: 'user',
  agentModel: null,
  availableModels: [],
  modelEnv: {},
  defaultModel: null,
  defaultEffort: null,
}

function settings(patch: Partial<SessionSettings> = {}): SessionSettings {
  return {
    modelId: null,
    effort: null,
    fastMode: null,
    channelId: null,
    channelMarks: [],
    advisor: false,
    chrome: false,
    extraArgs: '',
    permissionMode: null,
    ...patch,
  }
}

function snapshot(patch: Partial<RunConfigSnapshot> = {}): RunConfigSnapshot {
  return {
    channels: [official],
    defaultSessionChannel: null,
    defaultSessionModel: null,
    defaultSessionEffort: null,
    cliSettings: {
      model: 'cli-model',
      effort_level: 'high',
      ultracode: false,
      fast_mode: false,
      fast_mode_per_session_opt_in: false,
      permission_mode: 'plan',
    },
    ...patch,
  }
}

describe('resolveRunConfig', () => {
  it('CLI 有效值只进入 display，不形成启动覆盖', () => {
    const result = resolveRunConfig(settings(), snapshot())
    expect(result.display).toMatchObject({
      model: 'cli-model',
      modelSource: 'cli',
      effort: 'high',
      effortSource: 'cli',
      fastMode: false,
      fastModeSource: 'cli',
      permissionMode: 'plan',
      permissionModeSource: 'cli',
    })
    expect(result.launch).toEqual({
      model: undefined,
      effort: undefined,
      fastMode: undefined,
      permissionMode: null,
    })
    expect(result.cliDefaultModel).toBe('cli-model')
    expect(result.cliDefaultEffort).toBe('high')
    expect(result.cliDefaultFastMode).toBe(false)
  })

  it('会话显式选择同时进入 display 与 launch', () => {
    const result = resolveRunConfig(settings({
      modelId: 'session-model',
      effort: 'xhigh',
      fastMode: true,
      permissionMode: 'dontAsk',
    }), snapshot())
    expect(result.display).toMatchObject({
      model: 'session-model',
      modelSource: 'session',
      effort: 'xhigh',
      effortSource: 'session',
      permissionMode: 'dontAsk',
      permissionModeSource: 'session',
    })
    expect(result.launch).toEqual({
      model: 'session-model',
      effort: 'xhigh',
      fastMode: true,
      permissionMode: 'dontAsk',
    })
  })

  it('会话引擎默认形成启动覆盖，ultracode 保留虚拟 effort 值', () => {
    const channel: ChannelInfo = {
      ...official,
      id: 'proxy',
      name: 'Proxy',
      defaultModel: 'vendor-model',
      defaultEffort: 'ultracode',
    }
    const result = resolveRunConfig(
      settings({ channelId: 'proxy' }),
      snapshot({
        channels: [official, channel],
        defaultSessionModel: 'vendor-model',
        defaultSessionEffort: 'ultracode',
      }),
    )
    expect(result.channelId).toBe('proxy')
    expect(result.launch).toMatchObject({
      model: 'vendor-model',
      effort: 'ultracode',
    })
  })

  it('顾问模式锁定主模型并覆盖其他模型来源', () => {
    const result = resolveRunConfig(
      settings({ advisor: true, modelId: 'ignored' }),
      snapshot(),
    )
    expect(result.display.modelSource).toBe('advisor')
    expect(result.launch.model).toBe('claude-sonnet-4-6')
  })

  it('未知 CLI 权限值只影响回显降级，不产生覆盖', () => {
    const result = resolveRunConfig(settings(), snapshot({
      cliSettings: {
        model: null,
        effort_level: null,
        ultracode: false,
        fast_mode: false,
        fast_mode_per_session_opt_in: false,
        permission_mode: 'future-mode',
      },
    }))
    expect(result.display.permissionMode).toBe('default')
    expect(result.launch.permissionMode).toBeNull()
  })

  it('逐会话 opt-in 策略不把已保存的快速模式带进新进程', () => {
    const result = resolveRunConfig(settings(), snapshot({
      cliSettings: {
        model: null,
        effort_level: null,
        ultracode: false,
        fast_mode: true,
        fast_mode_per_session_opt_in: true,
        permission_mode: null,
      },
    }))
    expect(result.display.fastMode).toBe(false)
    expect(result.launch.fastMode).toBeUndefined()
  })
})
