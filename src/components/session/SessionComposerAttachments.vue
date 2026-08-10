<script setup lang="ts">
import type { PendingImage } from '@/composables/useImageInput'

defineProps<{
  images: PendingImage[]
  dragging?: boolean
  error?: string | null
}>()

const emit = defineEmits<{
  (event: 'remove', id: string): void
}>()
</script>

<template>
  <div v-if="dragging" class="pointer-events-none mb-1 flex items-center gap-1.5 text-xs text-primary">
    <span class="i-carbon-image h-3.5 w-3.5" />
    {{ $t('image.dropHint') }}
  </div>

  <div v-if="images.length" class="mb-2 flex flex-wrap gap-2">
    <div
      v-for="image in images"
      :key="image.id"
      class="group relative h-14 w-14 overflow-hidden rounded border border-border"
    >
      <img :src="image.dataUrl" class="h-full w-full object-cover" alt="" />
      <button
        type="button"
        class="absolute right-0 top-0 flex h-4 w-4 items-center justify-center rounded-bl bg-destructive/80 text-[10px] leading-none text-destructive-foreground opacity-0 transition-opacity group-hover:opacity-100"
        :aria-label="$t('common.delete')"
        @click="emit('remove', image.id)"
      >
        &times;
      </button>
    </div>
  </div>

  <div v-if="error" class="mb-1 text-xs text-destructive">
    {{ error }}
  </div>
</template>
