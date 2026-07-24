import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { isWindows } from './usePlatform'
import i18n from '../locales'
import { THEMES, getTheme, type ThemeMeta } from './themeRegistry'
import { readMigratedStorage } from '../utils/storageMigrate'
import { bridgeSetting, writeSetting } from '../utils/settingBridge'

const STORAGE_KEY = 'monet-theme'
const LEGACY_STORAGE_KEY = 'cc-space-theme' // 旧 key,一次性迁移读取用
const SETTING_KEY = 'theme' // ~/.monet/settings.json 权威键

interface ThemeConfig {
  version: 2
  lightTheme: string
  darkTheme: string
}

/** 解析主题值:兼容 'system'/'light'/'dark' 简写与 v2 对象(含 glass 降级、非法 id 兜底) */
function parseThemeValue(v: unknown): ThemeConfig | null {
  if (v === 'system') return { version: 2, lightTheme: 'paper', darkTheme: 'ink' }
  if (v === 'light') return { version: 2, lightTheme: 'paper', darkTheme: 'paper' }
  if (v === 'dark') return { version: 2, lightTheme: 'ink', darkTheme: 'ink' }
  if (typeof v === 'object' && v !== null && (v as { version?: unknown }).version === 2) {
    const o = v as { lightTheme?: unknown; darkTheme?: unknown }
    const norm = (id: unknown, fallback: string) => {
      if (id === 'glass') return fallback
      return typeof id === 'string' ? getTheme(id).id : fallback
    }
    return { version: 2, lightTheme: norm(o.lightTheme, 'paper'), darkTheme: norm(o.darkTheme, 'ink') }
  }
  return null
}

function loadConfig(): ThemeConfig {
  const raw = readMigratedStorage(STORAGE_KEY, LEGACY_STORAGE_KEY)
  if (!raw) return { version: 2, lightTheme: 'paper', darkTheme: 'ink' }
  let value: unknown = raw
  try { value = JSON.parse(raw) } catch {}
  return parseThemeValue(value) ?? { version: 2, lightTheme: 'paper', darkTheme: 'ink' }
}

const config = ref<ThemeConfig>(loadConfig())

// settings.json 为权威源:文件有值以文件为准,无值则上迁镜像现值
bridgeSetting({
  key: SETTING_KEY,
  uplift: () => (localStorage.getItem(STORAGE_KEY) !== null ? config.value : undefined),
  apply: v => {
    const parsed = parseThemeValue(v)
    if (parsed && JSON.stringify(parsed) !== JSON.stringify(config.value)) config.value = parsed
  },
})

const prefersDark = ref(window.matchMedia('(prefers-color-scheme: dark)').matches)
window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (e) => {
  prefersDark.value = e.matches
})

const activeTheme = computed<ThemeMeta>(() => {
  const id = prefersDark.value ? config.value.darkTheme : config.value.lightTheme
  return getTheme(id)
})

let transitionTimer: ReturnType<typeof setTimeout> | null = null

function applyTheme(animate = true) {
  const theme = activeTheme.value
  const html = document.documentElement
  const body = document.body

  const commit = () => {
    THEMES.forEach(t => html.classList.remove(t.className))
    html.classList.add(theme.className)
    html.classList.toggle('dark', theme.isDark)

    const atmosphereClasses = THEMES.map(t => t.atmosphere).filter(Boolean) as string[]
    atmosphereClasses.forEach(cls => body.classList.remove(cls))
    if (theme.atmosphere) body.classList.add(theme.atmosphere)

    // Windows 原生标题栏亮暗跟随（DWM 直调）。不能用 window.setTheme：
    // 它会 override WebView 的 prefers-color-scheme，而上面 prefersDark
    // 正以该信号为输入，回授成环（双槽交叉配置时无限闪烁）
    if (isWindows) {
      invoke('set_titlebar_dark', { dark: theme.isDark }).catch(() => {})
    }
  }

  if (animate) {
    html.classList.add('theme-transitioning')
    void html.offsetHeight
    commit()
    if (transitionTimer) clearTimeout(transitionTimer)
    transitionTimer = setTimeout(() => html.classList.remove('theme-transitioning'), 400)
  } else {
    commit()
  }
}

let initialized = false
watch([config, prefersDark], () => {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(config.value))
  applyTheme(initialized)
  initialized = true
}, { immediate: true, deep: true })

// 双写权威源(单独 watch:prefersDark 变化不触发文件写)
watch(config, v => writeSetting(SETTING_KEY, v), { deep: true })

function setLightTheme(themeId: string) {
  config.value = { ...config.value, lightTheme: themeId }
}

function setDarkTheme(themeId: string) {
  config.value = { ...config.value, darkTheme: themeId }
}

function cycleActiveTheme() {
  const currentId = prefersDark.value ? config.value.darkTheme : config.value.lightTheme
  const idx = THEMES.findIndex(t => t.id === currentId)
  const nextId = THEMES[(idx + 1) % THEMES.length].id
  if (prefersDark.value) {
    config.value = { ...config.value, darkTheme: nextId }
  } else {
    config.value = { ...config.value, lightTheme: nextId }
  }
}

const activeThemeLabel = computed(() => i18n.global.t(activeTheme.value.labelKey))

export function useTheme() {
  return {
    config,
    activeTheme,
    activeThemeLabel,
    prefersDark,
    themes: THEMES,
    setLightTheme,
    setDarkTheme,
    cycleActiveTheme,
  }
}
