<script setup lang="ts">
import { ref, type VNodeRef } from 'vue'
import SessionCopyToolbar from './SessionCopyToolbar.vue'
import { useSemanticCopy } from '@/composables/useSemanticCopy'
import { onSessionImageContextMenu } from '@/composables/useImageActions'

const props = defineProps<{
  /** 让控制器继续使用真实根节点处理拖放、可见性与列内浮层。 */
  rootRef?: (element: HTMLElement | null) => void
  /** Markdown 相对图片路径的解析根目录。 */
  fileRoot?: string | null
}>()

const surfaceRef = ref<HTMLElement | null>(null)
const copy = useSemanticCopy(surfaceRef)

const bindRoot: VNodeRef = (value) => {
  surfaceRef.value = value instanceof HTMLElement ? value : null
  props.rootRef?.(surfaceRef.value)
}
</script>

<template>
  <div
    :ref="bindRoot"
    class="session-surface h-full min-h-0 flex bg-card"
    @contextmenu="onSessionImageContextMenu($event, fileRoot)"
  >
    <main class="session-surface-main min-w-0 flex-1 flex flex-col relative">
      <slot name="topbar" />
      <slot name="overlay" />
      <slot />
      <slot name="interaction" />
      <slot name="input" />
      <slot name="footer" />
    </main>
    <slot name="side-panel" />
    <SessionCopyToolbar
      :visible="copy.toolbarVisible.value"
      :menu-open="copy.menuOpen.value"
      :left="copy.toolbarPosition.value.left"
      :top="copy.toolbarPosition.value.top"
      @copy="copy.copy"
      @toggle-menu="copy.toggleMenu"
    />
  </div>
</template>
