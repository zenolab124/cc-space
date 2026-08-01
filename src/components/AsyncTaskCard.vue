<script setup lang="ts">
import { computed, ref, watch, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { asyncTaskStopId, isActive, type AsyncTaskItem, type AsyncSpecies, type AsyncTaskState } from '@/composables/useAsyncTasks'
import type { SubAgentMeta } from '@/types'
import { formatTokens } from '@/types'

const props = defineProps<{
  task: AsyncTaskItem
  /** 面板级时钟（30s tick），waiting 倒计时用 */
  now: number
  stopping?: boolean
}>()

const emit = defineEmits<{
  open: [task: AsyncTaskItem]
  openChild: [meta: SubAgentMeta]
  locate: [toolUseId: string]
  stop: [task: AsyncTaskItem]
}>()

const { t } = useI18n()

// 物种注册表：图标 + 标签 + 角标配色。新物种未注册时落 generic 视觉
const SPECIES_META: Record<AsyncSpecies, { icon: string; label: string; cls: string }> = {
  bash: { icon: 'i-carbon-terminal', label: 'Shell', cls: 'bg-tag text-tag-foreground' },
  agent: { icon: 'i-carbon-bot', label: 'Agent', cls: 'bg-tag text-tag-foreground' },
  workflow: { icon: 'i-carbon-flow', label: 'Workflow', cls: 'bg-claude/15 text-claude' },
  monitor: { icon: 'i-carbon-view', label: 'Monitor', cls: 'bg-tag text-tag-foreground' },
  wakeup: { icon: 'i-carbon-alarm', label: 'Timer', cls: 'bg-tag text-tag-foreground' },
  generic: { icon: 'i-carbon-circle-dash', label: 'Task', cls: 'bg-tag text-tag-foreground' },
}

const STATE_DOT: Record<AsyncTaskState, string> = {
  running: 'bg-green-500 animate-pulse',
  waiting: 'bg-blue-500 animate-pulse',
  completed: 'bg-muted-foreground/40',
  failed: 'bg-destructive',
  killed: 'bg-orange-400',
  unknown: 'bg-yellow-500',
}

const species = computed(() => SPECIES_META[props.task.species] ?? SPECIES_META.generic)
const canStop = computed(() => asyncTaskStopId(props.task) !== null)

// 活跃任务秒级计时器
const elapsed = ref(0)
let elapsedTimer: ReturnType<typeof setInterval> | null = null

function startTimer() {
  stopTimer()
  if (!isActive(props.task) || !props.task.startedAt) return
  const t0 = new Date(props.task.startedAt).getTime()
  elapsed.value = Math.max(0, Math.round((Date.now() - t0) / 1000))
  elapsedTimer = setInterval(() => { elapsed.value++ }, 1000)
}
function stopTimer() {
  if (elapsedTimer) { clearInterval(elapsedTimer); elapsedTimer = null }
}
watch(() => [props.task.state, props.task.startedAt] as const, () => {
  if (isActive(props.task)) startTimer(); else stopTimer()
}, { immediate: true })
onUnmounted(stopTimer)

/** 元信息：耗时 / tokens / 退出码 / 唤醒倒计时，按可用性拼接 */
const metaLine = computed(() => {
  const parts: string[] = []
  const u = props.task.usage
  if (isActive(props.task) && props.task.startedAt) {
    parts.push(formatDuration(elapsed.value * 1000))
  } else if (u?.durationMs) {
    parts.push(formatDuration(u.durationMs))
  }
  if (u?.tokens) parts.push(`${formatTokens(u.tokens)} tok`)
  if (props.task.exitCode !== null && props.task.state === 'failed') {
    parts.push(t('asyncTask.exitCode', { code: props.task.exitCode }))
  }
  if (props.task.state === 'waiting' && props.task.scheduledFor) {
    const mins = Math.max(0, Math.round((props.task.scheduledFor - props.now) / 60000))
    parts.push(t('asyncTask.countdown', { min: mins }))
  }
  return parts.join(' · ')
})

function formatDuration(ms: number): string {
  if (ms < 60_000) return `${Math.round(ms / 1000)}s`
  return `${Math.floor(ms / 60_000)}m${Math.round((ms % 60_000) / 1000)}s`
}
</script>

<template>
  <div class="overflow-hidden">
    <div
      class="px-2.5 py-2 rounded text-[11px]
             bg-background border border-border cursor-pointer
             transition-colors hover:bg-card hover:border-primary/40"
      @click="emit('open', task)"
    >
      <div class="flex items-center gap-1.5 mb-1">
        <span
          class="px-1 py-0.5 rounded text-[9px] font-semibold shrink-0 flex items-center gap-0.5"
          :class="species.cls"
        >
          <span :class="species.icon" class="w-2.5 h-2.5" />
          {{ species.label }}
        </span>
        <span
          class="w-1.5 h-1.5 rounded-full shrink-0"
          :class="STATE_DOT[task.state]"
          :title="t(`asyncTask.state.${task.state}`)"
        />
        <span v-if="metaLine" class="text-[9px] text-muted-foreground/70 tabular-nums truncate">{{ metaLine }}</span>
        <span class="flex-1" />
        <button
          v-if="canStop"
          class="w-5 h-5 flex items-center justify-center rounded text-destructive
                 hover:bg-destructive/10 focus-visible:outline-none focus-visible:ring-2
                 focus-visible:ring-destructive/40 disabled:opacity-50 disabled:cursor-wait transition-colors"
          :title="stopping ? t('asyncTask.stopping') : t('asyncTask.stop')"
          :aria-label="stopping ? t('asyncTask.stopping') : t('asyncTask.stop')"
          :disabled="stopping"
          @click.stop="emit('stop', task)"
        >
          <span :class="stopping ? 'i-carbon-circle-dash animate-spin' : 'i-carbon-stop-filled'" class="w-3 h-3" />
        </button>
        <button
          v-if="task.toolUseId"
          class="w-4 h-4 flex items-center justify-center rounded hover:bg-muted transition-colors text-muted-foreground hover:text-foreground"
          :title="t('asyncTask.locate')"
          :aria-label="t('asyncTask.locate')"
          @click.stop="emit('locate', task.toolUseId!)"
        >
          <span class="i-carbon-location w-3 h-3" />
        </button>
      </div>
      <div class="text-foreground leading-relaxed line-clamp-2">{{ task.title || task.detail || task.key }}</div>
      <!-- 活跃任务不确定进度条 -->
      <div v-if="isActive(task)" class="mt-1.5 h-0.5 rounded-full bg-border overflow-hidden">
        <div class="h-full w-1/3 rounded-full bg-claude/60 animate-shimmer" />
      </div>
    </div>
    <!-- Workflow 子 Agent 折叠列表 -->
    <div v-if="task.species === 'workflow' && task.children.length" class="ml-6 mt-0.5 space-y-0.5">
      <div
        v-for="child in task.children"
        :key="child.agent_id"
        class="flex gap-1.5 px-2 py-1.5 rounded bg-background text-[10px]
               text-muted-foreground cursor-pointer hover:text-foreground hover:bg-card transition-colors"
        @click.stop="emit('openChild', child)"
      >
        <span class="w-1.5 h-1.5 rounded-full bg-primary shrink-0 mt-1" />
        <span class="flex-1 leading-relaxed">{{ child.description || child.agent_type || child.agent_id }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.animate-shimmer {
  animation: shimmer 1.8s ease-in-out infinite;
}
@keyframes shimmer {
  0% { transform: translateX(-100%); }
  100% { transform: translateX(400%); }
}
</style>
