<script setup lang="ts">
import { ref, computed, inject } from 'vue'
import type { ContentBlock } from '@/types'
import { IMAGE_LOCATOR, buildCcimgUrl, withFullParam } from '@/utils/ccimg'
import { showImageContextMenu } from '@/composables/useImageActions'

const props = defineProps<{
  block: Extract<ContentBlock, { type: 'image' }>
  /** 图片所属 record 的 uuid;协议 URL 路径必需(历史区)。pending/流式区无落盘 uuid,走 dataUrl 无需此值 */
  recordUuid?: string | null
}>()

const expanded = ref(false)
// 协议路径按需加载：完成前给占位高度，减小加载完成时的布局突变
// （剩余位移由 SessionDetail 的滚动锚定补偿兜底）
const loaded = ref(false)

// 会话级定位上下文(projectId/sessionId/agentId);pending 预览等无宿主场景可能为 null
const locator = inject(IMAGE_LOCATOR, null)

// 双路径:source.data 非空(pending/流式内存路径)→ dataUrl;否则(已落盘历史)→ ccimg 协议 URL 按需取
const imgUrl = computed(() => {
  const { media_type, data, img_index } = props.block.source
  if (data) return `data:${media_type};base64,${data}`
  // 协议 URL 需要定位坐标齐备
  if (!locator?.value || !props.recordUuid) return null
  return buildCcimgUrl(locator.value, props.recordUuid, img_index ?? 0)
})

// 放大 overlay 用原图（缩略图 800px 放大会糊）；data: 路径两者同源
const fullUrl = computed(() => (imgUrl.value ? withFullParam(imgUrl.value) : null))

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') expanded.value = false
}
</script>

<template>
  <div v-if="imgUrl" class="mt-1">
    <img
      :src="imgUrl"
      :alt="$t('block.image', { mime: block.source.media_type, size: block.source.data?.length ?? '?' })"
      class="block-image-thumb"
      :class="{ 'block-image-loading': !loaded && !block.source.data }"
      role="button"
      tabindex="0"
      loading="lazy"
      decoding="async"
      @load="loaded = true"
      @error="loaded = true"
      @click="expanded = true"
      @keydown.enter.prevent="expanded = true"
      @keydown.space.prevent="expanded = true"
    />
    <Teleport to="body">
      <div
        v-if="expanded"
        class="block-image-overlay"
        @click="expanded = false"
        @keydown="onKeydown"
        tabindex="0"
      >
        <img
          :src="fullUrl ?? imgUrl"
          class="block-image-full"
          @contextmenu="showImageContextMenu($event, { src: fullUrl ?? imgUrl })"
        />
      </div>
    </Teleport>
  </div>
  <div v-else class="mt-2 rounded-md bg-background border border-border px-3 py-2">
    <div class="text-xs text-muted-foreground flex items-center gap-1.5">
      <span class="i-carbon-image w-3.5 h-3.5" />
      {{ $t('block.image', { mime: block.source.media_type, size: '?' }) }}
    </div>
  </div>
</template>

<style scoped>
.block-image-thumb {
  max-width: 300px;
  max-height: 200px;
  border-radius: 6px;
  border: 1px solid var(--border);
  cursor: zoom-in;
  object-fit: contain;
  background: var(--muted);
}
/* 协议路径加载中的骨架占位（120×160 接近缩略图典型尺寸，减小 0→实际高的突变） */
.block-image-loading {
  min-height: 120px;
  min-width: 160px;
}
.block-image-overlay {
  position: fixed;
  inset: 0;
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgb(0 0 0 / 0.7);
  cursor: zoom-out;
  backdrop-filter: blur(4px);
}
.block-image-full {
  max-width: 90vw;
  max-height: 90vh;
  object-fit: contain;
  border-radius: 8px;
}
</style>
