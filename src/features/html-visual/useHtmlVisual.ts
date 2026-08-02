import { ref, watch } from 'vue'
import { readMigratedStorage } from '../../utils/storageMigrate'
import { bridgeSetting, writeSetting } from '../../utils/settingBridge'

const STORAGE_KEY = 'monet:feature:html-visual'
const LEGACY_STORAGE_KEY = 'cc-space:feature:html-visual' // 旧 key,一次性迁移读取用
const SETTING_KEY = 'featureHtmlVisual' // ~/.monet/settings.json 权威键

const enabled = ref(readMigratedStorage(STORAGE_KEY, LEGACY_STORAGE_KEY) === 'true')

watch(enabled, v => {
  localStorage.setItem(STORAGE_KEY, String(v))
  writeSetting(SETTING_KEY, v)
})

// settings.json 为权威源:文件有值以文件为准,无值则上迁镜像现值
bridgeSetting({
  key: SETTING_KEY,
  uplift: () => (localStorage.getItem(STORAGE_KEY) !== null ? enabled.value : undefined),
  apply: v => {
    if (typeof v === 'boolean') enabled.value = v
  },
})

export function useHtmlVisual() {
  return { enabled }
}
