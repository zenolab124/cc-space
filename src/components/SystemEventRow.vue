<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionRecord } from '@/types'
import DividerMark from './DividerMark.vue'

const { t } = useI18n()

const props = defineProps<{
  record: Extract<SessionRecord, { type: 'system' }>
}>()

/** api_error 的可读文案：优先 formatted，回落 message */
const errorText = computed(() => {
  const err = props.record.error
  if (!err) return props.record.content ?? t('block.apiError')
  const formatted = err.formatted
  if (typeof formatted === 'string' && formatted) return formatted
  const message = err.message
  if (typeof message === 'string' && message) return message
  return t('block.apiError')
})

/** compact 前的 token 数（compactMetadata.preTokens） */
const preTokens = computed(() => {
  const v = props.record.compactMetadata?.preTokens
  return typeof v === 'number' ? v : null
})

/** compact 边界文案：边界标题 + 可选压缩前 token 数 */
const compactLabel = computed(() => {
  let label = t('block.compactBoundary')
  if (preTokens.value) label += t('block.compactTokens', { tokens: Math.round(preTokens.value / 1000) })
  return label
})
</script>

<template>
  <!-- compact 边界：分隔线形态（与渠道/模型切换、跨天分隔同款） -->
  <DividerMark
    v-if="record.subtype === 'compact_boundary'"
    icon="i-carbon-compress"
    :label="compactLabel"
    class="my-3"
  />

  <!-- API 错误：红色紧凑提示行（连续重试已在 messages 层折叠为末条） -->
  <div v-else-if="record.subtype === 'api_error'" class="my-2 rounded-md bg-destructive/5 border border-destructive/20 px-3 py-1.5 text-xs flex items-center gap-1.5 flex-wrap">
    <span class="i-carbon-warning-alt w-3.5 h-3.5 shrink-0 text-destructive" />
    <span class="text-destructive font-medium">{{ $t('block.apiError') }}</span>
    <span class="text-muted-foreground break-all">{{ errorText }}</span>
    <span v-if="record.retryAttempt" class="px-1.5 py-0.5 rounded border border-destructive/40 text-destructive/80 shrink-0">
      {{ $t('block.retryCount', { current: record.retryAttempt, max: record.maxRetries ? ` / ${record.maxRetries}` : '' }) }}
    </span>
  </div>

  <!-- 其余 subtype：灰色一行兜底 -->
  <div v-else class="my-2 text-xs text-muted-foreground flex items-center gap-1.5">
    <span class="i-carbon-information w-3.5 h-3.5 shrink-0" />
    <span class="font-mono">{{ record.subtype ?? $t('session.system') }}</span>
    <span v-if="record.content" class="text-muted-foreground">{{ record.content }}</span>
  </div>
</template>
