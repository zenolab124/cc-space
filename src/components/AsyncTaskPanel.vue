<script setup lang="ts">
import { ref, computed, provide, watch, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SubAgentState } from '@/composables/useSubAgents'
import type { AsyncTaskItem, AsyncTaskState } from '@/composables/useAsyncTasks'
import { asyncTaskStopId, isActive } from '@/composables/useAsyncTasks'
import { TOOL_FOLD_INTERACTION, provideToolFoldState } from '@/composables/useToolDisplay'
import type { SubAgentMeta, SessionRecord, ContentBlock } from '@/types'
import { shortModel, formatTokens, hasReportedUsage } from '@/types'
import { filterConsumedResults } from '@/utils/toolPair'
import { IMAGE_LOCATOR, type ImageLocator } from '@/utils/ccimg'
import AsyncTaskCard from './AsyncTaskCard.vue'
import AsyncTaskDetail from './AsyncTaskDetail.vue'
import ContentBlockList from './ContentBlockList.vue'
import MsgClamp from './MsgClamp.vue'
import UserMsgContent from './UserMsgContent.vue'

const props = defineProps<{
  tasks: AsyncTaskItem[]
  openTabs: SubAgentState[]
  activeTabId: string | null
  projectId?: string | null
  sessionId?: string | null
  stoppingTaskIds?: ReadonlySet<string>
}>()

const emit = defineEmits<{
  selectAgent: [meta: SubAgentMeta]
  closeTab: [agentId: string]
  close: []
  locate: [toolUseId: string]
  stop: [task: AsyncTaskItem]
}>()

const { t } = useI18n()
const toolFoldState = provideToolFoldState()
provide(TOOL_FOLD_INTERACTION, () => {})

const imageLocator = computed<ImageLocator | null>(() => {
  if (!props.projectId || !props.sessionId || !props.activeTabId) return null
  return { projectId: props.projectId, sessionId: props.sessionId, agentId: props.activeTabId }
})
provide(IMAGE_LOCATOR, imageLocator)

const activeAgent = computed(() =>
  props.openTabs.find(a => a.meta.agent_id === props.activeTabId) ?? null,
)

// 三视图：列表 / agent 对话流 / 非 agent 任务详情
const view = ref<'list' | 'agent' | 'task'>('list')
const selectedKey = ref<string | null>(null)
const selectedTask = computed(() =>
  props.tasks.find(x => x.key === selectedKey.value) ?? null,
)
const headerTask = computed(() => view.value === 'list' ? null : selectedTask.value)

function isStopping(task: AsyncTaskItem): boolean {
  const id = asyncTaskStopId(task)
  return id !== null && (props.stoppingTaskIds?.has(id) ?? false)
}

// 会话切换：面板实例跨会话复用（档案馆单 SessionDetail），视图状态必须归位，
// 否则残留的 view='task'/'agent' 会因 key 失配落进空白 agent 分支
watch(() => props.sessionId, () => {
  view.value = 'list'
  selectedKey.value = null
  toolFoldState.reset()
})
// 任务重算后 key 失配（孤儿 key 漂移/records 换血）：自动回列表兜底
watch(selectedTask, (t) => {
  if (view.value === 'task' && !t) view.value = 'list'
})

// ---- 两段式分区 ----
const activeTasks = computed(() => props.tasks.filter(isActive))
const finishedTasks = computed(() => {
  const done = props.tasks.filter(x => !isActive(x))
  // 已结束按结束时刻倒序（无结束时刻的排最后，按启动时刻倒序兜底）
  return done.sort((a, b) =>
    (b.endedAt ?? b.startedAt ?? '').localeCompare(a.endedAt ?? a.startedAt ?? ''))
})

// waiting 倒计时时钟：面板级 30s tick，全部卡片共享
const now = ref(Date.now())
let ticker: ReturnType<typeof setInterval> | null = null
onMounted(() => { ticker = setInterval(() => { now.value = Date.now() }, 30_000) })
onUnmounted(() => { if (ticker) clearInterval(ticker) })

function openAgentTranscript(meta: SubAgentMeta) {
  emit('selectAgent', meta)
  view.value = 'agent'
}

function onOpenTask(task: AsyncTaskItem) {
  selectedKey.value = task.key
  // agent 物种且已知转录 id → 对话流；其余（含流式中未拿到 agentId 的）→ 任务详情
  if (task.species === 'agent' && task.agentId) {
    openAgentTranscript({
      agent_id: task.agentId,
      tool_use_id: task.toolUseId ?? '',
      agent_type: task.model ? `Agent (${shortModel(task.model)})` : 'Agent',
      description: task.title,
    })
    return
  }
  view.value = 'task'
}

