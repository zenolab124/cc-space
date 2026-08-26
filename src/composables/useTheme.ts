import { computed, ref, watch, type CSSProperties } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { isWindows } from './usePlatform'
import i18n from '../locales'
import {
  BUILTIN_THEMES,
  COMMUNITY_THEMES,
  themeMetaFromDefinition,
  type ThemeMeta,
} from './themeRegistry'
import type { ThemeDefinition, ThemeLibrary } from '@/types/theme'
import { readMigratedStorage } from '../utils/storageMigrate'
import { bridgeSetting, writeSetting } from '../utils/settingBridge'

const STORAGE_KEY = 'monet-theme'
const LEGACY_STORAGE_KEY = 'cc-space-theme'
const SETTING_KEY = 'theme'

export interface ThemeConfig {
  version: 2
  lightTheme: string
  darkTheme: string
  mode: 'system' | 'light' | 'dark'
}

const defaultConfig = (): ThemeConfig => ({
  version: 2,
  lightTheme: 'paper',
  darkTheme: 'ink',
  mode: 'system',
})

function parseThemeValue(value: unknown): ThemeConfig | null {
  if (value === 'system' || value === 'light' || value === 'dark') {
    return { ...defaultConfig(), mode: value }
  }
  if (typeof value !== 'object' || value === null || (value as { version?: unknown }).version !== 2) return null
  const input = value as { lightTheme?: unknown; darkTheme?: unknown; mode?: unknown }
  return {
    version: 2,
    lightTheme: typeof input.lightTheme === 'string' && input.lightTheme ? input.lightTheme : 'paper',
    darkTheme: typeof input.darkTheme === 'string' && input.darkTheme ? input.darkTheme : 'ink',
    mode: input.mode === 'light' || input.mode === 'dark' ? input.mode : 'system',
  }
}

function loadConfig(): ThemeConfig {
  const raw = readMigratedStorage(STORAGE_KEY, LEGACY_STORAGE_KEY)
  if (!raw) return defaultConfig()
  let value: unknown = raw
  try { value = JSON.parse(raw) } catch {}
  return parseThemeValue(value) ?? defaultConfig()
}

const config = ref<ThemeConfig>(loadConfig())
const localThemes = ref<ThemeDefinition[]>([])
const pendingPreviews = ref<ThemeLibrary['previews']>([])
const invalidThemeEntries = ref<string[]>([])
const themeLibraryLoading = ref(false)

bridgeSetting({
  key: SETTING_KEY,
  uplift: () => (localStorage.getItem(STORAGE_KEY) !== null ? config.value : undefined),
  apply: value => {
    const parsed = parseThemeValue(value)
    if (parsed && JSON.stringify(parsed) !== JSON.stringify(config.value)) config.value = parsed
  },
})

const prefersDark = ref(window.matchMedia('(prefers-color-scheme: dark)').matches)
window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', event => {
  prefersDark.value = event.matches
})

const themes = computed<ThemeMeta[]>(() => [
  ...BUILTIN_THEMES,
  ...COMMUNITY_THEMES,
  ...localThemes.value.map(theme => themeMetaFromDefinition(theme, 'custom')),
])
const lightThemes = computed(() => themes.value.filter(theme => theme.appearance === 'light'))
const darkThemes = computed(() => themes.value.filter(theme => theme.appearance === 'dark'))

function themeForSlot(id: string, appearance: 'light' | 'dark'): ThemeMeta {
  return themes.value.find(theme => theme.id === id && theme.appearance === appearance)
    ?? BUILTIN_THEMES.find(theme => theme.appearance === appearance)!
}

const activeAppearance = computed<'light' | 'dark'>(() => (
  config.value.mode === 'dark' || (config.value.mode === 'system' && prefersDark.value) ? 'dark' : 'light'
))
const activeTheme = computed(() => themeForSlot(
  activeAppearance.value === 'dark' ? config.value.darkTheme : config.value.lightTheme,
  activeAppearance.value,
))

const COLOR_VARIABLES: Record<keyof ThemeDefinition['colors'], string> = {
  background: '--background',
  foreground: '--foreground',
  card: '--card',
  cardForeground: '--card-foreground',
  popover: '--popover',
  popoverForeground: '--popover-foreground',
  primary: '--primary',
  primaryForeground: '--primary-foreground',
  secondary: '--secondary',
  secondaryForeground: '--secondary-foreground',
  muted: '--muted',
  mutedForeground: '--muted-foreground',
  accent: '--accent',
  accentForeground: '--accent-foreground',
  destructive: '--destructive',
  destructiveForeground: '--destructive-foreground',
  border: '--border',
  input: '--input',
  ring: '--ring',
  claude: '--claude',
  codex: '--codex',
  tag: '--tag',
  tagForeground: '--tag-foreground',
  visualBorder: '--hv-border',
  visualWarm: '--hv-warm',
  visualCool: '--hv-cool',
  visualRed: '--hv-red',
  visualGreen: '--hv-green',
}

function hexRgba(hex: string, opacity: number): string {
  const red = Number.parseInt(hex.slice(1, 3), 16)
  const green = Number.parseInt(hex.slice(3, 5), 16)
  const blue = Number.parseInt(hex.slice(5, 7), 16)
  return `rgba(${red}, ${green}, ${blue}, ${opacity})`
}

