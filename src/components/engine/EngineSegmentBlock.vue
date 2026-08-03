<script setup lang="ts">
import { onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { resolveAsset, segmentText } from '@/engines/client'
import type { EngineSegment } from '@/engines/types'

const props = defineProps<{ segment: EngineSegment }>()
const { t } = useI18n()
const assetUrl = ref<string | null>(null)
const assetError = ref<string | null>(null)
const loadingAsset = ref(false)

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
  <div v-if="segment.kind === 'reasoning'" class="mt-1 border-l-2 border-border pl-2 text-xs text-muted-foreground whitespace-pre-wrap leading-relaxed">
    <div class="mb-1 text-[10px] uppercase tracking-wide">{{ t('engine.reasoning') }}</div>
    {{ segment.text }}
  </div>
  <div v-else-if="segment.kind === 'attachment'" class="mt-2">
    <img v-if="assetUrl" :src="assetUrl" :alt="segment.title || t('engine.attachment')" class="max-w-full max-h-96 rounded border border-border" />
    <button v-else type="button" class="text-xs text-primary hover:underline disabled:opacity-50" :disabled="loadingAsset" @click="loadAsset">
      {{ loadingAsset ? t('common.loading') : (segment.title || t('engine.loadAttachment')) }}
    </button>
    <p v-if="assetError" role="alert" class="mt-1 text-xs text-destructive">{{ assetError }}</p>
  </div>
  <pre v-else-if="segment.kind === 'commandExecution' || segment.kind === 'fileChange' || segment.kind === 'toolCall' || segment.kind === 'toolResult'" class="mt-1 overflow-x-auto rounded border border-border bg-muted/60 p-2 text-[11px] leading-relaxed whitespace-pre-wrap">{{ segmentText(segment) }}</pre>
  <p v-else-if="segment.kind === 'unknown'" class="mt-1 text-xs text-muted-foreground italic">{{ segmentText(segment) }}</p>
  <p v-else class="whitespace-pre-wrap break-words leading-relaxed">{{ segmentText(segment) }}</p>
</template>
