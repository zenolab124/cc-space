<script setup lang="ts">
import { computed, ref } from 'vue'
import { Menu } from '@tauri-apps/api/menu'
import { useI18n } from 'vue-i18n'
import { useWorkbench } from '@/composables/useWorkbench'
import { useUiState } from '@/composables/useUiState'

const props = withDefaults(defineProps<{
  sessionId: string
  disabled?: boolean
  title?: string
  variant?: 'primary' | 'secondary'
}>(), {
  disabled: false,
  title: '',
  variant: 'primary',
})

const { t } = useI18n()
const { ordinaryTabs, defaultOrdinaryTab, findSession, openSessionInTab } = useWorkbench()
const { switchSection } = useUiState()
const opening = ref(false)
const menuOpening = ref(false)
const existing = computed(() => findSession(props.sessionId))

const mainClass = computed(() => props.variant === 'primary'
  ? 'bg-primary text-primary-foreground hover:shadow-paper'
  : 'bg-card text-muted-foreground hover:bg-muted hover:text-foreground')
const outerClass = computed(() => props.variant === 'secondary' ? 'border border-border' : '')
const dividerClass = computed(() => props.variant === 'primary'
  ? 'border-primary-foreground/20'
  : 'border-border')

function openTarget(tabId?: string) {
  if (props.disabled || opening.value) return
  opening.value = true
  try {
    openSessionInTab(props.sessionId, tabId)
    switchSection('workbench')
  } finally {
    opening.value = false
  }
}

async function chooseTarget() {
  if (props.disabled || opening.value || menuOpening.value || existing.value || ordinaryTabs.value.length === 0) return
  menuOpening.value = true
  const defaultId = defaultOrdinaryTab.value?.id
  try {
    const menu = await Menu.new({
      items: ordinaryTabs.value.map(tab => ({
        text: tab.id === defaultId
          ? t('workbench.targetDefault', { name: tab.name })
          : tab.name,
        action: () => {
          menuOpening.value = false
          openTarget(tab.id)
        },
      })),
    })
    await menu.popup()
  } finally {
    menuOpening.value = false
  }
}
</script>

<template>
  <div class="inline-flex shrink-0 overflow-hidden rounded-md" :class="outerClass" @click.stop>
    <button
      type="button"
      class="min-h-7 px-2.5 py-1 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-45"
      :class="mainClass"
      :disabled="disabled || opening || menuOpening"
      :title="title || (existing ? t('session.openWorkbench') : t('session.addToWorkbench'))"
      @click="openTarget()"
    >
      {{ existing ? t('session.openWorkbench') : t('session.addToWorkbench') }}
    </button>
    <button
      v-if="!existing && ordinaryTabs.length"
      type="button"
      class="flex min-h-7 w-7 items-center justify-center border-l px-1 transition-colors disabled:cursor-not-allowed disabled:opacity-45"
      :class="[mainClass, dividerClass]"
      :disabled="disabled || opening || menuOpening"
      :title="t('workbench.chooseTarget')"
      :aria-label="t('workbench.chooseTarget')"
      @click="chooseTarget"
    >
      <span class="i-carbon-chevron-down h-3 w-3" aria-hidden="true" />
    </button>
  </div>
</template>
