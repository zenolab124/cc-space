<script setup lang="ts">
import { ref, computed, watch, onUnmounted } from 'vue'
import { useDraggable, useDroppable } from '@dnd-kit/vue'
import { useI18n } from 'vue-i18n'
import { useProjects } from '@/composables/useProjects'
import { useWorkbench } from '@/composables/useWorkbench'
import { useSessionStream, useStreaming } from '@/composables/useStreaming'
import { useSessionStatus } from '@/composables/useSessionStatus'
import { queueForSession, usePermissionRequests, isInteractiveTool } from '@/composables/usePermissionRequests'
import { useNotifications, isInteractiveKind } from '@/composables/useNotifications'
import { useConfirm } from '@/composables/useConfirm'
import { displayTitle, formatTokens, relativeTime } from '@/types'
import { fileName } from '@/utils/path'
import { useSessionMeta } from '@/composables/useSessionMeta'
import { useRunners } from '@/composables/useRunners'
import { respondInteraction } from '@/engines/client'
import { useEngineRuntimeState } from '@/engines/runtimeState'

const { t } = useI18n()
const { getMeta } = useSessionMeta()

/**
 * 左列监控卡(FR-003):状态行 / 标题 / 尾部区 / 就地决策条 / meta 行。
 * 只消费 stream.tail 等轻量字段,不 mount 消息流组件树(NFR-001 渲染分级)。
 */
const props = defineProps<{
  sessionId: string
  expanded: boolean
}>()

const sid = computed(() => props.sessionId)

const { projects } = useProjects()
const { activeTab, expandSession, removeSession, flashSessionId, draftCwd, engineDraft } = useWorkbench()
const { retrySession } = useStreaming()
const { respondRequest } = usePermissionRequests()
const { notifyTransient, sessionTitle, dismissError } = useNotifications()
const { confirm } = useConfirm()

const stream = useSessionStream(sid)
const status = useSessionStatus(sid)
const engineRuntime = useEngineRuntimeState(sid)
const perms = queueForSession(sid)

// Runner 运行中计数（用于小徽标）
const { runningCount: getRunnerCount } = useRunners()
const runnerCount = computed(() => getRunnerCount(props.sessionId))
const headPerm = computed(() => perms.value[0] ?? null)
const engineInteraction = computed(() => engineRuntime.value.snapshot?.pendingInteractions[0] ?? null)
const displayTail = computed(() => engineRuntime.value.tail.length ? engineRuntime.value.tail : stream.value.tail)
const runtimeActive = computed(() => {
  const phase = engineRuntime.value.snapshot?.phase
  return phase === 'running' || phase === 'awaitingInteraction'
})

const summary = computed(() => {
  for (const p of projects.value) {
    const s = p.sessions.find(s => s.id === props.sessionId)
    if (s) return { summary: s, projectName: fileName(p.display_path) }
  }
  return null
})

/** 应用内新建未落盘的草稿:标题与项目名占位 */
const draft = computed(() => {
  if (summary.value) return null
  const genericDraft = engineDraft(props.sessionId)
  if (genericDraft) return { projectName: fileName(genericDraft.cwd), engineName: genericDraft.engineName }
  const cwd = draftCwd(props.sessionId)
  return cwd ? { projectName: fileName(cwd), engineName: null } : null
})

const title = computed(() =>
  summary.value ? displayTitle(summary.value.summary, getMeta(props.sessionId)?.title)
  : draft.value ? t('session.newSessionTitle')
  : props.sessionId.slice(0, 8),
)

// --- 秒级时钟:流式持续时间显示用,空闲时停表 ---

const now = ref(Date.now())
let timer: number | null = null

watch(
  () => stream.value.streaming || runtimeActive.value,
  (active) => {
    if (active && timer === null) {
      timer = window.setInterval(() => {
        now.value = Date.now()
      }, 1000)
    } else if (!active && timer !== null) {
      clearInterval(timer)
      timer = null
    }
  },
  { immediate: true },
)

onUnmounted(() => {
  if (timer !== null) clearInterval(timer)
})

/** 决策条单行摘要:工具名 + 目标 */
const permSummary = computed(() => {
  const p = headPerm.value
  if (!p) return ''
  if (p.toolName === 'AskUserQuestion') return t('workbench.monitor.claudeAsking')
  if (p.toolName === 'ExitPlanMode') return t('workbench.monitor.planApproval')
  if (p.toolName === 'EnterPlanMode') return t('workbench.monitor.enterPlanMode')
  for (const k of ['file_path', 'command', 'url', 'pattern']) {
    const v = p.input[k]
    if (typeof v === 'string' && v) return `${p.toolName} · ${v.split('\n')[0]}`
  }
  return p.toolName
})

