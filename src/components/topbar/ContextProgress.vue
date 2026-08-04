<script setup lang="ts">
import { computed } from 'vue'
import type { EngineAccent } from '@/engines/presentation'
import { formatTokens } from '@/types'

const props = defineProps<{
  /** 已用 tokens */
  used: number
  /** 容量 */
  capacity: number
  /** 紧凑形态(单行顶栏用):条 + 百分比,完整数字进 title */
  compact?: boolean
  /** 引擎身份色；警示状态仍使用全局警示色。 */
  accent?: EngineAccent
}>()

/** 0..1 的占用率(防越界) */
const ratio = computed(() => {
  if (props.capacity <= 0) return 0
  const r = props.used / props.capacity
  if (r < 0) return 0
  if (r > 1) return 1
  return r
})

const percent = computed(() => Math.round(ratio.value * 100))

/** 警示等级:0 正常 / 1 橙(>=80%) / 2 红(>=95%) */
const level = computed<0 | 1 | 2>(() => {
  if (ratio.value >= 0.95) return 2
  if (ratio.value >= 0.8) return 1
  return 0
})

const barColorClass = computed(() => {
  switch (level.value) {
    case 2: return 'bg-destructive'
    case 1: return 'bg-accent'
    default: return 'bg-primary'
  }
})

const normalBarColor = computed(() => `var(--${props.accent ?? 'primary'})`)

const textColorClass = computed(() => {
  switch (level.value) {
    case 2: return 'text-destructive'
    case 1: return 'text-accent'
    default: return 'text-muted-foreground'
  }
})

const normalTextColor = computed(() => `var(--${props.accent ?? 'primary'})`)

const usedText = computed(() => formatTokens(props.used))
const capacityText = computed(() => formatTokens(props.capacity))
</script>

<template>
  <div
    class="flex items-center gap-1.5"
    :class="compact ? 'flex-1 min-w-0' : 'min-w-32 inline-flex'"
    :title="$t('topbar.contextUsage', { used: usedText, capacity: capacityText, percent })"
  >
    <!-- 进度条 -->
    <div
      class="relative h-1.5 rounded-full overflow-hidden bg-muted"
      :class="compact ? 'flex-1 min-w-0' : 'w-20 shrink-0'"
    >
      <div
        class="absolute inset-y-0 left-0 rounded-full transition-all duration-200"
        :class="barColorClass"
        :style="{ width: `${percent}%`, ...(level === 0 ? { backgroundColor: normalBarColor } : {}) }"
      />
    </div>
    <!-- 紧凑形态:仅百分比(将满时染警示色已足够提示) -->
    <span v-if="compact" class="text-xs tabular-nums shrink-0" :class="textColorClass" :style="level === 0 ? { color: normalTextColor } : undefined">{{ percent }}%</span>
    <!-- 完整形态:数字 + 百分比 + 将满提示 -->
    <template v-else>
      <span class="text-xs whitespace-nowrap" :class="textColorClass">
        {{ usedText }} / {{ capacityText }}
        <span class="text-muted-foreground">({{ percent }}%)</span>
      </span>
      <span v-if="level === 2" class="text-xs text-destructive flex items-center gap-1">
        <span class="i-carbon-warning-alt w-3 h-3" />
        {{ $t('topbar.contextNearFull') }}
      </span>
    </template>
  </div>
</template>
