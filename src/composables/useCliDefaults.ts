import { computed, ref, type ComputedRef, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

export interface CliSettings {
  model: string | null
  effort_level: string | null
  ultracode: boolean
  fast_mode: boolean
  fast_mode_per_session_opt_in: boolean
  permission_mode: string | null
}

const EMPTY_CLI_SETTINGS: CliSettings = {
  model: null,
  effort_level: null,
  ultracode: false,
  fast_mode: false,
  fast_mode_per_session_opt_in: false,
  permission_mode: null,
}

const cliDefaultsByCwd = ref<Record<string, CliSettings>>({})

export function normalizeCliSettingsCwd(cwd?: string | null): string {
  const value = cwd?.trim()
  if (!value) return ''
  const slashNormalized = value.replace(/\\/g, '/')
  if (/^[A-Za-z]:\/+$/u.test(slashNormalized)) {
    return `${slashNormalized.slice(0, 2)}/`
  }
  const normalized = slashNormalized.replace(/\/+$/, '')
  return normalized || '/'
}

export function readCliDefaults(cwd?: string | null): CliSettings {
  return cliDefaultsByCwd.value[normalizeCliSettingsCwd(cwd)] ?? EMPTY_CLI_SETTINGS
}

export async function refreshCliDefaults(cwd?: string | null): Promise<CliSettings> {
  const key = normalizeCliSettingsCwd(cwd)
  try {
    const value = await invoke<CliSettings>('get_cli_settings', { cwd: key || null })
    cliDefaultsByCwd.value = { ...cliDefaultsByCwd.value, [key]: value }
    return value
  } catch (_) {
    return readCliDefaults(cwd)
  }
}

export function useCliDefaults(cwd?: Ref<string | null | undefined>): {
  cliDefaults: ComputedRef<CliSettings>
  refreshCliDefaults: () => Promise<CliSettings>
} {
  const cliDefaults = computed(() => readCliDefaults(cwd?.value))
  return {
    cliDefaults,
    refreshCliDefaults: () => refreshCliDefaults(cwd?.value),
  }
}
