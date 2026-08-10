<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { resolveAsset } from '@/engines/client'
import type { ToolResultAttachment } from '@/engines/types'

const props = withDefaults(defineProps<{
  attachment: ToolResultAttachment
  autoLoad?: boolean
  compact?: boolean
}>(), {
  autoLoad: false,
  compact: false,
})

const { t } = useI18n()
const assetUrl = ref<string | null>(null)
const assetError = ref<string | null>(null)
const loadingAsset = ref(false)

async function loadAsset() {
  if (assetUrl.value || loadingAsset.value) return
  loadingAsset.value = true
  try {
    const result = await resolveAsset(props.attachment.asset.session, props.attachment.asset.nativeId)
    assetUrl.value = URL.createObjectURL(new Blob([new Uint8Array(result.bytes)], { type: result.mediaType }))
  } catch (error) {
    assetError.value = String(error)
  } finally {
    loadingAsset.value = false
  }
}

onMounted(() => {
  if (props.autoLoad) void loadAsset()
})

onUnmounted(() => {
  if (assetUrl.value) URL.revokeObjectURL(assetUrl.value)
})
</script>

<template>
  <div class="engine-asset-image" :class="{ 'is-compact': compact }">
    <img
      v-if="assetUrl"
      :src="assetUrl"
      :alt="attachment.title || t('engine.attachment')"
      class="engine-asset-image-content"
    />
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
  </div>
</template>

<style scoped>
.engine-asset-image { margin: 6px 0; }
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
</style>
