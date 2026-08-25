<script setup lang="ts">
import { computed, ref, inject, type ComputedRef } from 'vue'
import { flattenResultText, type ToolResultData } from '@/utils/toolPair'

const MCP_NAME_RE = /^mcp__([^_]+(?:_[^_]+)*?)__(.+)$/

const props = defineProps<{
  input: Record<string, unknown>
  toolUseId: string
  name: string
}>()

const parsed = computed(() => {
  const m = props.name.match(MCP_NAME_RE)
  if (!m) return null
  return { server: m[1], tool: m[2] }
})

const inputJson = computed(() => {
  try {
    return JSON.stringify(props.input, null, 2)
  } catch {
    return String(props.input)
  }
})

const toolResultMap = inject<ComputedRef<Map<string, ToolResultData>>>('toolResultMap')
const result = computed(() => toolResultMap?.value.get(props.toolUseId))

const resultText = computed(() => {
  if (!result.value) return ''
  return flattenResultText(result.value.content)
})

const expanded = ref(false)
const outputExpanded = ref(false)
</script>

<template>
  <div class="mt-2 rounded-md bg-background border border-border px-3 py-2 text-xs">
    <button
      class="flex items-center gap-1.5 w-full text-left flex-wrap"
      @click="expanded = !expanded"
    >
      <span class="i-carbon-chevron-right w-3 h-3 transition-transform shrink-0" :class="{ 'rotate-90': expanded }" />
      <span class="i-carbon-plug w-3.5 h-3.5 shrink-0" />
      <span class="text-foreground font-medium">mcp</span>
      <template v-if="parsed">
        <span class="text-muted-foreground">·</span>
        <code class="px-1.5 py-0.5 rounded bg-muted text-muted-foreground font-mono">{{ parsed.server }}</code>
        <span class="text-muted-foreground">·</span>
        <code class="px-1.5 py-0.5 rounded border border-border text-muted-foreground font-mono">{{ parsed.tool }}</code>
      </template>
      <code v-else class="px-1.5 py-0.5 rounded bg-muted text-muted-foreground font-mono">{{ name }}</code>
    </button>
    <pre v-if="expanded" class="mt-2 rounded bg-muted px-2 py-1 text-muted-foreground whitespace-pre-wrap break-all font-mono max-h-96 overflow-y-auto">{{ inputJson }}</pre>

    <!-- 结果文本 -->
    <div v-if="resultText" class="mt-1.5 pt-1.5 border-t border-border/40 flex items-start gap-1.5">
      <span
        class="shrink-0 font-mono text-2xs mt-px"
        :class="result?.is_error ? 'text-destructive/60' : 'text-muted-foreground/60'"
      >{{ $t('common.output') }}</span>
      <pre
        class="flex-1 min-w-0 font-mono whitespace-pre-wrap break-all cursor-pointer select-text"
        :class="[
          result?.is_error ? 'text-destructive' : 'text-muted-foreground',
          outputExpanded ? '' : 'line-clamp-3',
        ]"
        @click="outputExpanded = !outputExpanded"
      >{{ resultText }}</pre>
    </div>
  </div>
</template>

<style scoped>
.line-clamp-3 {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 3;
  overflow: hidden;
}
</style>