export function themeCssVariables(theme: ThemeDefinition): CSSProperties {
  const variables: Record<string, string> = {}
  for (const [token, variable] of Object.entries(COLOR_VARIABLES) as Array<[keyof ThemeDefinition['colors'], string]>) {
    variables[variable] = theme.colors[token]
  }
  const shadow = theme.metrics.shadow
  variables['--radius'] = `${theme.metrics.radius}px`
  variables['--theme-font-scale'] = String(theme.metrics.fontScale)
  variables['--theme-line-height'] = String(theme.metrics.lineHeight)
  variables['--shadow-paper'] = `0 ${shadow.y}px ${shadow.blur}px ${hexRgba(shadow.color, shadow.opacity)}`
  variables['--shadow-paper-lifted'] = `0 ${shadow.y + 4}px ${shadow.blur + 12}px ${hexRgba(shadow.color, Math.min(0.75, shadow.opacity * 1.35))}`
  variables['--theme-atmosphere-tint'] = theme.atmosphere.tint
  variables['--theme-atmosphere-noise'] = String(theme.atmosphere.noise)
  variables['--theme-atmosphere-vignette'] = String(theme.atmosphere.vignette)
  variables['--run-running'] = theme.colors.primary
  variables['--run-exited'] = theme.colors.accent
  variables['--run-exited-wash'] = theme.colors.visualWarm
  variables['--run-crashed'] = theme.colors.destructive
  variables['--run-starting'] = theme.colors.mutedForeground
  variables['--ansi-green'] = theme.colors.primary
  variables['--ansi-red'] = theme.colors.destructive
  variables['--ansi-yellow'] = theme.colors.accent
  variables['--ansi-blue'] = theme.colors.codex
  variables['--ansi-magenta'] = theme.colors.tagForeground
  return variables as CSSProperties
}

const CUSTOM_VARIABLES = [
  ...Object.values(COLOR_VARIABLES), '--radius', '--theme-font-scale', '--theme-line-height',
  '--shadow-paper', '--shadow-paper-lifted', '--theme-atmosphere-tint',
  '--theme-atmosphere-noise', '--theme-atmosphere-vignette', '--run-running', '--run-exited',
  '--run-exited-wash', '--run-crashed', '--run-starting', '--ansi-green', '--ansi-red',
  '--ansi-yellow', '--ansi-blue', '--ansi-magenta',
]

let transitionTimer: ReturnType<typeof setTimeout> | null = null

function applyTheme(animate = true) {
  const theme = activeTheme.value
  const html = document.documentElement
  const body = document.body
  const commit = () => {
    BUILTIN_THEMES.forEach(item => html.classList.remove(item.className))
    html.classList.remove('theme-custom')
    CUSTOM_VARIABLES.forEach(variable => html.style.removeProperty(variable))
    if (theme.definition) {
      html.classList.add('theme-custom')
      for (const [property, value] of Object.entries(themeCssVariables(theme.definition))) {
        if (value !== undefined && value !== null) html.style.setProperty(property, String(value))
      }
    } else {
      html.classList.add(theme.className)
    }
    html.classList.toggle('dark', theme.isDark)
    body.classList.remove('paper-atmosphere', 'custom-theme-atmosphere')
    if (theme.atmosphere) body.classList.add(theme.atmosphere)
    if (isWindows) invoke('set_titlebar_dark', { dark: theme.isDark }).catch(() => {})
  }
  if (!animate) return commit()
  html.classList.add('theme-transitioning')
  void html.offsetHeight
  commit()
  if (transitionTimer) clearTimeout(transitionTimer)
  transitionTimer = setTimeout(() => html.classList.remove('theme-transitioning'), 400)
}

let initialized = false
watch([config, prefersDark, localThemes], () => {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(config.value))
  applyTheme(initialized)
  initialized = true
}, { immediate: true, deep: true })
watch(config, value => writeSetting(SETTING_KEY, value), { deep: true })

export async function refreshThemeLibrary() {
  themeLibraryLoading.value = true
  try {
    const library = await invoke<ThemeLibrary>('theme_library')
    localThemes.value = library.themes
    pendingPreviews.value = library.previews
    invalidThemeEntries.value = library.invalidEntries
    const repaired = { ...config.value }
    if (!themes.value.some(theme => theme.id === repaired.lightTheme && theme.appearance === 'light')) {
      repaired.lightTheme = 'paper'
    }
    if (!themes.value.some(theme => theme.id === repaired.darkTheme && theme.appearance === 'dark')) {
      repaired.darkTheme = 'ink'
    }
    if (repaired.lightTheme !== config.value.lightTheme || repaired.darkTheme !== config.value.darkTheme) {
      config.value = repaired
    }
  } finally {
    themeLibraryLoading.value = false
  }
}

function setLightTheme(id: string) {
  config.value = { ...config.value, lightTheme: themeForSlot(id, 'light').id }
}

function setDarkTheme(id: string) {
  config.value = { ...config.value, darkTheme: themeForSlot(id, 'dark').id }
}

function setThemeMode(mode: ThemeConfig['mode']) {
  config.value = { ...config.value, mode }
}

function cycleActiveTheme() {
  setThemeMode(activeTheme.value.isDark ? 'light' : 'dark')
}

const activeThemeLabel = computed(() => activeTheme.value.labelKey
  ? i18n.global.t(activeTheme.value.labelKey)
  : activeTheme.value.name ?? activeTheme.value.id)

void refreshThemeLibrary().catch(() => {})

export function useTheme() {
  return {
    config,
    activeTheme,
    activeThemeLabel,
    prefersDark,
    themes,
    lightThemes,
    darkThemes,
    localThemes,
    pendingPreviews,
    invalidThemeEntries,
    themeLibraryLoading,
    refreshThemeLibrary,
    setLightTheme,
    setDarkTheme,
    setThemeMode,
    cycleActiveTheme,
  }
}