/** 交互工具(提问/计划)无法就地允许/拒绝,决策条改为引导去会话作答 */
const headPermInteractive = computed(
  () => !!headPerm.value && isInteractiveTool(headPerm.value.toolName),
)

/** 引擎交互同理:提问/计划类要去会话内作答,就地放行只会送出空答 */
const engineInteractionInteractive = computed(
  () => !!engineInteraction.value && isInteractiveKind(engineInteraction.value.kind),
)

/** 出错重试条(stream 源且有可重发消息) */
const canRetry = computed(() => status.value.key === 'error' && !!stream.value.lastSent)

// --- meta:持续时间(运行中)或最后活动时间 ---

const durationText = computed(() => {
  const runtimeStartedAt = engineRuntime.value.startedAt
  const activeSince = runtimeActive.value ? runtimeStartedAt : stream.value.streaming ? stream.value.startedAt : null
  if (activeSince) {
    const mins = Math.floor((now.value - activeSince) / 60_000)
    return mins < 1 ? t('time.justStarted') : t('time.nMinutes', { n: mins })
  }
  return summary.value ? relativeTime(summary.value.summary.last_modified) : ''
})

const tokenText = computed(() => {
  const t = summary.value?.summary.total_tokens
  if (!t) return null
  const total = t.input_tokens + t.output_tokens + t.cache_creation_input_tokens + t.cache_read_input_tokens
  return total > 0 ? formatTokens(total) : null
})

// --- 交互 ---

/** 点击卡片:未展开→展开;已展开→右区滚动聚焦(幂等) */
function onCardClick() {
  const result = expandSession(activeTab.value.id, props.sessionId)
  if (result.collapsedSessionIds.length > 0) {
    notifyTransient(t('workbench.monitor.collapsed', { names: result.collapsedSessionIds.map(sessionTitle).join('、') }))
  }
}

/** ×:退出工作台。流式中需确认(退出≠终止,流在后台继续直至落盘) */
async function onClose() {
  if (stream.value.streaming || runtimeActive.value) {
    const ok = await confirm(t('workbench.monitor.removeConfirm'), t('common.removeBrief'))
    if (!ok) return
  }
  removeSession(props.sessionId)
}

async function onAllow() {
  const p = headPerm.value
  if (p) await respondRequest(p.requestId, 'allow_once')
}

async function onDeny() {
  const p = headPerm.value
  if (p) await respondRequest(p.requestId, 'deny')
}

async function decideEngineInteraction(allow: boolean) {
  const request = engineInteraction.value
  if (!request) return
  const option = allow
    ? request.options.find(item => !item.dangerous)
    : request.options.find(item => item.dangerous)
  if (option) await respondInteraction(request.reference, option.id)
}

async function onRetry() {
  const ok = await retrySession(props.sessionId)
  if (ok) dismissError(props.sessionId)
}

// --- 拖拽(FR-005:拖至 tab 跨台移动 / 拖至右区展开定位) ---

const cardEl = ref<HTMLElement>()
const handleEl = ref<HTMLElement>()
const dropZoneEl = ref<HTMLElement>()
const { isDragging } = useDraggable({
  id: computed(() => 'session:' + props.sessionId),
  element: cardEl,
  handle: handleEl,
})
const { isDropTarget } = useDroppable({
  id: computed(() => 'session-drop:' + props.sessionId),
  element: dropZoneEl,
})
</script>

