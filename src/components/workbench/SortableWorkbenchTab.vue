<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useSortable } from '@dnd-kit/vue/sortable'

const props = defineProps<{
  tabId: string
  index: number
  active: boolean
  disabled?: boolean
}>()

const emit = defineEmits<{
  (event: 'activate'): void
  (event: 'rename'): void
  (event: 'contextmenu', mouseEvent: MouseEvent): void
}>()

const element = ref<HTMLElement>()
const suppressActivation = ref(false)
const { isDragging, isDropTarget } = useSortable({
  id: computed(() => `tab:${props.tabId}`),
  index: () => props.index,
  group: 'workbench-tabs',
  element,
  disabled: () => !!props.disabled,
})

watch(isDragging, (dragging, wasDragging) => {
  if (dragging || !wasDragging) return
  suppressActivation.value = true
  window.setTimeout(() => { suppressActivation.value = false }, 0)
})

function activate() {
  if (!suppressActivation.value) emit('activate')
}

function activateFromKeyboard(event: KeyboardEvent) {
  event.preventDefault()
  event.stopImmediatePropagation()
  activate()
}
</script>

<template>
  <div
    ref="element"
    class="wb-tab"
    :class="{
      active,
      dragging: isDragging,
      'drop-target': isDropTarget && !isDragging,
      disabled,
    }"
    role="tab"
    :aria-selected="active"
    :aria-grabbed="isDragging"
    :tabindex="active ? 0 : -1"
    @click="activate"
    @keydown.enter="activateFromKeyboard"
    @keydown.space="activateFromKeyboard"
    @dblclick="emit('rename')"
    @contextmenu="emit('contextmenu', $event)"
  >
    <slot />
  </div>
</template>

<style scoped>
.wb-tab {
  display: inline-flex;
  position: relative;
  align-items: center;
  flex: 0 0 auto;
  gap: 5px;
  height: 22px;
  padding: 2px 10px;
  border-radius: var(--radius);
  color: var(--muted-foreground);
  font-size: 11px;
  white-space: nowrap;
  cursor: grab;
  touch-action: none;
  transition: opacity 120ms ease, background-color 120ms ease, box-shadow 120ms ease;
}
.wb-tab:hover { background: var(--muted); }
.wb-tab.active {
  background: var(--card);
  box-shadow: var(--shadow-paper);
  color: var(--foreground);
  font-weight: 500;
}
.wb-tab.dragging {
  opacity: 0.45;
  cursor: grabbing;
}
.wb-tab.drop-target {
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--primary) 35%, transparent);
}
.wb-tab.disabled { cursor: default; }
.wb-tab:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 1px;
}
:deep(.wb-tab-close) {
  width: 0;
  height: 12px;
  border-radius: 3px;
  color: var(--muted-foreground);
  opacity: 0;
  cursor: pointer;
  transition: width 120ms ease, opacity 120ms ease;
}
:deep(.wb-tab-close:hover) {
  background: var(--accent);
  color: var(--foreground);
}
.wb-tab:hover :deep(.wb-tab-close),
.wb-tab.active :deep(.wb-tab-close) {
  width: 12px;
  opacity: 0.7;
}

@media (prefers-reduced-motion: reduce) {
  .wb-tab,
  :deep(.wb-tab-close) {
    transition: none !important;
  }
}
</style>
