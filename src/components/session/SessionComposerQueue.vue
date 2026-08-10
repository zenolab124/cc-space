<script setup lang="ts">
export interface ComposerQueueItem {
  id: string
  text: string
  imageCount?: number
}

defineProps<{
  items: ComposerQueueItem[]
}>()

const emit = defineEmits<{
  (event: 'remove', id: string): void
}>()
</script>

<template>
  <div v-if="items.length" class="mb-2 flex flex-col gap-1">
    <div
      v-for="item in items"
      :key="item.id"
      class="group flex items-center gap-1.5 rounded-md border border-border/50 bg-muted/60 px-2.5 py-1.5 text-xs"
    >
      <span class="i-carbon-time h-3 w-3 shrink-0 text-muted-foreground" />
      <span class="min-w-0 flex-1 truncate text-muted-foreground">
        {{ item.text || (item.imageCount ? $t('image.dropHint') : '') }}
      </span>
      <span v-if="item.imageCount" class="shrink-0 text-[10px] text-muted-foreground">
        {{ item.imageCount }} ×
      </span>
      <button
        type="button"
        class="i-carbon-close h-3 w-3 shrink-0 text-muted-foreground/50 opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100"
        :title="$t('common.delete')"
        @click="emit('remove', item.id)"
      />
    </div>
  </div>
</template>