<template>
  <div
    ref="cardEl"
    class="monitor-card relative bg-card border border-border rounded shadow-paper overflow-hidden cursor-default transition-shadow"
    :class="{
      'edge-accent': status.edge === 'accent',
      'edge-destructive': status.edge === 'destructive',
      'opacity-0': isDragging,
      'card-drop-target': isDropTarget && !isDragging,
      'flash-once': flashSessionId === sessionId,
    }"
    @click="onCardClick"
  >
    <div ref="dropZoneEl" class="absolute inset-0 z-0 pointer-events-none" />
    <!-- 拖拽区：状态行 + 标题 -->
    <div ref="handleEl" class="cursor-grab active:cursor-grabbing touch-none">
    <div class="px-2.5 pt-2 flex items-center gap-1.5 text-[10.5px] text-muted-foreground">
      <span
        class="w-1.5 h-1.5 rounded-full shrink-0"
        :class="[status.dotClass, { 'dot-pulse': status.pulse }]"
      />
      <span
        class="font-semibold"
        :class="{
          'text-primary': status.dotClass === 'bg-primary',
          'text-accent': status.key === 'waiting_permission',
          'text-destructive': status.key === 'error',
        }"
      >{{ status.label }}</span>
      <span
        v-if="expanded"
        class="px-1 text-[9.5px] border border-primary text-primary rounded-sm shrink-0"
      >{{ $t('workbench.monitor.expanded') }}</span>
      <span
        v-if="runnerCount > 0"
        class="px-1 text-[9.5px] border border-primary text-primary rounded-sm shrink-0"
        @click.stop="onCardClick"
      >▶ {{ runnerCount }}</span>
      <button
        class="ml-auto w-4 h-4 grid place-items-center rounded-sm text-muted-foreground hover:text-destructive hover:bg-muted shrink-0"
        :title="$t('workbench.monitor.exitWorkbench')"
        @pointerdown.stop
        @click.stop="onClose"
      >
        <span class="i-carbon-close w-3 h-3" />
      </button>
    </div>

    <!-- 标题 -->
    <div class="px-2.5 mt-0.5 text-xs font-semibold truncate">{{ title }}</div>
    </div><!-- /handle -->

    <!-- 尾部区:最近输出末 2-3 行(150ms 节流;流式中末行带光标) -->
    <div class="mx-2.5 my-1.5 px-2 py-1.5 bg-background border border-border rounded text-[11px] leading-relaxed text-muted-foreground min-h-9">
      <template v-if="displayTail.length > 0">
        <div
          v-for="(line, i) in displayTail"
          :key="i"
          class="truncate"
          :class="{
            'font-mono text-[10.5px]': line.kind === 'tool',
            'text-destructive': line.kind === 'error',
            'text-foreground': i === displayTail.length - 1 && line.kind === 'text',
          }"
        >
          {{ line.text }}<span
            v-if="(stream.streaming || runtimeActive) && i === displayTail.length - 1"
            class="tail-caret"
          />
        </div>
      </template>
      <div v-else class="truncate">
        {{ stream.streaming || runtimeActive ? $t('workbench.monitor.starting') : $t('workbench.monitor.noOutput') }}<span v-if="stream.streaming || runtimeActive" class="tail-caret" />
      </div>
    </div>

    <!-- 就地决策条:等待权限 -->
    <div
      v-if="headPerm"
      class="mx-2.5 mb-1.5 px-2 py-1 border border-border rounded bg-popover flex items-center gap-1.5 text-[11px] decision-accent"
      @click.stop
    >
      <span class="flex-1 min-w-0 truncate font-mono text-[10.5px] text-muted-foreground">{{ permSummary }}</span>
      <!-- 提问/计划类:就地无法作答,引导展开会话 -->
      <button
        v-if="headPermInteractive"
        class="px-2 py-0.5 text-[10.5px] rounded bg-primary text-primary-foreground shrink-0"
        @pointerdown.stop
        @click.stop="onCardClick"
      >{{ $t('workbench.monitor.goAnswer') }}</button>
      <template v-else>
        <button
          class="w-5 h-5 grid place-items-center rounded bg-primary/15 text-primary shrink-0 hover:bg-primary/25"
          :title="$t('common.allow')"
          @pointerdown.stop
          @click.stop="onAllow"
        >
          <span class="i-carbon-arrow-right w-3.5 h-3.5" />
        </button>
        <button
          class="w-5 h-5 grid place-items-center rounded bg-destructive/15 text-destructive shrink-0 hover:bg-destructive/25"
          :title="$t('common.deny')"
          @pointerdown.stop
          @click.stop="onDeny"
        >
          <span class="i-carbon-close w-3.5 h-3.5" />
        </button>
      </template>
    </div>

    <div
      v-else-if="engineInteraction"
      class="mx-2.5 mb-1.5 px-2 py-1 border border-border rounded bg-popover flex items-center gap-1.5 text-[11px] decision-accent"
      @click.stop
    >
      <span class="flex-1 min-w-0 truncate text-muted-foreground">{{ engineInteraction.title || engineInteraction.kind }}</span>
      <!-- 提问/计划类:就地无法作答,引导展开会话 -->
      <button
        v-if="engineInteractionInteractive"
        class="px-2 py-0.5 text-[10.5px] rounded bg-primary text-primary-foreground shrink-0"
        @pointerdown.stop
        @click.stop="onCardClick"
      >{{ $t('workbench.monitor.goAnswer') }}</button>
      <template v-else>
        <button
          class="w-5 h-5 grid place-items-center rounded bg-primary/15 text-primary shrink-0 hover:bg-primary/25"
          :title="$t('common.allow')"
          @pointerdown.stop
          @click.stop="decideEngineInteraction(true)"
        >
          <span class="i-carbon-arrow-right w-3.5 h-3.5" />
        </button>
        <button
          class="w-5 h-5 grid place-items-center rounded bg-destructive/15 text-destructive shrink-0 hover:bg-destructive/25"
          :title="$t('common.deny')"
          @pointerdown.stop
          @click.stop="decideEngineInteraction(false)"
        >
          <span class="i-carbon-close w-3.5 h-3.5" />
        </button>
      </template>
    </div>

    <!-- 就地决策条:出错重试 -->
    <div
      v-else-if="canRetry"
      class="mx-2.5 mb-1.5 px-2 py-1 border border-border rounded bg-popover flex items-center gap-1.5 text-[11px] decision-destructive"
      @click.stop
    >
      <span class="flex-1 min-w-0 truncate text-muted-foreground">{{ $t('workbench.monitor.taskStopped') }}</span>
      <button
        class="px-2 py-0.5 text-[10.5px] rounded bg-primary text-primary-foreground shrink-0"
        @pointerdown.stop
        @click.stop="onRetry"
      >{{ $t('common.retry') }}</button>
    </div>

    <!-- 摘要 -->
    <div
      v-if="getMeta(sessionId)?.summary"
      v-tooltip="getMeta(sessionId)!.summary"
      class="mx-2.5 mb-1 text-[10.5px] text-muted-foreground/60 line-clamp-2 leading-relaxed"
    >{{ getMeta(sessionId)!.summary }}</div>

    <!-- meta 行:项目名 + 持续/最后活动时间 + token -->
    <div class="px-2.5 pb-2 flex items-center gap-2.5 text-[10px] text-muted-foreground tabular-nums">
      <span class="truncate">{{ summary?.projectName ?? draft?.projectName ?? '—' }}</span>
      <span class="shrink-0">{{ durationText }}</span>
      <span v-if="tokenText" class="shrink-0">{{ tokenText }}</span>
    </div>
  </div>
