<script setup lang="ts">
import { computed } from 'vue'
import { formatTokens, type TokenUsage } from '@/types'

const props = defineProps<{
  totalTokens: TokenUsage
  subagentTokens?: TokenUsage | null
}>()

const totalInput = computed(() => props.totalTokens.input_tokens
  + props.totalTokens.cache_creation_input_tokens
  + props.totalTokens.cache_read_input_tokens)
const total = computed(() => totalInput.value + props.totalTokens.output_tokens)
const cacheHitRate = computed(() => totalInput.value > 0
  ? `${Math.round(props.totalTokens.cache_read_input_tokens / totalInput.value * 100)}%`
  : '—')
const cacheRatio = computed(() => total.value > 0
  ? `${Math.round(props.totalTokens.cache_read_input_tokens / total.value * 100)}%`
  : '—')
const subagentTotal = computed(() => {
  const usage = props.subagentTokens
  return usage
    ? usage.input_tokens + usage.output_tokens + usage.cache_creation_input_tokens + usage.cache_read_input_tokens
    : 0
})
</script>

<template>
  <div class="flex flex-col gap-0.5 pt-1 border-t border-border/50 tabular-nums">
    <div class="flex items-center justify-between"><span>input_tokens</span><span>{{ formatTokens(totalTokens.input_tokens) }}</span></div>
    <div class="flex items-center justify-between"><span>output_tokens</span><span>{{ formatTokens(totalTokens.output_tokens) }}</span></div>
    <div class="flex items-center justify-between"><span>cache_creation</span><span>{{ formatTokens(totalTokens.cache_creation_input_tokens) }}</span></div>
    <div class="flex items-center justify-between"><span>cache_read</span><span>{{ formatTokens(totalTokens.cache_read_input_tokens) }}</span></div>
    <div class="flex items-center justify-between pt-1 border-t border-border/50">
      <span>{{ $t('topbar.tokenTotalInput') }}</span><span>{{ formatTokens(totalInput) }}</span>
    </div>
    <div class="flex items-center justify-between">
      <span>{{ $t('topbar.tokenTotalOutput') }}</span><span>{{ formatTokens(totalTokens.output_tokens) }}</span>
    </div>
    <div class="flex items-center justify-between"><span>{{ $t('topbar.tokenCacheHitRate') }}</span><span>{{ cacheHitRate }}</span></div>
    <div class="flex items-center justify-between"><span>{{ $t('topbar.tokenCacheRatio') }}</span><span>{{ cacheRatio }}</span></div>
    <div class="flex items-center justify-between font-medium text-foreground">
      <span>{{ $t('topbar.tokenTotal') }}</span><span>{{ formatTokens(total) }}</span>
    </div>

    <template v-if="subagentTokens && subagentTotal > 0">
      <div class="flex items-center justify-between pt-1 border-t border-border/50 font-medium text-foreground">
        <span>{{ $t('topbar.tokenSubagents') }}</span><span>{{ formatTokens(subagentTotal) }}</span>
      </div>
      <div class="flex items-center justify-between pl-2"><span>input_tokens</span><span>{{ formatTokens(subagentTokens.input_tokens) }}</span></div>
      <div class="flex items-center justify-between pl-2"><span>output_tokens</span><span>{{ formatTokens(subagentTokens.output_tokens) }}</span></div>
      <div class="flex items-center justify-between pl-2"><span>cache_creation</span><span>{{ formatTokens(subagentTokens.cache_creation_input_tokens) }}</span></div>
      <div class="flex items-center justify-between pl-2"><span>cache_read</span><span>{{ formatTokens(subagentTokens.cache_read_input_tokens) }}</span></div>
    </template>
  </div>
</template>
