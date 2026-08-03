<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

interface TrayTitleSlot { provider: string; item: string }
interface QuotaItem {
  id: string
  label: string
  kind: 'fiveHour' | 'weekly' | 'other'
}
interface QuotaGroup { id: string; label: string; items: QuotaItem[] }
interface ProviderQuota { id: string; displayName: string; groups: QuotaGroup[] }

interface TrayQuotaOption {
  key: string
  slot: TrayTitleSlot
  label: string
}

const props = defineProps<{
  providers: ProviderQuota[]
  modelValue: TrayTitleSlot[]
  disabled?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [slots: TrayTitleSlot[]]
}>()

const { t } = useI18n()
const open = ref(false)
const rootRef = ref<HTMLElement | null>(null)
const triggerRef = ref<HTMLButtonElement | null>(null)

function slotKey(slot: TrayTitleSlot): string {
  return `${slot.provider}/${slot.item}`
}

function metricLabel(item: QuotaItem): string {
  if (item.kind === 'fiveHour') return t('settings.traySlotFiveHour')
  if (item.kind !== 'weekly') return item.label
  if (item.label && !item.label.toLowerCase().includes('weekly')) {
    return t('settings.traySlotWeeklyNamed', { name: item.label })
  }
  return t('settings.traySlotWeekly')
}

const optionGroups = computed(() => props.providers
  .map(provider => ({
    id: provider.id,
    label: provider.displayName,
    options: provider.groups.flatMap(group => group.items.map(item => {
      const slot = { provider: provider.id, item: item.id }
      return {
        key: slotKey(slot),
        slot,
        label: metricLabel(item),
      }
    })),
  }))
  .filter(group => group.options.length > 0))

const flatOptions = computed(() => optionGroups.value.flatMap(group => group.options))
const knownKeys = computed(() => new Set(flatOptions.value.map(option => option.key)))
const activeKeys = computed(() => new Set(
  props.modelValue
    .map(slotKey)
    .filter(key => knownKeys.value.has(key)),
))
const selectedCount = computed(() => activeKeys.value.size)
const summary = computed(() => {
  if (flatOptions.value.length === 0) return t('settings.traySlotNoOptions')
  if (selectedCount.value === 0) return t('settings.traySlotNone')
  return t('settings.traySlotSelected', { count: selectedCount.value })
})

function isActive(option: TrayQuotaOption): boolean {
  return activeKeys.value.has(option.key)
}

function toggleOption(option: TrayQuotaOption) {
  const active = new Set(activeKeys.value)
  if (active.has(option.key)) active.delete(option.key)
  else active.add(option.key)
  emit(
    'update:modelValue',
    flatOptions.value
      .filter(item => active.has(item.key))
      .map(item => item.slot),
  )
}

function setOpen(next: boolean) {
  if (props.disabled || flatOptions.value.length === 0) return
  open.value = next
}

function close(restoreFocus = false) {
  open.value = false
  if (restoreFocus) nextTick(() => triggerRef.value?.focus())
}

async function openAndFocusFirst() {
  setOpen(true)
  await nextTick()
  optionButtons()[0]?.focus()
}

function optionButtons(): HTMLButtonElement[] {
  return Array.from(rootRef.value?.querySelectorAll<HTMLButtonElement>('[data-tray-quota-option]') ?? [])
}

function focusRelative(event: KeyboardEvent, offset: number) {
  event.preventDefault()
  const buttons = optionButtons()
  const current = buttons.indexOf(event.currentTarget as HTMLButtonElement)
  if (current < 0 || buttons.length === 0) return
  buttons[(current + offset + buttons.length) % buttons.length]?.focus()
}

function focusBoundary(event: KeyboardEvent, edge: 'first' | 'last') {
  event.preventDefault()
  const buttons = optionButtons()
  buttons[edge === 'first' ? 0 : buttons.length - 1]?.focus()
}

function onDocumentPointerDown(event: PointerEvent) {
  const target = event.target as Node | null
  if (target && !rootRef.value?.contains(target)) close()
}

function onDocumentFocusIn(event: FocusEvent) {
  const target = event.target as Node | null
  if (target && !rootRef.value?.contains(target)) close()
}

function onDocumentKeyDown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  close(true)
}

function removeDocumentListeners() {
  document.removeEventListener('pointerdown', onDocumentPointerDown)
  document.removeEventListener('focusin', onDocumentFocusIn)
  document.removeEventListener('keydown', onDocumentKeyDown)
}