/** 从主对话 Workflow/Agent 卡片直达：按 toolUseId 打开对应条目（SessionDetail 调用） */
function openByToolUse(toolUseId: string): boolean {
  const task = props.tasks.find(x => x.toolUseId === toolUseId)
  if (!task) return false
  onOpenTask(task)
  return true
}
defineExpose({ openByToolUse })

function backToList() {
  view.value = 'list'
}

const headerTitle = computed(() => {
  if (view.value === 'list') return t('asyncTask.title')
  if (view.value === 'agent') return activeAgent.value?.meta.description || activeAgent.value?.meta.agent_type || 'Agent'
  return selectedTask.value?.title || t('asyncTask.title')
})

/** 详情头部状态徽章 */
const STATE_BADGE: Record<AsyncTaskState, string> = {
  running: 'bg-green-500/15 text-green-600 dark:text-green-400',
  waiting: 'bg-blue-500/15 text-blue-600 dark:text-blue-400',
  completed: 'bg-muted text-muted-foreground',
  failed: 'bg-destructive/15 text-destructive',
  killed: 'bg-orange-400/15 text-orange-500',
  unknown: 'bg-yellow-500/15 text-yellow-600 dark:text-yellow-400',
}

// ---- agent 对话流分组（与主对话同一套渲染） ----

interface MessageGroup {
  user: SessionRecord | null
  responses: SessionRecord[]
}

function contentBlocks(record: Extract<SessionRecord, { type: 'user' | 'assistant' }>): ContentBlock[] {
  if (!record.message) return []
  if (record.type === 'user') {
    const content = record.message.content
    return typeof content === 'string'
      ? [{ type: 'text', text: content }]
      : content
  }
  return filterConsumedResults(record.message.content)
}

function groupMessages(records: SessionRecord[]): MessageGroup[] {
  const groups: MessageGroup[] = []
  let current: MessageGroup | null = null
  for (const record of records) {
    if (record.type === 'user') {
      current = { user: record, responses: [] }
      groups.push(current)
    } else if (record.type === 'assistant' || record.type === 'system') {
      if (!current) {
        current = { user: null, responses: [] }
        groups.push(current)
      }
      current.responses.push(record)
    }
  }
  return groups
}

const messageGroups = computed<MessageGroup[]>(() =>
  activeAgent.value ? groupMessages(activeAgent.value.records) : [],
)
</script>

