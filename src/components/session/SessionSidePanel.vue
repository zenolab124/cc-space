<script setup lang="ts">
defineProps<{
  mounted: boolean
  expanded: boolean
  width: number
}>()
</script>

<template>
  <div
    v-if="mounted"
    class="session-side-panel h-full shrink-0 overflow-hidden"
    :class="expanded && 'border-l border-border'"
    :style="{
      width: expanded ? `${width}px` : '0',
      transition: 'width 250ms cubic-bezier(0.32, 0.72, 0, 1)',
    }"
  >
    <slot />
  </div>
</template>
