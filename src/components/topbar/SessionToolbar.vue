<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref } from 'vue'
import type { EngineAccent } from '@/engines/presentation'
import ContextProgress from './ContextProgress.vue'

const props = withDefaults(defineProps<{
  usedContextTokens?: number
  contextCapacity?: number
  accent?: EngineAccent
}>(), {
  usedContextTokens: 0,
  contextCapacity: 0,
  accent: 'primary',
})

const menuOpen = defineModel<boolean>('menuOpen', { default: false })
const containerRef = ref<HTMLElement>()
const menuRef = ref<HTMLElement>()
const menuPanelRef = ref<HTMLElement>()
const containerWidth = ref(Number.POSITIVE_INFINITY)
const menuAlignLeft = ref(false)
let resizeObserver: ResizeObserver | null = null

function toggleMenu() {
  menuOpen.value = !menuOpen.value
  if (!menuOpen.value) return
  nextTick(() => {
    const panel = menuPanelRef.value
    if (!panel) return
    menuAlignLeft.value = panel.getBoundingClientRect().right > window.innerWidth - 4
  })
}

function onDocumentPointerDown(event: MouseEvent) {
  if (menuOpen.value && menuRef.value && !menuRef.value.contains(event.target as Node)) {
    menuOpen.value = false
  }
}

onMounted(() => {
  const element = containerRef.value
  if (element) {
    containerWidth.value = element.clientWidth
    resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) containerWidth.value = entry.contentRect.width
    })
    resizeObserver.observe(element)
  }
  document.addEventListener('mousedown', onDocumentPointerDown)
})

onUnmounted(() => {
  resizeObserver?.disconnect()
  document.removeEventListener('mousedown', onDocumentPointerDown)
})
</script>

<template>
  <div
    ref="containerRef"
    class="session-toolbar shrink-0 flex items-center gap-1.5 border-b border-border bg-card px-3 py-1"
  >
    <slot name="controls" :container-width="containerWidth" />

    <ContextProgress
      v-if="props.contextCapacity > 0"
      :used="props.usedContextTokens"
      :capacity="props.contextCapacity"
      :accent="props.accent"
      compact
    />
    <span v-else class="min-w-0 flex-1" />

    <slot name="actions" />

    <div ref="menuRef" class="relative inline-flex shrink-0">
      <button
        type="button"
        class="rounded p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        :title="$t('topbar.sessionMenu')"
        :aria-expanded="menuOpen"
        @click="toggleMenu"
      >
        <span class="i-carbon-overflow-menu-horizontal h-3.5 w-3.5" />
      </button>

      <div
        v-if="menuOpen"
        ref="menuPanelRef"
        class="absolute top-full z-50 mt-1 w-52 rounded-md border border-border bg-popover py-1 shadow-paper-lifted"
        :class="menuAlignLeft ? 'left-0' : 'right-0'"
      >
        <slot name="menu" :container-width="containerWidth" />
      </div>
    </div>
  </div>
</template>
