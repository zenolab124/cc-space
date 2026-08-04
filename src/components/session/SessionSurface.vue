<script setup lang="ts">
import type { VNodeRef } from 'vue'

const props = defineProps<{
  /** 让控制器继续使用真实根节点处理拖放、可见性与列内浮层。 */
  rootRef?: (element: HTMLElement | null) => void
}>()

const bindRoot: VNodeRef = (value) => {
  props.rootRef?.(value instanceof HTMLElement ? value : null)
}
</script>

<template>
  <div :ref="bindRoot" class="session-surface h-full min-h-0 flex bg-card">
    <main class="session-surface-main min-w-0 flex-1 flex flex-col relative">
      <slot name="topbar" />
      <slot name="overlay" />
      <slot />
      <slot name="interaction" />
      <slot name="input" />
      <slot name="footer" />
    </main>
    <slot name="side-panel" />
  </div>
</template>
