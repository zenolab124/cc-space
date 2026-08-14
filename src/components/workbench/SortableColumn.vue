<script setup lang="ts">
import { ref, computed } from 'vue'
import { useSortable } from '@dnd-kit/vue/sortable'

const props = defineProps<{
  tabId: string
  index: number
  flex: number
  fill?: boolean
  resizing?: boolean
}>()
const el = ref<HTMLElement>()
const handleEl = ref<HTMLElement>()

const { isDragging } = useSortable({
  id: computed(() => `col:${props.tabId}:${props.index}`),
  index: () => props.index,
  group: 'columns',
  element: el,
  handle: handleEl,
})

const setHandle = (node: any) => { handleEl.value = node }
</script>

<template>
  <div
    ref="el"
    class="sortable-col"
    data-workbench-column
    :class="{ 'sortable-col-dragging': isDragging, 'no-transition': resizing }"
    :style="fill
      ? { width: 'auto', flex: '1 1 0' }
      : { width: `${flex}px`, flex: `${flex} 0 auto` }"
  >
    <slot :is-dragging="isDragging" :handle-ref="setHandle" />
  </div>
</template>

<style scoped>
.sortable-col {
  min-width: 0;
  height: 100%;
  position: relative;
  overflow: hidden;
  transition: width 250ms cubic-bezier(0.32, 0.72, 0, 1);
}
.sortable-col-dragging {
  opacity: 0.4;
}
.no-transition {
  transition: none !important;
}
</style>
