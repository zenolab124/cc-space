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
  compact?: boolean
}>(), {
  disabled: false,
  title: '',
  variant: 'primary',
  compact: false,
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
const compactLabel = computed(() => props.title || (existing.value
  ? t('session.jumpToOpenSession')
  : t('session.addToCurrentWorkbench')))
const compactClass = computed(() => existing.value
  ? 'border-primary/35 bg-primary/10 text-primary hover:bg-primary/15'
  : 'border-border bg-card text-muted-foreground hover:border-primary/35 hover:bg-muted hover:text-foreground')

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
  <div
    class="inline-flex shrink-0 rounded-md"
    :class="compact ? '' : ['overflow-hidden', outerClass]"
    @click.stop
  >
    <button
      v-if="compact"
      type="button"
      class="flex h-6 w-6 items-center justify-center rounded-md border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-45"
      :class="compactClass"
      :disabled="disabled || opening"
      :title="compactLabel"
      :aria-label="compactLabel"
      @click="openTarget()"
    >
      <span
        class="h-3.5 w-3.5"
        :class="existing ? 'i-carbon-launch' : 'i-carbon-add-alt'"
        aria-hidden="true"
      />
    </button>
    <button
      v-else
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
      v-if="!compact && !existing && ordinaryTabs.length"
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
