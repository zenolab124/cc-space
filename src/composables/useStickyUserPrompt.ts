import { ref, watch } from 'vue'
import { bridgeSetting, writeSetting } from '@/utils/settingBridge'

const STORAGE_KEY = 'monet:sticky-user-prompt'
const SETTING_KEY = 'stickyUserPrompt'
const DEFAULT_ENABLED = true

function loadEnabled(): boolean {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw === null ? DEFAULT_ENABLED : raw === 'true'
  } catch {
    return DEFAULT_ENABLED
  }
}

const stickyUserPromptEnabled = ref(loadEnabled())

// settings.json 为权威源:文件有值以文件为准,无值则上迁镜像现值
bridgeSetting({
  key: SETTING_KEY,
  uplift: () => (localStorage.getItem(STORAGE_KEY) !== null ? stickyUserPromptEnabled.value : undefined),
  apply: value => {
    if (typeof value === 'boolean' && value !== stickyUserPromptEnabled.value) {
      stickyUserPromptEnabled.value = value
    }
  },
})

watch(stickyUserPromptEnabled, value => {
  try {
    localStorage.setItem(STORAGE_KEY, String(value))
  } catch {}
  writeSetting(SETTING_KEY, value)
})

export function useStickyUserPrompt() {
  function setStickyUserPrompt(enabled: boolean) {
    stickyUserPromptEnabled.value = enabled
  }

  return { stickyUserPromptEnabled, setStickyUserPrompt }
}
