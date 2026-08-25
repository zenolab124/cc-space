<script setup lang="ts">
import { ref } from 'vue'
import type { SessionFindStatus } from '@/utils/sessionFind'

defineProps<{
  query: string
  status: SessionFindStatus
}>()

const emit = defineEmits<{
  (event: 'update:query', query: string): void
  (event: 'previous'): void
  (event: 'next'): void
  (event: 'close'): void
}>()

const inputRef = ref<HTMLInputElement>()

function focus(select = false) {
  inputRef.value?.focus()
  if (select) inputRef.value?.select()
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter') {
    event.preventDefault()
    if (event.shiftKey) emit('previous')
    else emit('next')
  } else if (event.key === 'Escape') {
    event.preventDefault()
    event.stopPropagation()
    emit('close')
  }
}

defineExpose({ focus })
</script>

<template>
  <div
    class="shrink-0 flex items-center gap-1.5 border-b border-border bg-muted/45 px-2 py-1.5"
    role="search"
  >
    <label class="min-w-0 flex-1">
      <span class="sr-only">{{ $t('workbench.column.find') }}</span>
      <input
        ref="inputRef"
        :value="query"
        type="search"
        class="h-7 w-full rounded border border-input bg-background px-2 text-xs text-foreground outline-none placeholder:text-muted-foreground focus:border-ring"
        :placeholder="$t('workbench.column.findPlaceholder')"
        autocomplete="off"
        @input="emit('update:query', ($event.target as HTMLInputElement).value)"
        @keydown="onKeydown"
      />
    </label>
    <span class="min-w-11 text-center text-[10px] tabular-nums text-muted-foreground" aria-live="polite">
      {{ status.current }} / {{ status.total }}
    </span>
    <button
      type="button"
      class="icon-btn icon-btn-sm shrink-0"
      :disabled="status.total === 0"
      :title="$t('workbench.column.findPrevious')"
      :aria-label="$t('workbench.column.findPrevious')"
      @click="emit('previous')"
    ><span class="i-carbon-chevron-up w-3 h-3" /></button>
    <button
      type="button"
      class="icon-btn icon-btn-sm shrink-0"
      :disabled="status.total === 0"
      :title="$t('workbench.column.findNext')"
      :aria-label="$t('workbench.column.findNext')"
      @click="emit('next')"
    ><span class="i-carbon-chevron-down w-3 h-3" /></button>
    <button
      type="button"
      class="icon-btn icon-btn-sm shrink-0"
      :title="$t('common.close')"
      :aria-label="$t('common.close')"
      @click="emit('close')"
    ><span class="i-carbon-close w-3 h-3" /></button>
  </div>
</template>