<template>
  <div class="async-panel-root h-full flex flex-col bg-card">
    <!-- Header -->
    <div class="shrink-0 h-10 flex items-center px-3 border-b border-border gap-2">
      <button
        v-if="view !== 'list'"
        class="w-5 h-5 flex items-center justify-center rounded hover:bg-muted transition-colors"
        @click="backToList"
        :aria-label="t('common.back')"
      >
        <span class="i-carbon-chevron-left w-3.5 h-3.5" />
      </button>
      <span v-if="view === 'list'" class="i-carbon-lightning w-3.5 h-3.5 text-claude shrink-0" />
      <span class="text-xs font-semibold flex-1 truncate">{{ headerTitle }}</span>
      <span
        v-if="view === 'task' && selectedTask"
        class="px-1.5 py-0.5 rounded-full text-[9px] font-semibold shrink-0"
        :class="STATE_BADGE[selectedTask.state]"
      >{{ t(`asyncTask.state.${selectedTask.state}`) }}</span>
      <span
        v-if="view === 'list'"
        class="px-1.5 py-0.5 rounded-full text-[10px] font-semibold bg-claude/15 text-claude tabular-nums"
      >{{ tasks.length }}</span>
      <button
        v-if="headerTask && asyncTaskStopId(headerTask)"
        class="w-6 h-6 flex items-center justify-center rounded text-destructive
               hover:bg-destructive/10 focus-visible:outline-none focus-visible:ring-2
               focus-visible:ring-destructive/40 disabled:opacity-50 disabled:cursor-wait transition-colors"
        :title="isStopping(headerTask) ? t('asyncTask.stopping') : t('asyncTask.stop')"
        :aria-label="isStopping(headerTask) ? t('asyncTask.stopping') : t('asyncTask.stop')"
        :disabled="isStopping(headerTask)"
        @click="emit('stop', headerTask)"
      >
        <span :class="isStopping(headerTask) ? 'i-carbon-circle-dash animate-spin' : 'i-carbon-stop-filled'" class="w-3.5 h-3.5" />
      </button>
      <button
        class="w-5 h-5 flex items-center justify-center rounded hover:bg-muted transition-colors"
        :title="t('common.close')"
        :aria-label="t('common.close')"
        @click="emit('close')"
      >
        <span class="i-carbon-close w-3 h-3" />
      </button>
    </div>

    <!-- List View：进行中 / 已结束 两段式 -->
    <div v-if="view === 'list'" class="flex-1 min-h-0 overflow-y-auto p-2 space-y-1.5 overscroll-contain">
      <template v-if="activeTasks.length">
        <div class="flex items-center gap-1.5 px-0.5 pt-0.5">
          <span class="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />
          <span class="text-[10px] font-semibold text-muted-foreground">{{ t('asyncTask.sectionActive') }}</span>
          <span class="text-[10px] text-muted-foreground/60 tabular-nums">{{ activeTasks.length }}</span>
        </div>
        <AsyncTaskCard
          v-for="task in activeTasks"
          :key="task.key"
          :task="task"
          :now="now"
          :stopping="isStopping(task)"
          @open="onOpenTask"
          @open-child="openAgentTranscript"
          @locate="emit('locate', $event)"
          @stop="emit('stop', $event)"
        />
      </template>

      <template v-if="finishedTasks.length">
        <div class="flex items-center gap-1.5 px-0.5" :class="activeTasks.length ? 'pt-2' : ''">
          <span class="text-[10px] font-semibold text-muted-foreground">{{ t('asyncTask.sectionFinished') }}</span>
          <span class="text-[10px] text-muted-foreground/60 tabular-nums">{{ finishedTasks.length }}</span>
        </div>
        <AsyncTaskCard
          v-for="task in finishedTasks"
          :key="task.key"
          :task="task"
          :now="now"
          :stopping="isStopping(task)"
          @open="onOpenTask"
          @open-child="openAgentTranscript"
          @locate="emit('locate', $event)"
          @stop="emit('stop', $event)"
        />
      </template>

      <div v-if="tasks.length === 0" class="text-center text-muted-foreground text-xs py-8">
        {{ t('asyncTask.empty') }}
      </div>
    </div>

    <!-- Task Detail View（bash/monitor/wakeup/workflow/generic）-->
    <AsyncTaskDetail
      v-else-if="view === 'task' && selectedTask"
      :task="selectedTask"
      @open-child="openAgentTranscript"
    />

    <!-- Agent Transcript View -->
    <template v-else>
      <div v-if="!activeAgent" class="flex-1 flex items-center justify-center">
        <p class="text-muted-foreground text-xs">{{ t('asyncTask.empty') }}</p>
      </div>

      <div v-else-if="activeAgent.loading" class="flex-1 flex items-center justify-center">
        <p class="text-muted-foreground text-xs">{{ t('session.loadingChat') }}</p>
      </div>

      <div v-else class="flex-1 min-h-0 overflow-y-auto px-3 py-2 space-y-3 overscroll-contain">
        <div v-for="(group, gi) in messageGroups" :key="gi" class="space-y-3">
          <div v-if="group.user && group.user.type === 'user'">
            <div class="flex gap-2">
              <div class="w-0.5 shrink-0 rounded-full bg-primary/60" />
              <div class="min-w-0 flex-1 bg-background border border-border rounded px-2.5 py-1.5">
                <div class="text-[10px] font-medium mb-0.5 text-primary">Prompt</div>
                <MsgClamp>
                  <UserMsgContent :blocks="contentBlocks(group.user as any)" :record-uuid="(group.user as any).uuid" />
                </MsgClamp>
              </div>
            </div>
          </div>
          <template v-for="resp in group.responses" :key="(resp as any).uuid || resp">
            <div v-if="resp.type === 'assistant'" class="flex gap-2">
              <div class="w-0.5 shrink-0 rounded-full bg-claude/60" />
              <div class="min-w-0 flex-1">
                <div class="text-[10px] font-medium mb-0.5 text-claude flex items-center gap-1">
                  <span>
                    Agent
                    <span v-if="(resp as any).message?.model" class="text-muted-foreground font-normal">
                      ({{ shortModel((resp as any).message.model) }})
                    </span>
                  </span>
                  <span v-if="hasReportedUsage((resp as any).message?.usage)" class="text-muted-foreground/70 font-normal tabular-nums">
                    {{ formatTokens((resp as any).message.usage.input_tokens) }} in
                    · {{ formatTokens((resp as any).message.usage.output_tokens) }} out
                  </span>
                </div>
                <div>
                  <ContentBlockList
                    :blocks="contentBlocks(resp as any)"
                    :record-uuid="(resp as any).uuid"
                  />
                </div>
              </div>
            </div>
          </template>
        </div>
      </div>
    </template>
  </div>
</template>
