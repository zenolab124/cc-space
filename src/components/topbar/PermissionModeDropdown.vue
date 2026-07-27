<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'

export type PermissionMode = 'default' | 'plan' | 'acceptEdits' | 'auto' | 'bypassPermissions' | 'dontAsk'

const props = defineProps<{
  current: PermissionMode
}>()

const { t } = useI18n()

const emit = defineEmits<{
  (e: 'select', mode: PermissionMode): void
}>()

interface ModeOption {
  value: PermissionMode
  label: string
  icon: string
  desc: string
}

const OPTIONS = computed<ModeOption[]>(() => [
  { value: 'default', label: t('topbar.permApproval'), icon: 'i-carbon-locked', desc: t('topbar.permApprovalDesc') },
  { value: 'acceptEdits', label: t('topbar.permAutoEdit'), icon: 'i-carbon-edit', desc: t('topbar.permAutoEditDesc') },
  { value: 'plan', label: t('topbar.permPlan'), icon: 'i-carbon-document', desc: t('topbar.permPlanDesc') },
  { value: 'auto', label: t('topbar.permAuto'), icon: 'i-carbon-lightning', desc: t('topbar.permAutoDesc') },
  { value: 'bypassPermissions', label: t('topbar.permBypass'), icon: 'i-carbon-unlocked', desc: t('topbar.permBypassDesc') },
  { value: 'dontAsk', label: t('topbar.permDontAsk'), icon: 'i-carbon-close-outline', desc: t('topbar.permDontAskDesc') },
])

const open = ref(false)
const containerRef = ref<HTMLElement>()
const buttonRef = ref<HTMLButtonElement>()
const focusedIndex = ref(0)

const currentIndex = computed(() => OPTIONS.value.findIndex(o => o.value === props.current))
const currentOption = computed(() => OPTIONS.value.find(o => o.value === props.current) ?? OPTIONS.value[0])

function toggle() {
  open.value = !open.value
  if (open.value) {
    focusedIndex.value = currentIndex.value >= 0 ? currentIndex.value : 0
    nextTick(() => focusListItem(focusedIndex.value))
  }
}

function close() {
  open.value = false
  buttonRef.value?.focus()
}

function selectAt(index: number) {
  const o = OPTIONS.value[index]
  if (!o) return
  emit('select', o.value)
  close()
}

function onKeydown(e: KeyboardEvent) {
  if (!open.value) return
  switch (e.key) {
    case 'ArrowDown':
      e.preventDefault()
      focusedIndex.value = (focusedIndex.value + 1) % OPTIONS.value.length
      focusListItem(focusedIndex.value)
      break
    case 'ArrowUp':
      e.preventDefault()
      focusedIndex.value = (focusedIndex.value - 1 + OPTIONS.value.length) % OPTIONS.value.length
      focusListItem(focusedIndex.value)
      break
    case 'Enter':
      e.preventDefault()
      selectAt(focusedIndex.value)
      break
    case 'Escape':
      e.preventDefault()
      close()
      break
  }
}

function focusListItem(index: number) {
  nextTick(() => {
    const el = containerRef.value?.querySelectorAll<HTMLElement>('[data-item]')[index]
    el?.focus()
  })
}

function onDocumentClick(e: MouseEvent) {
  if (!open.value) return
  const target = e.target as Node
  if (containerRef.value && !containerRef.value.contains(target)) {
    open.value = false
  }
}

onMounted(() => document.addEventListener('mousedown', onDocumentClick))
onUnmounted(() => document.removeEventListener('mousedown', onDocumentClick))
</script>

<template>
  <div ref="containerRef" class="relative inline-flex" @keydown="onKeydown">
    <button
      ref="buttonRef"
      type="button"
      class="h-[22px] px-1.5 text-xs rounded-[5px] text-muted-foreground hover:text-foreground hover:bg-muted
             transition-colors flex items-center gap-1 border border-border"
      :title="$t('topbar.permTitle', { name: currentOption.label })"
      aria-haspopup="listbox"
      :aria-expanded="open"
      @click="toggle"
    >
      <span :class="[currentOption.icon, 'w-3.5 h-3.5']" />
      <span class="truncate">{{ currentOption.label }}</span>
      <span class="i-carbon-chevron-down w-3 h-3 text-muted-foreground" />
    </button>

    <ul
      v-if="open"
      role="listbox"
      class="absolute top-full left-0 mt-1 z-50 min-w-36 py-1 rounded-md border border-border
             shadow-paper-lifted bg-popover"
    >
      <li
        v-for="(o, i) in OPTIONS"
        :key="o.value"
        data-item
        role="option"
        tabindex="-1"
        :aria-selected="i === currentIndex"
        class="px-2 py-1.5 text-xs flex items-center gap-2 cursor-pointer
               text-muted-foreground hover:bg-muted hover:text-foreground focus:bg-muted focus:text-foreground focus:outline-none"
        @click="selectAt(i)"
        @mouseenter="focusedIndex = i"
      >
        <span
          class="w-3 h-3 shrink-0"
          :class="i === currentIndex ? 'i-carbon-checkmark text-primary' : ''"
        />
        <span :class="[o.icon, 'w-3.5 h-3.5 shrink-0']" />
        <div class="flex-1 min-w-0">
          <div>{{ o.label }}</div>
          <div class="text-2xs text-muted-foreground/70">{{ o.desc }}</div>
        </div>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.text-2xs {
  font-size: 10px;
  line-height: 1.3;
}
</style>