</template>

<style scoped>
.monitor-card {
  transition: box-shadow 0.15s, opacity 0.15s;
}
.monitor-card:hover {
  box-shadow: var(--shadow-paper-lifted);
}
.card-drop-target {
  box-shadow: var(--shadow-paper-lifted);
  outline: 2px solid var(--primary);
  outline-offset: -2px;
  border-radius: var(--radius);
}
.card-drop-target::before {
  content: '';
  position: absolute;
  top: -5px;
  left: 4px;
  right: 4px;
  height: 2px;
  background: var(--primary);
  border-radius: 1px;
  z-index: 1;
}
/* 状态左边框(3px 语义色) */
.edge-accent {
  border-left: 3px solid var(--accent);
}
.edge-destructive {
  border-left: 3px solid var(--destructive);
}
/* 决策条左侧 2px 语义线 */
.decision-accent {
  border-left: 2px solid var(--accent);
}
.decision-destructive {
  border-left: 2px solid var(--destructive);
}
/* 运行态脉冲 */
.dot-pulse {
  animation: mc-pulse 1.6s ease-in-out infinite;
}
@keyframes mc-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.35; }
}
/* 流式尾部光标 */
.tail-caret {
  display: inline-block;
  width: 2px;
  height: 1em;
  background: var(--foreground);
  vertical-align: text-bottom;
  margin-left: 1px;
  animation: mc-blink 1s steps(2) infinite;
}
@keyframes mc-blink {
  50% { opacity: 0; }
}
/* 重复打开高亮:背景闪烁一次(FR-002) */
.flash-once {
  animation: mc-flash 1s ease-out 1;
}
@keyframes mc-flash {
  0%, 60% { background: var(--secondary); }
  100% { background: var(--card); }
}
@media (prefers-reduced-motion: reduce) {
  .dot-pulse, .tail-caret, .flash-once {
    animation: none;
  }
}
</style>
