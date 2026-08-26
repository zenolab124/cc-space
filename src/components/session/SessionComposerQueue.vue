<script setup lang="ts">
import { nextTick, ref } from 'vue'

export type ComposerQueueStatus = 'pending' | 'processing' | 'failed'

export interface ComposerQueueItem {
  id: string
  text: string
  imageCount?: number
  detail?: string
  status?: ComposerQueueStatus
  actionLabel?: string
  actionTitle?: string
  actionDisabled?: boolean
}

defineProps<{
  items: ComposerQueueItem[]
}>()

const emit = defineEmits<{
  (event: 'remove', id: string): void
  (event: 'update', id: string, text: string): void
  (event: 'process', id: string): void
}>()

const editingId = ref<string | null>(null)
const editingText = ref('')
const editField = ref<HTMLTextAreaElement | null>(null)

function bindEditField(element: unknown) {
  editField.value = element instanceof HTMLTextAreaElement ? element : null
}

function beginEdit(item: ComposerQueueItem) {
  if (item.status === 'processing') return
  editingId.value = item.id
  editingText.value = item.text
  void nextTick(() => {
    editField.value?.focus()
    editField.value?.select()
  })
}

function cancelEdit() {
  editingId.value = null
  editingText.value = ''
}

function saveEdit(item: ComposerQueueItem) {
  const text = editingText.value.trim()
  if (!text && !item.imageCount) return
  emit('update', item.id, text)
  cancelEdit()
}

function statusKey(status: ComposerQueueStatus | undefined): string {
  if (status === 'processing') return 'session.queueProcessing'
  if (status === 'failed') return 'session.queueFailed'
  return 'session.queuePending'
}
</script>

<template>
  <div v-if="items.length" class="mb-2 flex flex-col gap-1" role="list" :aria-label="$t('session.queueLabel')">
    <div
      v-for="item in items"
      :key="item.id"
      class="group rounded-md border border-border/60 bg-muted/60 px-2.5 py-1.5 text-xs"
      :class="item.status === 'failed' && 'border-destructive/35 bg-destructive/5'"
      role="listitem"
    >
      <div v-if="editingId === item.id" class="flex items-start gap-1.5">
        <textarea
          :ref="bindEditField"
          v-model="editingText"
          rows="2"
          class="min-h-12 flex-1 resize-y rounded border border-ring/60 bg-popover px-2 py-1.5 text-xs leading-relaxed text-foreground outline-none"
          :aria-label="$t('session.queueEditLabel')"
          @keydown.meta.enter.prevent="saveEdit(item)"
          @keydown.ctrl.enter.prevent="saveEdit(item)"
          @keydown.esc.prevent="cancelEdit"
        />
        <div class="flex shrink-0 items-center gap-1">
          <button
            type="button"
            class="h-7 rounded border border-border px-2 text-[11px] text-muted-foreground transition-colors hover:bg-card hover:text-foreground"
            @click="cancelEdit"
          >
            {{ $t('common.cancel') }}
          </button>
          <button
            type="button"
            class="h-7 rounded bg-primary px-2 text-[11px] text-primary-foreground transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
            :disabled="!editingText.trim() && !item.imageCount"
            @click="saveEdit(item)"
          >
            {{ $t('common.save') }}
          </button>
        </div>
      </div>

      <div v-else class="flex items-center gap-1.5">
        <span
          class="h-3 w-3 shrink-0 text-muted-foreground"
          :class="item.status === 'processing' ? 'i-carbon-circle-dash animate-spin' : item.status === 'failed' ? 'i-carbon-warning-alt text-destructive' : 'i-carbon-time'"
          aria-hidden="true"
        />
        <div class="min-w-0 flex-1">
          <div class="flex min-w-0 items-center gap-1.5">
            <span class="truncate text-muted-foreground">
              {{ item.text || (item.imageCount ? $t('image.dropHint') : '') }}
            </span>
            <span v-if="item.imageCount" class="shrink-0 text-[10px] text-muted-foreground">
              {{ item.imageCount }} ×
            </span>
          </div>
          <div class="mt-0.5 flex items-center gap-1.5 text-[10px] leading-tight">
            <span :class="item.status === 'failed' ? 'text-destructive' : 'text-muted-foreground/75'">
              {{ $t(statusKey(item.status)) }}
            </span>
            <span v-if="item.detail" class="truncate text-muted-foreground/65">· {{ item.detail }}</span>
          </div>
        </div>
        <button
          type="button"
          class="h-7 rounded border border-border px-2 text-[11px] text-muted-foreground transition-colors hover:bg-card hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="item.status === 'processing'"
          @click="beginEdit(item)"
        >
          {{ $t('common.edit') }}
        </button>
        <button
          v-if="item.actionLabel"
          type="button"
          class="h-7 rounded border border-primary/35 bg-primary/8 px-2 text-[11px] text-primary transition-colors hover:bg-primary/14 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="item.actionDisabled || item.status === 'processing'"
          :title="item.actionTitle"
          @click="emit('process', item.id)"
        >
          {{ item.actionLabel }}
        </button>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded text-muted-foreground/60 transition-colors hover:bg-destructive/10 hover:text-destructive disabled:cursor-not-allowed disabled:opacity-30"
          :disabled="item.status === 'processing'"
          :title="$t('common.delete')"
          :aria-label="$t('common.delete')"
          @click="emit('remove', item.id)"
        >
          <span class="i-carbon-close h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </div>
    </div>
  </div>
</template>
