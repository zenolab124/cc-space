<script setup lang="ts">
import { computed, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ContentBlock } from '@/types'
import { resolveAsset } from '@/engines/client'
import type { EngineSegment } from '@/engines/types'
import MessageBlock from '@/components/MessageBlock.vue'

const props = withDefaults(defineProps<{
  segment: EngineSegment
  compact?: boolean
  streaming?: boolean
}>(), {
  compact: false,
})
const { t } = useI18n()
const assetUrl = ref<string | null>(null)
const assetError = ref<string | null>(null)
const loadingAsset = ref(false)

const contentBlock = computed<ContentBlock | null>(() => {
  const segment = props.segment
  if (segment.kind === 'text') return { type: 'text', text: segment.text }
  if (segment.kind === 'reasoning') {
    return segment.visibility === 'redacted'
      ? { type: 'redacted_thinking' }
      : { type: 'thinking', thinking: segment.text }
  }
  if (segment.kind === 'unknown' && segment.summary?.trim()) {
    return { type: segment.typeName, summary: segment.summary }
  }
  return null
})

async function loadAsset() {
  if (props.segment.kind !== 'attachment' || assetUrl.value || loadingAsset.value) return
  loadingAsset.value = true
  try {
    const result = await resolveAsset(props.segment.asset.session, props.segment.asset.nativeId)
    assetUrl.value = URL.createObjectURL(new Blob([new Uint8Array(result.bytes)], { type: result.mediaType }))
  } catch (error) {
    assetError.value = String(error)
  } finally {
    loadingAsset.value = false
  }
}

onUnmounted(() => {
  if (assetUrl.value) URL.revokeObjectURL(assetUrl.value)
})
</script>

<template>
  <div v-if="contentBlock" :class="compact && 'engine-compact-text'">
    <MessageBlock :block="contentBlock" :streaming="streaming" />
  </div>

  <div v-else-if="segment.kind === 'attachment'" class="my-1.5">
    <img
      v-if="assetUrl"
      :src="assetUrl"
      :alt="segment.title || t('engine.attachment')"
      class="max-h-96 max-w-full rounded border border-border shadow-paper"
    />
    <button
      v-else
      type="button"
      class="inline-flex items-center gap-1 text-xs text-primary hover:underline disabled:opacity-50"
      :disabled="loadingAsset"
      @click="loadAsset"
    >
      <span :class="loadingAsset ? 'i-carbon-renew animate-spin' : 'i-carbon-image'" class="h-3 w-3" />
      {{ loadingAsset ? t('common.loading') : (segment.title || t('engine.loadAttachment')) }}
    </button>
    <p v-if="assetError" role="alert" class="mt-1 text-xs text-destructive">{{ assetError }}</p>
  </div>

</template>

<style scoped>
.engine-compact-text {
  color: var(--muted-foreground);
}
.engine-compact-text :deep(.prose-msg.message-prose) {
  font-size: 12px;
  line-height: 1.6;
}
</style>