watch(open, (isOpen) => {
  removeDocumentListeners()
  if (!isOpen) return
  document.addEventListener('pointerdown', onDocumentPointerDown)
  document.addEventListener('focusin', onDocumentFocusIn)
  document.addEventListener('keydown', onDocumentKeyDown)
})

watch(() => props.disabled, disabled => {
  if (disabled) close()
})

onBeforeUnmount(removeDocumentListeners)
</script>

<template>
  <div ref="rootRef" class="tray-quota-select">
    <button
      ref="triggerRef"
      type="button"
      class="tray-quota-trigger"
      :disabled="disabled || flatOptions.length === 0"
      aria-haspopup="menu"
      :aria-expanded="open"
      :aria-label="$t('settings.traySlotSelectorLabel', { summary })"
      @click="setOpen(!open)"
      @keydown.arrow-down.prevent="openAndFocusFirst"
    >
      <span>{{ summary }}</span>
      <span class="i-carbon-chevron-down tray-quota-chevron" :class="{ open }" aria-hidden="true" />
    </button>

    <div v-if="open" class="tray-quota-menu" role="menu">
      <div
        v-for="group in optionGroups"
        :key="group.id"
        class="tray-quota-group"
        role="group"
        :aria-label="group.label"
      >
        <div class="tray-quota-group-label">{{ group.label }}</div>
        <button
          v-for="option in group.options"
          :key="option.key"
          type="button"
          role="menuitemcheckbox"
          class="tray-quota-option"
          :class="{ active: isActive(option) }"
          :aria-checked="isActive(option)"
          data-tray-quota-option
          @click="toggleOption(option)"
          @keydown.arrow-down="focusRelative($event, 1)"
          @keydown.arrow-up="focusRelative($event, -1)"
          @keydown.home="focusBoundary($event, 'first')"
          @keydown.end="focusBoundary($event, 'last')"
        >
          <span class="tray-quota-check" aria-hidden="true">
            <span v-if="isActive(option)" class="i-carbon-checkmark" />
          </span>
          <span>{{ option.label }}</span>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.tray-quota-select {
  position: relative;
  flex: 0 0 auto;
}

.tray-quota-trigger {
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  gap: 7px;
  min-width: 96px;
  height: 27px;
  padding: 0 8px 0 9px;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: var(--card);
  color: var(--foreground);
  font-size: 11.5px;
  line-height: 1;
  cursor: pointer;
  transition: border-color 0.15s, background-color 0.15s;
}

.tray-quota-trigger:hover:not(:disabled),
.tray-quota-trigger[aria-expanded='true'] {
  border-color: color-mix(in srgb, var(--primary) 55%, var(--border));
  background: var(--muted);
}

.tray-quota-trigger:focus-visible,
.tray-quota-option:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 1px;
}

.tray-quota-trigger:disabled {
  color: var(--muted-foreground);
  cursor: default;
}

.tray-quota-chevron {
  width: 12px;
  height: 12px;
  color: var(--muted-foreground);
  transition: transform 0.15s;
}

.tray-quota-chevron.open {
  transform: rotate(180deg);
}

.tray-quota-menu {
  position: absolute;
  z-index: 40;
  top: calc(100% + 5px);
  right: 0;
  width: 224px;
  max-width: min(280px, calc(100vw - 32px));
  padding: 4px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--popover);
  color: var(--popover-foreground);
  box-shadow: var(--shadow-paper-lifted);
}

.tray-quota-group + .tray-quota-group {
  margin-top: 4px;
  padding-top: 4px;
  border-top: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
}

.tray-quota-group-label {
  padding: 3px 7px 4px;
  color: var(--muted-foreground);
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.2px;
}

.tray-quota-option {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  min-height: 27px;
  padding: 4px 7px;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--foreground);
  font-size: 11.5px;
  line-height: 1.35;
  text-align: left;
  cursor: pointer;
}

.tray-quota-option:hover,
.tray-quota-option:focus-visible {
  background: var(--muted);
}

.tray-quota-option.active {
  color: var(--primary);
}

.tray-quota-check {
  display: grid;
  flex: 0 0 auto;
  width: 14px;
  height: 14px;
  place-items: center;
  border: 1px solid var(--border);
  border-radius: 3px;
  color: var(--primary);
  font-size: 11px;
}

.tray-quota-option.active .tray-quota-check {
  border-color: color-mix(in srgb, var(--primary) 65%, var(--border));
  background: color-mix(in srgb, var(--primary) 10%, transparent);
}
</style>
