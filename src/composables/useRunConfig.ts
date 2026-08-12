import { computed, watch, type ComputedRef } from 'vue'
import type { SessionSettings, EffortSetting, PermissionMode } from './useSessionSettings'
import { ADVISOR_MAIN_MODEL } from './useSessionSettings'
import {
  useChannels,
  OFFICIAL_CHANNEL_ID,
  type ChannelInfo,
} from './useChannels'
import {
  useCliDefaults,
  type CliSettings,
} from './useCliDefaults'

export type ValueSource = 'session' | 'channel' | 'cli' | 'advisor'

export interface ResolvedRunConfig {
  channelId: string | null
  display: {
    model: string | undefined
    modelSource: ValueSource
    effort: NonNullable<EffortSetting> | undefined
    effortSource: ValueSource
    fastMode: boolean
    fastModeSource: 'session' | 'cli'
    permissionMode: PermissionMode
    permissionModeSource: 'session' | 'cli'
  }
  launch: {
    model: string | undefined
    effort: NonNullable<EffortSetting> | undefined
    fastMode: boolean | undefined
    permissionMode: PermissionMode | null
  }
  channelDefaultModel: string | null
  channelDefaultEffort: NonNullable<EffortSetting> | null
  cliDefaultModel: string | null
  cliDefaultEffort: NonNullable<EffortSetting> | null
  cliDefaultFastMode: boolean
}

export interface RunConfigSnapshot {
  channels: readonly ChannelInfo[]
  defaultSessionChannel: string | null
  cliSettings: CliSettings
}

const VALID_EFFORT_VALUES = new Set(['low', 'medium', 'high', 'xhigh', 'max', 'ultracode'])
const VALID_PERMISSION_MODES = new Set<PermissionMode>([
  'default',
  'plan',
  'acceptEdits',
  'auto',
  'bypassPermissions',
  'dontAsk',
])

function sanitizeEffort(raw: string | null | undefined): NonNullable<EffortSetting> | null {
  if (!raw) return null
  const value = raw.trim().toLowerCase()
  return VALID_EFFORT_VALUES.has(value)
    ? value as NonNullable<EffortSetting>
    : null
}

function sanitizePermissionMode(raw: string | null | undefined): PermissionMode {
  return raw && VALID_PERMISSION_MODES.has(raw as PermissionMode)
    ? raw as PermissionMode
    : 'default'
}

function resolveChannelFromSnapshot(
  selected: string | null,
  snapshot: RunConfigSnapshot,
): string | null {
  if (selected === OFFICIAL_CHANNEL_ID) return null
  if (selected) return selected
  const defaultId = snapshot.defaultSessionChannel
  if (!defaultId) return null
  return snapshot.channels.find(channel => channel.id === defaultId)?.enabled
    ? defaultId
    : null
}

export function resolveRunConfig(
  settings: SessionSettings,
  snapshot: RunConfigSnapshot,
): ResolvedRunConfig {
  const channelId = resolveChannelFromSnapshot(settings.channelId, snapshot)
  const channel = snapshot.channels.find(item =>
    item.id === (channelId ?? OFFICIAL_CHANNEL_ID),
  ) ?? null
  const channelDefaultModel = channel?.defaultModel ?? null
  const channelDefaultEffort = sanitizeEffort(channel?.defaultEffort)
  const cliDefaultModel = snapshot.cliSettings.model ?? null
  const cliDefaultEffort = snapshot.cliSettings.ultracode
    ? 'ultracode'
    : sanitizeEffort(snapshot.cliSettings.effort_level)
  // 管理策略要求逐会话显式选择时，新进程不能沿用已保存的全局 fastMode 偏好。
  const cliDefaultFastMode = snapshot.cliSettings.fast_mode
    && !snapshot.cliSettings.fast_mode_per_session_opt_in

  let displayModel: string | undefined
  let launchModel: string | undefined
  let modelSource: ValueSource
  if (settings.advisor) {
    displayModel = ADVISOR_MAIN_MODEL
    launchModel = ADVISOR_MAIN_MODEL
    modelSource = 'advisor'
  } else if (settings.modelId) {
    displayModel = settings.modelId
    launchModel = settings.modelId
    modelSource = 'session'
  } else if (channelDefaultModel) {
    displayModel = channelDefaultModel
    launchModel = channelDefaultModel
    modelSource = 'channel'
  } else {
    displayModel = cliDefaultModel ?? undefined
    launchModel = undefined
    modelSource = 'cli'
  }

  let displayEffort: NonNullable<EffortSetting> | undefined
  let launchEffort: NonNullable<EffortSetting> | undefined
  let effortSource: ValueSource
  if (settings.effort) {
    displayEffort = settings.effort
    launchEffort = settings.effort
    effortSource = 'session'
  } else if (channelDefaultEffort) {
    displayEffort = channelDefaultEffort
    launchEffort = channelDefaultEffort
    effortSource = 'channel'
  } else {
    displayEffort = cliDefaultEffort ?? undefined
    launchEffort = undefined
    effortSource = 'cli'
  }

  const inheritedPermissionMode = sanitizePermissionMode(snapshot.cliSettings.permission_mode)
  const permissionMode = settings.permissionMode ?? inheritedPermissionMode
  const fastMode = settings.fastMode ?? cliDefaultFastMode

  return {
    channelId,
    display: {
      model: displayModel,
      modelSource,
      effort: displayEffort,
      effortSource,
      fastMode,
      fastModeSource: settings.fastMode === null ? 'cli' : 'session',
      permissionMode,
      permissionModeSource: settings.permissionMode ? 'session' : 'cli',
    },
    launch: {
      model: launchModel,
      effort: launchEffort,
      fastMode: settings.fastMode ?? undefined,
      permissionMode: settings.permissionMode,
    },
    channelDefaultModel,
    channelDefaultEffort,
    cliDefaultModel,
    cliDefaultEffort,
    cliDefaultFastMode,
  }
}

export function useRunConfig(
  settings: ComputedRef<SessionSettings>,
  cwd: ComputedRef<string | null | undefined>,
): { runConfig: ComputedRef<ResolvedRunConfig> } {
  const { channels, defaultSessionChannel } = useChannels()
  const { cliDefaults, refreshCliDefaults } = useCliDefaults(cwd)

  watch(cwd, () => {
    void refreshCliDefaults()
  }, { immediate: true })

  const runConfig = computed<ResolvedRunConfig>(() => resolveRunConfig(settings.value, {
    channels: channels.value,
    defaultSessionChannel: defaultSessionChannel.value,
    cliSettings: cliDefaults.value,
  }))
  return { runConfig }
}
