<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import type { PermissionMode } from '@/composables/useSessionSettings'

const props = defineProps<{
  selected: PermissionMode | null
  effective: PermissionMode
}>()

const { t } = useI18n()

const emit = defineEmits<{
  (e: 'select', mode: PermissionMode | null): void
}>()

interface ModeOption {
  value: PermissionMode | null
  label: string
  icon: string
  desc: string
}

const OPTIONS = computed<ModeOption[]>(() => [
  { value: null, label: t('topbar.permFollowCli'), icon: 'i-carbon-parent-child', desc: t('topbar.permFollowCliDesc') },
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

const currentIndex = computed(() => OPTIONS.value.findIndex(option => option.value === props.selected))
const effectiveOption = computed(() =>
  OPTIONS.value.find(option => option.value === props.effective) ?? OPTIONS.value[1],
)
const currentOption = computed(() => {
  if (props.selected !== null) {
    return OPTIONS.value.find(option => option.value === props.selected) ?? effectiveOption.value
  }
  return {
    value: null,
    label: effectiveOption.value.label,
    icon: effectiveOption.value.icon,
    desc: t('topbar.permFollowingCli', { name: effectiveOption.value.label }),
  } satisfies ModeOption
})

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
  const option = OPTIONS.value[index]
  if (!option) return
  emit('select', option.value)
  close()
}

function onKeydown(event: KeyboardEvent) {
  if (!open.value) return
  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      focusedIndex.value = (focusedIndex.value + 1) % OPTIONS.value.length
      focusListItem(focusedIndex.value)
      break
    case 'ArrowUp':
      event.preventDefault()
      focusedIndex.value = (focusedIndex.value - 1 + OPTIONS.value.length) % OPTIONS.value.length
      focusListItem(focusedIndex.value)
      break
    case 'Enter':
      event.preventDefault()
      selectAt(focusedIndex.value)
      break
    case 'Escape':
      event.preventDefault()
      close()
      break
  }
}

function focusListItem(index: number) {
  nextTick(() => {
    const element = containerRef.value?.querySelectorAll<HTMLElement>('[data-item]')[index]
    element?.focus()
  })
}

function onDocumentClick(event: MouseEvent) {
  if (!open.value) return
  const target = event.target as Node
  if (containerRef.value && !containerRef.value.contains(target)) open.value = false
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
      :title="$t('topbar.permTitle', { name: currentOption.desc })"
      aria-haspopup="listbox"
      :aria-expanded="open"
      @click="toggle"
    >
      <span :class="[currentOption.icon, 'w-3.5 h-3.5']" />
      <span class="truncate">{{ currentOption.label }}</span>
      <span v-if="selected === null" class="text-2xs text-muted-foreground/70">CLI</span>
      <span class="i-carbon-chevron-down w-3 h-3 text-muted-foreground" />
    </button>

    <ul
      v-if="open"
      role="listbox"
      class="absolute top-full left-0 mt-1 z-50 min-w-44 py-1 rounded-md border border-border
             shadow-paper-lifted bg-popover"
    >
      <li
        v-for="(option, index) in OPTIONS"
        :key="option.value ?? 'inherit'"
        data-item
        role="option"
        tabindex="-1"
        :aria-selected="index === currentIndex"
        class="px-2 py-1.5 text-xs flex items-center gap-2 cursor-pointer
               text-muted-foreground hover:bg-muted hover:text-foreground focus:bg-muted focus:text-foreground focus:outline-none"
        @click="selectAt(index)"
        @mouseenter="focusedIndex = index"
      >
        <span
          class="w-3 h-3 shrink-0"
          :class="index === currentIndex ? 'i-carbon-checkmark text-primary' : ''"
        />
        <span :class="[option.icon, 'w-3.5 h-3.5 shrink-0']" />
        <div class="flex-1 min-w-0">
          <div>{{ option.label }}</div>
          <div class="text-2xs text-muted-foreground/70">
            {{ option.value === null ? $t('topbar.permFollowingCli', { name: effectiveOption.label }) : option.desc }}
          </div>
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
