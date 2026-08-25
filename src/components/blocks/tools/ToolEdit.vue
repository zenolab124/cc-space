<script setup lang="ts">
import { computed, inject, ref } from 'vue'
import { fileName } from '@/utils/path'
import TextDiffViewer from '@/components/diff/TextDiffViewer.vue'
import { calculateTextDiffStats } from '@/utils/textDiff'

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

const oldString = computed(() => {
  const v = props.input.old_string
  return typeof v === 'string' ? v : ''
})

const newString = computed(() => {
  const v = props.input.new_string
  return typeof v === 'string' ? v : ''
})

const unifiedDiff = computed(() => {
  const v = props.input.unified_diff
  return typeof v === 'string' ? v : ''
})

const diffStats = computed(() => calculateTextDiffStats({
  oldText: oldString.value,
  newText: newString.value,
  unifiedDiff: unifiedDiff.value,
}))
const hasDiff = computed(() => !!(oldString.value || newString.value || unifiedDiff.value))

const replaceAll = computed(() => props.input.replace_all === true)

const expanded = ref(false)

// 直达文件账本(仅主对话入账的调用显示;子代理转录不入账,自然隐藏)
const hasLedgerAnchor = inject<(id: string) => boolean>('hasLedgerAnchor', () => false)
const openFileLedger = inject<(id: string) => boolean>('openFileLedger', () => false)
const inLedger = computed(() => hasLedgerAnchor(props.toolUseId))
</script>

<template>
  <div :data-tool-use-id="toolUseId" class="mt-2 rounded-md bg-background border border-border px-3 py-2 text-xs">
    <div
      class="flex items-center gap-1.5 w-full text-left cursor-pointer"
      @click="expanded = !expanded"
    >
      <span class="i-carbon-chevron-right w-3 h-3 transition-transform shrink-0" :class="{ 'rotate-90': expanded }" />
      <span class="i-carbon-edit w-3.5 h-3.5 shrink-0" />
      <span class="text-foreground font-medium">Edit</span>
      <span v-if="filePath" class="font-mono text-muted-foreground truncate" :title="filePath">{{ displayName }}</span>
      <span v-if="hasDiff" class="ml-1 flex items-center gap-1 text-2xs tabular-nums shrink-0">
        <span class="text-primary">+{{ diffStats.additions }}</span>
        <span class="text-destructive">−{{ diffStats.deletions }}</span>
      </span>
      <span v-if="replaceAll" class="ml-1 px-1.5 py-0.5 rounded border border-accent/50 text-accent text-2xs shrink-0">{{ $t('block.toolEdit.replaceAll') }}</span>
      <button
        v-if="inLedger"
        class="ml-auto p-0.5 rounded text-muted-foreground/60 hover:text-claude hover:bg-muted shrink-0"
        :title="$t('fileLedger.viewInLedger')"
        @click.stop="openFileLedger(toolUseId)"
      >
        <span class="i-carbon-catalog w-3.5 h-3.5" />
      </button>
    </div>
    <TextDiffViewer
      v-if="expanded && hasDiff"
      class="mt-2"
      :old-text="oldString"
      :new-text="newString"
      :unified-diff="unifiedDiff"
    />
  </div>
</template>
