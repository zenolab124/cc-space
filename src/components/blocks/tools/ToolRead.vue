<script setup lang="ts">
import { computed, inject } from 'vue'
import { fileName } from '@/utils/path'
import { openPath, showPathOpenMenu } from '@/composables/useFileOpener'

const props = defineProps<{
  input: Record<string, unknown>
  toolUseId: string
  name: string
}>()

const filePath = computed(() => {
  const v = props.input.file_path
  return typeof v === 'string' ? v : ''
})

const displayName = computed(() => fileName(filePath.value))

const offset = computed(() => {
  const v = props.input.offset
  return typeof v === 'number' ? v : null
})

const limit = computed(() => {
  const v = props.input.limit
  return typeof v === 'number' ? v : null
})

const lineRange = computed(() => {
  if (offset.value === null && limit.value === null) return ''
  const start = offset.value ?? 1
  const end = limit.value !== null ? start + limit.value : null
  return end !== null ? `L${start}-${end}` : `L${start}-`
})

async function openFile() {
  if (!filePath.value) return
  try {
    await openPath(filePath.value)
  } catch {
    // 静默降级
  }
}

// 直达文件账本(仅主对话入账的调用显示;子代理转录不入账,自然隐藏)
const hasLedgerAnchor = inject<(id: string) => boolean>('hasLedgerAnchor', () => false)
const openFileLedger = inject<(id: string) => boolean>('openFileLedger', () => false)
const inLedger = computed(() => hasLedgerAnchor(props.toolUseId))
</script>

<template>
  <div :data-tool-use-id="toolUseId" class="mt-2 rounded-md bg-background border border-border px-3 py-2 text-xs">
    <div class="flex items-center gap-1.5">
      <span class="i-carbon-document w-3.5 h-3.5 shrink-0" />
      <span class="text-foreground font-medium">Read</span>
      <button
        v-if="filePath"
        class="font-mono text-muted-foreground hover:text-primary hover:underline truncate transition-colors"
        :title="filePath"
        @click="openFile"
        @contextmenu="showPathOpenMenu($event, filePath)"
      >{{ displayName }}</button>
      <span v-if="lineRange" class="font-mono text-muted-foreground">{{ lineRange }}</span>
      <button
        v-if="inLedger"
        class="ml-auto p-0.5 rounded text-muted-foreground/60 hover:text-claude hover:bg-muted shrink-0"
        :title="$t('fileLedger.viewInLedger')"
        @click.stop="openFileLedger(toolUseId)"
      >
        <span class="i-carbon-catalog w-3.5 h-3.5" />
      </button>
    </div>
  </div>
</template>
