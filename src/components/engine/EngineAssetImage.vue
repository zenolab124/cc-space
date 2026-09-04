<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { resolveAssetLimited } from '@/engines/assetLoader'
import type { ToolResultAttachment } from '@/engines/types'
import { errorMessage } from '@/utils/errorMessage'
import { showImageContextMenu } from '@/composables/useImageActions'

const props = withDefaults(defineProps<{
  attachment: ToolResultAttachment
  autoLoad?: boolean
  compact?: boolean
}>(), {
  autoLoad: false,
  compact: false,
})

const { t } = useI18n()
const containerRef = ref<HTMLElement | null>(null)
const assetUrl = ref<string | null>(null)
const fullAssetUrl = ref<string | null>(null)
const assetError = ref<string | null>(null)
const loadingAsset = ref(false)
const loadingFullAsset = ref(false)
const lightboxOpen = ref(false)
let observer: IntersectionObserver | null = null
let disposed = false

function createAssetUrl(result: { mediaType: string; bytes: number[] }): string {
  return URL.createObjectURL(new Blob([new Uint8Array(result.bytes)], { type: result.mediaType }))
}

async function loadAsset() {
  if (assetUrl.value || loadingAsset.value) return
  loadingAsset.value = true
  assetError.value = null
  try {
    const result = await resolveAssetLimited(
      props.attachment.asset.session,
      props.attachment.asset.nativeId,
      true,
    )
    const url = createAssetUrl(result)
    if (disposed) URL.revokeObjectURL(url)
    else assetUrl.value = url
  } catch (error) {
    assetError.value = errorMessage(error, t('common.unknownError'))
  } finally {
    loadingAsset.value = false
  }
}

async function openLightbox() {
  if (!assetUrl.value) {
    await loadAsset()
    if (!assetUrl.value) return
  }
  lightboxOpen.value = true
  if (fullAssetUrl.value || loadingFullAsset.value) return
  loadingFullAsset.value = true
  assetError.value = null
  try {
    const result = await resolveAssetLimited(
      props.attachment.asset.session,
      props.attachment.asset.nativeId,
    )
    const url = createAssetUrl(result)
    if (disposed || !lightboxOpen.value) URL.revokeObjectURL(url)
    else fullAssetUrl.value = url
  } catch (error) {
    assetError.value = errorMessage(error, t('common.unknownError'))
  } finally {
    loadingFullAsset.value = false
  }
}

function closeLightbox() {
  lightboxOpen.value = false
  if (fullAssetUrl.value) {
    URL.revokeObjectURL(fullAssetUrl.value)
    fullAssetUrl.value = null
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') closeLightbox()
}

onMounted(() => {
  if (!props.autoLoad) return
  if (typeof IntersectionObserver === 'undefined') {
    void loadAsset()
    return
  }
  observer = new IntersectionObserver(entries => {
    if (!entries.some(entry => entry.isIntersecting)) return
    observer?.disconnect()
    observer = null
    void loadAsset()
  }, { rootMargin: '320px' })
  if (containerRef.value) observer.observe(containerRef.value)
})

watch(lightboxOpen, open => {
  if (open) window.addEventListener('keydown', onKeydown)
  else window.removeEventListener('keydown', onKeydown)
})

onUnmounted(() => {
  disposed = true
  observer?.disconnect()
  window.removeEventListener('keydown', onKeydown)
  if (assetUrl.value) URL.revokeObjectURL(assetUrl.value)
  if (fullAssetUrl.value) URL.revokeObjectURL(fullAssetUrl.value)
})
</script>

<template>
  <div ref="containerRef" class="engine-asset-image" :class="{ 'is-compact': compact }">
    <button
      v-if="assetUrl"
      type="button"
      class="engine-asset-image-open"
      :aria-label="attachment.title || t('engine.attachment')"
      @click="openLightbox"
    >
      <img
        :src="assetUrl"
        :alt="attachment.title || t('engine.attachment')"
        class="engine-asset-image-content"
        loading="lazy"
        decoding="async"
      />
    </button>
    <button
      v-else
      type="button"
      class="engine-asset-image-load"
      :disabled="loadingAsset"
      @click="loadAsset"
    >
      <span :class="loadingAsset ? 'i-carbon-renew animate-spin' : 'i-carbon-image'" class="h-3 w-3" />
      {{ loadingAsset ? t('common.loading') : (attachment.title || t('engine.loadAttachment')) }}
    </button>
    <p v-if="assetError" role="alert" class="mt-1 text-xs text-destructive">{{ assetError }}</p>
    <Teleport to="body">
      <div
        v-if="lightboxOpen && assetUrl"
        class="engine-asset-lightbox"
        role="dialog"
        aria-modal="true"
        :aria-label="attachment.title || t('engine.attachment')"
        @click.self="closeLightbox"
      >
        <img
          :src="fullAssetUrl || assetUrl"
          :alt="attachment.title || t('engine.attachment')"
          class="engine-asset-lightbox-image"
          decoding="async"
          @contextmenu="showImageContextMenu($event, { src: fullAssetUrl || assetUrl })"
        />
        <span v-if="loadingFullAsset" class="engine-asset-lightbox-loading">
          <span class="i-carbon-renew animate-spin h-4 w-4" />
          {{ t('common.loading') }}
        </span>
        <button
          type="button"
          class="engine-asset-lightbox-close"
          :aria-label="t('common.close')"
          @click="closeLightbox"
        >
          <span class="i-carbon-close h-5 w-5" />
        </button>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.engine-asset-image { margin: 6px 0; }
.engine-asset-image-open {
  display: block;
  max-width: 100%;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: zoom-in;
}
.engine-asset-image-content {
  display: block;
  max-width: 100%;
  max-height: 384px;
  border: 1px solid var(--border);
  border-radius: 6px;
  object-fit: contain;
  box-shadow: var(--shadow-paper);
}
.engine-asset-image.is-compact { margin: 4px 0 0; }
.engine-asset-image.is-compact .engine-asset-image-content {
  max-width: min(100%, 420px);
  max-height: 320px;
}
.engine-asset-image-load {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--primary);
  font-size: 12px;
}
.engine-asset-image-load:hover { text-decoration: underline; }
.engine-asset-image-load:disabled { opacity: 0.5; }
.engine-asset-lightbox {
  position: fixed;
  inset: 0;
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 5vh 5vw;
  background: rgb(0 0 0 / 0.72);
  backdrop-filter: blur(4px);
}
.engine-asset-lightbox-image {
  max-width: 90vw;
  max-height: 90vh;
  object-fit: contain;
  border-radius: 8px;
}
.engine-asset-lightbox-loading {
  position: absolute;
  bottom: 24px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  border-radius: 999px;
  color: white;
  background: rgb(0 0 0 / 0.55);
  font-size: 12px;
}
.engine-asset-lightbox-close {
  position: absolute;
  top: 18px;
  right: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border: 1px solid rgb(255 255 255 / 0.22);
  border-radius: 999px;
  color: white;
  background: rgb(0 0 0 / 0.45);
}
.engine-asset-lightbox-close:hover { background: rgb(0 0 0 / 0.68); }
</style>
