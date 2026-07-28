import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { relaunch } from '@tauri-apps/plugin-process'

export type UpdateStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'restarting' | 'up-to-date' | 'error'
export type UpdateChannel = 'stable' | 'nightly'

interface UpdateMeta {
  version: string
  notes: string
}

const status = ref<UpdateStatus>('idle')
const newVersion = ref('')
const releaseNotes = ref('')
const errorMessage = ref('')
const downloadProgress = ref(0)
const channel = ref<UpdateChannel>('stable')

// 检查/安装走 Rust command 而非 plugin-updater 的 check()：
// 通道切换需要动态 endpoint，而 JS 侧 CheckOptions 不含该字段（详见 src-tauri/src/updater.rs）
async function applyMeta(meta: UpdateMeta | null) {
  if (meta) {
    newVersion.value = meta.version
    releaseNotes.value = meta.notes ?? ''
    status.value = 'available'
  } else {
    status.value = 'up-to-date'
  }
}

async function loadChannel() {
  try {
    channel.value = (await invoke<string>('get_update_channel')) as UpdateChannel
  } catch {
    channel.value = 'stable'
  }
}

async function setChannel(next: UpdateChannel) {
  await invoke('set_update_channel', { channel: next })
  channel.value = next
  // 通道变了，旧的检查结果不再适用——清掉，让用户重新检查
  status.value = 'idle'
  newVersion.value = ''
  releaseNotes.value = ''
}

async function checkForUpdate() {
  status.value = 'checking'
  errorMessage.value = ''
  try {
    await applyMeta(await invoke<UpdateMeta | null>('updater_check'))
  } catch (e) {
    errorMessage.value = String(e)
    status.value = 'error'
  }
}

async function downloadAndInstall() {
  if (status.value !== 'available') return
  status.value = 'downloading'
  downloadProgress.value = 0
  let unlisten: UnlistenFn | undefined
  try {
    unlisten = await listen<{ downloaded: number; total: number | null }>(
      'updater://progress',
      ({ payload }) => {
        downloadProgress.value = payload.total
          ? Math.round((payload.downloaded / payload.total) * 100)
          : 0
      },
    )
    await invoke('updater_install')
    downloadProgress.value = 100
    // install resolve = 新版已替换就位,但 Tauri 不会自动重启——
    // 必须显式 relaunch 才生效(缺此调用曾致 UI 永卡"下载 100%")
    status.value = 'restarting'
    await relaunch()
  } catch (e) {
    errorMessage.value = String(e)
    status.value = 'error'
  } finally {
    unlisten?.()
  }
}

async function initAutoCheck() {
  await loadChannel()
  await new Promise(r => setTimeout(r, 5000))
  try {
    const meta = await invoke<UpdateMeta | null>('updater_check')
    if (meta) await applyMeta(meta)
  } catch {
    // 静默失败，不打扰用户
  }
}

export function useUpdater() {
  return {
    status,
    newVersion,
    releaseNotes,
    errorMessage,
    downloadProgress,
    channel,
    loadChannel,
    setChannel,
    checkForUpdate,
    downloadAndInstall,
    initAutoCheck,
  }
}
