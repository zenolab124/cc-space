<script setup lang="ts">
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { Menu } from '@tauri-apps/api/menu'
import { useUiState } from '@/composables/useUiState'
import { useAutomation } from '@/composables/useAutomation'
import { useWorkbench } from '@/composables/useWorkbench'
import { useWorkbenchCapture } from '@/composables/useWorkbenchCapture'
import { showSystemOpenMenu } from '@/composables/useFileOpener'

const { t } = useI18n()
const { activeSection } = useUiState()
const { activeTab, resetColumnSizes } = useWorkbench()
const { isCapturing, captureWorkbench } = useWorkbenchCapture()

async function showCaptureMenu() {
  if (isCapturing.value) return
  const nativeSupported = await invoke<boolean>('native_workbench_capture_supported').catch(() => false)
  const menu = await Menu.new({
    items: [
      {
        id: 'native',
        text: t('workbench.capture.native'),
        enabled: nativeSupported,
        action: () => void captureWorkbench('native'),
      },
      {
        id: 'canvas',
        text: t('workbench.capture.canvas'),
        action: () => void captureWorkbench('canvas'),
      },
    ],
  })
  await menu.popup()
}

// --- 自动化 ---
const { config: autoConfig, refresh: autoRefresh, loadingConfig, loadingStats } = useAutomation()
const autoLoading = computed(() => loadingConfig.value || loadingStats.value)
const openFailMsg = ref<string | null>(null)
let openFailTimer: ReturnType<typeof setTimeout> | undefined
async function openGlobalConfig() {
  const home = autoConfig.value?.homePath ?? ''
  const path = `${home}/.claude/settings.json`
  openFailMsg.value = null
  try {
    await invoke('open_hooks_config', { path, systemDefault: false })
  } catch {
    openFailMsg.value = t('common.openFailed')
    clearTimeout(openFailTimer)
    openFailTimer = setTimeout(() => { openFailMsg.value = null }, 3000)
  }
}

function showGlobalConfigMenu(event: MouseEvent) {
  const home = autoConfig.value?.homePath ?? ''
  const path = `${home}/.claude/settings.json`
  return showSystemOpenMenu(
    event,
    () => invoke('open_hooks_config', { path, systemDefault: true }),
    path,
  )
}
</script>

<template>
  <!-- 工作台 -->
  <button
    v-if="activeSection === 'workbench' && activeTab.columns.length > 0"
    data-capture-exclude
    class="icon-btn icon-btn-sm"
    :disabled="isCapturing"
    v-tooltip="isCapturing ? $t('workbench.capture.capturing') : $t('workbench.capture.action')"
    :aria-label="$t('workbench.capture.action')"
    @click="showCaptureMenu"
  >
    <span class="w-3.5 h-3.5" :class="isCapturing ? 'i-carbon-progress-bar-round animate-spin' : 'i-carbon-camera'" />
  </button>
  <button
    v-if="activeSection === 'workbench' && activeTab.columns.length >= 2"
    class="icon-btn icon-btn-sm"
    v-tooltip="$t('workbench.columns.resetWidths')"
    @click="resetColumnSizes(activeTab.id)"
  >
    <span class="i-carbon-fit-to-width w-3.5 h-3.5" />
  </button>

  <!-- 自动化 -->
  <template v-if="activeSection === 'automation'">
    <span v-if="openFailMsg" class="text-xs text-destructive">{{ openFailMsg }}</span>
    <button class="inline-flex items-center gap-1 text-[11px] px-2 py-0.5 rounded border border-border bg-card cursor-pointer hover:shadow-paper disabled:opacity-50 disabled:cursor-default" :disabled="!autoConfig" @click="openGlobalConfig" @contextmenu="showGlobalConfigMenu">{{ $t('common.openConfig') }}</button>
    <button class="inline-flex items-center gap-1 text-[11px] px-2 py-0.5 rounded border border-border bg-card cursor-pointer hover:shadow-paper disabled:opacity-50 disabled:cursor-default" :disabled="autoLoading" @click="autoRefresh">
      <span class="i-carbon-renew w-3 h-3" :class="{ 'animate-spin': autoLoading }" />
      {{ $t('common.refresh') }}
    </button>
  </template>
</template>
