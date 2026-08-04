<script setup lang="ts">
import type { VNodeRef } from 'vue'

const props = defineProps<{
  scrollRef?: (element: HTMLElement | null) => void
}>()

const emit = defineEmits<{
  (event: 'scroll', value: Event): void
  (event: 'wheel', value: WheelEvent): void
}>()

const bindScroll: VNodeRef = (value) => {
  props.scrollRef?.(value instanceof HTMLElement ? value : null)
}
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
