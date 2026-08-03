<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import type { PermissionMode } from '@/composables/useSessionSettings'

const props = withDefaults(defineProps<{
  selected: PermissionMode | null
  effective: PermissionMode
  variant?: 'toolbar' | 'submenu'
}>(), {
  variant: 'toolbar',
})

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
const listRef = ref<HTMLUListElement>()
const focusedIndex = ref(0)
const submenuOpenLeft = ref(false)

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

function placeSubmenu() {
  if (props.variant !== 'submenu') return
  submenuOpenLeft.value = false
  nextTick(() => {
    const rect = listRef.value?.getBoundingClientRect()
    if (rect && rect.right > window.innerWidth - 4) submenuOpenLeft.value = true
  })
}

function openMenu(focusItem = false) {
  if (!open.value) {
    open.value = true
    focusedIndex.value = currentIndex.value >= 0 ? currentIndex.value : 0
    placeSubmenu()
  }
  if (focusItem) nextTick(() => focusListItem(focusedIndex.value))
}

function toggle() {
  if (open.value) close()
  else openMenu(true)
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
  if (!open.value) {
    if (props.variant === 'submenu' && event.key === 'ArrowRight') {
      event.preventDefault()
      openMenu(true)
    }
    return
  }
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
    case 'ArrowLeft':
      if (props.variant === 'submenu') {
        event.preventDefault()
        close()
      }
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
  <div
    ref="containerRef"
    :class="variant === 'submenu' ? 'permission-submenu-root' : 'relative inline-flex'"
    @keydown="onKeydown"
    @mouseenter="variant === 'submenu' && openMenu(false)"
    @mouseleave="variant === 'submenu' && (open = false)"
  >
    <button
      ref="buttonRef"
      type="button"
      :class="variant === 'submenu'
        ? 'permission-submenu-button'
        : `h-[22px] px-1.5 text-xs rounded-[5px] text-muted-foreground hover:text-foreground hover:bg-muted
           transition-colors flex items-center gap-1 border border-border`"
      :title="$t('topbar.permTitle', { name: currentOption.desc })"
      aria-haspopup="listbox"
      :aria-expanded="open"
      @click="variant === 'submenu' ? openMenu(true) : toggle()"
    >
      <span :class="[currentOption.icon, 'w-3.5 h-3.5']" />
      <template v-if="variant === 'submenu'">
        <span class="flex-1 text-left">{{ $t('topbar.permissionMode') }}</span>
        <span class="permission-submenu-value">{{ currentOption.label }}</span>
        <span class="i-carbon-chevron-right w-3 h-3 text-muted-foreground" />
      </template>
      <template v-else>
        <span class="truncate">{{ currentOption.label }}</span>
        <span v-if="selected === null" class="text-2xs text-muted-foreground/70">CLI</span>
        <span class="i-carbon-chevron-down w-3 h-3 text-muted-foreground" />
      </template>
    </button>

    <ul
      v-if="open"
      ref="listRef"
      role="listbox"
      class="permission-options"
      :class="variant === 'submenu'
        ? ['is-submenu', submenuOpenLeft ? 'right-full mr-1' : 'left-full ml-1']
        : 'is-toolbar'"
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
.permission-submenu-root {
  position: relative;
  width: 100%;
}
.permission-submenu-button {
  display: flex;
  width: 100%;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  border: 0;
  color: var(--foreground);
  background: transparent;
  font-size: 12px;
  text-align: left;
  transition: background-color 150ms;
}
.permission-submenu-button:hover,
.permission-submenu-button:focus-visible {
  background: var(--muted);
  outline: none;
}
.permission-submenu-value {
  max-width: 84px;
  overflow: hidden;
  color: var(--muted-foreground);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.permission-options {
  position: absolute;
  z-index: 50;
  min-width: 176px;
  padding: 4px 0;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--popover);
  box-shadow: var(--shadow-paper-lifted);
}
.permission-options.is-toolbar {
  top: 100%;
  left: 0;
  margin-top: 4px;
}
.permission-options.is-submenu {
  top: 0;
  z-index: 51;
}
</style>
