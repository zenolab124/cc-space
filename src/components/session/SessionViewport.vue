<script setup lang="ts">
import { onUnmounted, type VNodeRef } from 'vue'
import { registerScrollSurface } from '@/lib/scrollGestureCoordinator'

const props = defineProps<{
  scrollRef?: (element: HTMLElement | null) => void
}>()

const emit = defineEmits<{
  (event: 'scroll', value: Event): void
  (event: 'wheel', value: WheelEvent): void
}>()

let unregisterScrollSurface: (() => void) | null = null

const bindScroll: VNodeRef = (value) => {
  unregisterScrollSurface?.()
  unregisterScrollSurface = null
  const element = value instanceof HTMLElement ? value : null
  props.scrollRef?.(element)
  if (!element) return
  unregisterScrollSurface = registerScrollSurface(element, 'y', (delta) => {
    emit('wheel', new WheelEvent('wheel', { deltaY: delta }))
    element.scrollTop += delta
  })
}

onUnmounted(() => {
  unregisterScrollSurface?.()
  unregisterScrollSurface = null
})
</script>

<template>
  <div class="session-viewport flex-1 min-h-0 relative">
    <slot name="overlay" />
    <div
      :ref="bindScroll"
      class="session-viewport-scroll h-full min-h-0 overflow-y-auto overscroll-contain px-4 py-3 relative"
      @wheel.passive="emit('wheel', $event)"
      @scroll.passive="emit('scroll', $event)"
    >
      <slot />
    </div>
  </div>
</template>
