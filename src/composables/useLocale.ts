import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setLocale, registerLocale, builtinLocales } from '../locales'
import type { MessageSchema } from '../locales'
import zhCN from '../locales/zh-CN.json'
import { readMigratedStorage } from '../utils/storageMigrate'
import { bridgeSetting, writeSetting } from '../utils/settingBridge'

const STORAGE_KEY = 'monet-locale'
const LEGACY_STORAGE_KEY = 'cc-space-locale' // 旧 key(更名前),一次性迁移读取用
const SETTING_KEY = 'locale' // ~/.monet/settings.json 权威键

function getStored(): string {
  return readMigratedStorage(STORAGE_KEY, LEGACY_STORAGE_KEY) || 'zh-CN'
}

const locale = ref(getStored())

const availableLocales = ref<Record<string, { label: string; nativeLabel: string }>>({
  ...builtinLocales,
})

const translating = ref(false)
const translateError = ref('')

interface ExternalLocale {
  code: string
  label: string
  nativeLabel: string
  messages: MessageSchema
}

interface LanguageIntent {
  code: string
  name: string
  native: string
  error?: string
}

async function loadExternalLocales() {
  try {
    const list = await invoke<ExternalLocale[]>('list_external_locales')
    for (const ext of list) {
      registerLocale(ext.code, ext.messages)
      availableLocales.value = {
        ...availableLocales.value,
        [ext.code]: { label: ext.label, nativeLabel: ext.nativeLabel },
      }
    }
  } catch {
    // 启动加载失败不阻塞
  }
}

function init() {
  setLocale(locale.value)
  invoke('set_agent_locale', { locale: locale.value }).catch(() => {})
  loadExternalLocales()
}

watch(locale, (code) => {
  localStorage.setItem(STORAGE_KEY, code)
  writeSetting(SETTING_KEY, code)
  setLocale(code)
  invoke('set_agent_locale', { locale: code }).catch(() => {})
})

// settings.json 为权威源:文件有值以文件为准,无值则上迁镜像现值
bridgeSetting({
  key: SETTING_KEY,
  uplift: () => localStorage.getItem(STORAGE_KEY) ?? undefined,
  apply: v => {
    if (typeof v === 'string' && v && v !== locale.value) locale.value = v
  },
})

function switchLocale(code: string) {
  locale.value = code
}

function addLocale(code: string, meta: { label: string; nativeLabel: string }, messages: MessageSchema) {
  registerLocale(code, messages)
  availableLocales.value = { ...availableLocales.value, [code]: meta }
}

async function parseLanguageIntent(userInput: string): Promise<LanguageIntent | null> {
  try {
    return await invoke<LanguageIntent>('parse_language_intent', { userInput })
  } catch {
    return null
  }
}

async function translateLocale(langCode: string, targetLang: string, targetNative: string) {
  translating.value = true
  translateError.value = ''
  try {
    const resultJson = await invoke<string>('translate_locale', {
      sourceJson: JSON.stringify(zhCN),
      targetLang,
      targetNative,
      langCode,
    })
    const messages = JSON.parse(resultJson) as MessageSchema
    addLocale(langCode, { label: targetLang, nativeLabel: targetNative }, messages)
    return true
  } catch (e) {
    translateError.value = String(e)
    return false
  } finally {
    translating.value = false
  }
}

async function deleteLocale(code: string) {
  try {
    await invoke('delete_external_locale', { code })
    const { [code]: _, ...rest } = availableLocales.value
    availableLocales.value = rest
    if (locale.value === code) switchLocale('zh-CN')
  } catch { /* ignore */ }
}

function isBuiltin(code: string) {
  return code in builtinLocales
}

init()

export function useLocale() {
  return {
    locale,
    availableLocales,
    translating,
    translateError,
    switchLocale,
    addLocale,
    parseLanguageIntent,
    translateLocale,
    deleteLocale,
    isBuiltin,
  }
}
