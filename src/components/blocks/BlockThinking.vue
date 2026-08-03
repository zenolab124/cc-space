<script setup lang="ts">
import { computed } from 'vue'
import type { ContentBlock } from '@/types'
import { useThinkingExpand } from '@/composables/useThinkingExpand'

const props = defineProps<{
  block: Extract<ContentBlock, { type: 'thinking' }>
}>()

const { thinkingExpanded: expanded, toggle } = useThinkingExpand()

const hasPlainText = computed(() => props.block.thinking.length > 0)
const isRedacted = computed(() => !hasPlainText.value && !!props.block.signature)
const isThinking = computed(() => !hasPlainText.value && !props.block.signature)

const durationLabel = computed(() => {
  const ms = props.block._thinkingMs
  if (!ms || ms < 1000) return ''
  const s = Math.round(ms / 1000)
  if (s < 60) return `${s}s`
  return `${Math.floor(s / 60)}m ${s % 60}s`
})
</script>

<template>
  <div class="thinking-block mt-2 mb-1.5">
    <!-- 有明文:可折叠,显示字数 -->
    <template v-if="hasPlainText">
      <button
        class="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1"
        :aria-expanded="expanded"
        :title="$t('block.foldShiftHint')"
        @click="toggle"
      >
        <span class="i-carbon-chevron-right w-3 h-3 transition-transform" :class="{ 'rotate-90': expanded }" />
        {{ $t('block.thinking', { length: block.thinking.length }) }}
        <span v-if="durationLabel" class="text-muted-foreground/60">· {{ durationLabel }}</span>
      </button>
      <div v-if="expanded" class="mt-1 pl-3 border-l-2 border-border text-xs text-muted-foreground whitespace-pre-wrap">
        {{ block.thinking }}
      </div>
    </template>

    <!-- 流式中,signature/明文都尚未到达 -->
    <div v-else-if="isThinking" class="text-xs text-muted-foreground flex items-center gap-1">
      <span class="i-carbon-thinking w-3 h-3 shrink-0 animate-pulse" />
      {{ $t('block.thinkingLoading') }}
    </div>

    <!-- redacted:仅显示标识,不可展开(无明文可展) -->
    <div v-else-if="isRedacted" class="text-xs text-muted-foreground flex items-center gap-1">
      <span class="i-carbon-locked w-3 h-3 shrink-0" />
      {{ $t('block.thinkingRedacted') }}
      <span v-if="durationLabel" class="text-muted-foreground/60">· {{ durationLabel }}</span>
    </div>
  </div>
</template>
