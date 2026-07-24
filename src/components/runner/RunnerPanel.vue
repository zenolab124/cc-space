<script setup lang="ts">
/**
 * RunnerPanel — 跑单面板主体（形态无关）
 * 头部（图标+标题+运行计数+新跑单+钉住+关闭）→
 * 悬浮形态渲染 RunnerChipBar / 停靠形态渲染 RunnerDockList →
 * RunnerLogView 日志区
 */
import { ref, computed, watch, onMounted, onUnmounted, inject } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRunners } from '@/composables/useRunners'
import type { RunnerSnapshot } from '@/types'
import RunnerChipBar from './RunnerChipBar.vue'
import RunnerDockList from './RunnerDockList.vue'
import RunnerLogView from './RunnerLogView.vue'
import RunnerNewDialog from './RunnerNewDialog.vue'

const props = defineProps<{
  mode: 'float' | 'dock'
  sessionId: string
  sessionCwd: string
  projectName: string
}>()

const emit = defineEmits<{
  close: []
  togglePin: []
  toggleDock: []
}>()

const { t } = useI18n()
const {
  dialogOpen,
  runnerPinned,
  tailLinesDefault,
  getCommands,
  getSelectedRunner,
  setSelectedRunner,
  getSessionRunners,
  getRunnerLogs,
  spawnRunner,
  stopRunner,
  restartRunner,
  removeCommand,
  runningCount,
  runners: runnersMap,
} = useRunners()

// --- 本会话 runner 列表（响应式） ---
const sessionRunners = computed(() => getSessionRunners(props.sessionId))
const currentRunningCount = computed(() => runningCount(props.sessionId))

// --- 本项目候选命令（按 cwd 分桶取本项目） ---
const sessionCommands = computed(() => getCommands(props.sessionCwd))

// --- 选中 runner 联动（按会话分桶，多列不串台） ---
const currentSelectedId = computed(() => getSelectedRunner(props.sessionId))

const selectedRunner = computed<RunnerSnapshot | null>(() => {
  const id = currentSelectedId.value
  if (!id) return null
  return sessionRunners.value.find(r => r.id === id) ?? null
})

const selectedLogs = computed(() => {
  const id = currentSelectedId.value
  if (!id) return []
  return getRunnerLogs(id)
})

function onSelect(runnerId: string) {
  setSelectedRunner(props.sessionId, runnerId)
}

// 有 runner 时默认选中第一个
watch(sessionRunners, (list) => {
  if (list.length > 0 && !list.some(r => r.id === currentSelectedId.value)) {
    setSelectedRunner(props.sessionId, list[0].id)
  }
}, { immediate: true })

// --- 时长活秒表 ---
const now = ref(Date.now())
let durationTimer: ReturnType<typeof setInterval> | null = null

function startDurationTick() {
  if (durationTimer) return
  durationTimer = setInterval(() => { now.value = Date.now() }, 1000)
}

function stopDurationTick() {
  if (durationTimer) {
    clearInterval(durationTimer)
    durationTimer = null
  }
}

// 有运行中的 runner 时才开 timer
watch(currentRunningCount, (n) => {
  if (n > 0) startDurationTick()
  else stopDurationTick()
}, { immediate: true })

onMounted(() => { if (currentRunningCount.value > 0) startDurationTick() })
onUnmounted(() => stopDurationTick())

/** 格式化 HH:MM:SS（startedAt 为 epoch 毫秒） */
function formatDuration(startedAt: number): string {
  const elapsed = Math.max(0, Math.floor((now.value - startedAt) / 1000))
  const h = Math.floor(elapsed / 3600)
  const m = Math.floor((elapsed % 3600) / 60)
  const s = elapsed % 60
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

// --- 新跑单弹窗 ---
const dialogVisible = ref(false)
const spawnError = ref('')

// 弹窗状态同步到全局（悬浮面板点外关闭豁免依据）
watch(dialogVisible, (v) => {
  dialogOpen.value = v
  if (!v) spawnError.value = ''
})

async function onSpawn(spec: { cmd: string; cwd: string; alias: string; env: Record<string, string> }) {
  spawnError.value = ''
  try {
    await spawnRunner(props.sessionId, spec.cmd, spec.cwd, spec.alias || undefined, Object.keys(spec.env).length > 0 ? spec.env : undefined)
    dialogVisible.value = false
  } catch (e) {
    spawnError.value = t('runner.spawnFailed') + ': ' + String(e)
  }
}

// --- 候选命令操作 ---
async function onLaunchCommand(commandId: string) {
  const cmd = sessionCommands.value.find(c => c.id === commandId)
  if (!cmd) return
  try {
    await spawnRunner(props.sessionId, cmd.cmd, cmd.cwd || props.sessionCwd, cmd.alias || undefined, undefined, commandId)
  } catch (_) {
    // 失败由事件通知
  }
}

async function onRemoveCommand(commandId: string) {
  try {
    await removeCommand(props.sessionCwd, commandId)
  } catch (_) {
    // 静默
  }
}

// --- 塞输入框 ---
const appendToInput = inject<(text: string) => void>('runnerAppendToInput', () => {})

function onInsertToInput(text: string) {
  appendToInput(text)
}

// --- 空态检测 ---
const isEmpty = computed(() => sessionRunners.value.length === 0 && sessionCommands.value.length === 0)
</script>

<template>
  <div class="runner-panel">
    <!-- 面板头部 -->
    <div class="rp-head">
      <span class="text-primary text-xs">▶</span>
      <span class="text-xs font-semibold">{{ t('runner.title') }}</span>
      <span v-if="currentRunningCount > 0" class="text-[10.5px] text-muted-foreground">
        {{ t('runner.runningCount', { n: currentRunningCount }) }}
      </span>
      <span class="flex-1" />
      <button
        class="rp-icon-btn rp-icon-new"
        :title="t('runner.newRunner')"
        @click="dialogVisible = true"
      >
        <span class="i-carbon-add w-3.5 h-3.5" />
      </button>
      <button
        v-if="mode === 'float'"
        class="rp-icon-btn"
        :class="{ active: runnerPinned }"
        :title="runnerPinned ? t('runner.unpinTitle') : t('runner.pinTitle')"
        @click="emit('togglePin')"
      >
        <span :class="runnerPinned ? 'i-carbon-pin-filled' : 'i-carbon-pin'" class="w-3.5 h-3.5" />
      </button>
      <button
        class="rp-icon-btn"
        :class="{ active: mode === 'dock' }"
        :title="mode === 'float' ? t('runner.toDockTitle') : t('runner.toFloatTitle')"
        @click="emit('toggleDock')"
      >
        <span class="i-carbon-open-panel-right w-3.5 h-3.5" />
      </button>
      <button class="rp-icon-btn" :title="t('runner.close')" @click="emit('close')">
        <span class="i-carbon-close w-3.5 h-3.5" />
      </button>
    </div>

    <!-- 空态 -->
    <div v-if="isEmpty" class="empty-body">
      <div class="text-2xl opacity-35">▶</div>
      <div>{{ t('runner.emptyTitle') }}</div>
      <div class="empty-hint">{{ t('runner.emptyHint') }}</div>
    </div>

    <!-- 内容体 -->
    <template v-else>
      <!-- 悬浮形态：胶囊栏 -->
      <RunnerChipBar
        v-if="mode === 'float'"
        :runners="sessionRunners"
        :commands="sessionCommands"
        :selected-id="currentSelectedId"
        @select="onSelect"
        @stop="stopRunner($event)"
        @restart="restartRunner($event)"
        @launch="onLaunchCommand"
        @remove-command="onRemoveCommand"
      />

      <!-- 停靠形态：分区列表 -->
      <RunnerDockList
        v-else
        :runners="sessionRunners"
        :commands="sessionCommands"
        :selected-id="currentSelectedId"
        :project-name="projectName"
        @select="onSelect"
        @stop="stopRunner($event)"
        @restart="restartRunner($event)"
        @launch="onLaunchCommand"
        @remove-command="onRemoveCommand"
      >
        <template #duration="{ runner }">
          {{ formatDuration(runner.startedAt) }}
        </template>
      </RunnerDockList>

      <!-- 日志区 -->
      <RunnerLogView
        :runner="selectedRunner"
        :lines="selectedLogs"
        :tail-lines-default="tailLinesDefault"
        @restart="restartRunner($event)"
        @stop="stopRunner($event)"
        @insert-to-input="onInsertToInput"
        @change-tail-lines="tailLinesDefault = $event"
      />
    </template>

    <!-- 新跑单弹窗 -->
    <RunnerNewDialog
      :visible="dialogVisible"
      :default-cwd="sessionCwd"
      :spawn-error="spawnError"
      @close="dialogVisible = false"
      @spawn="onSpawn"
    />
  </div>
</template>

<style scoped>
.runner-panel {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
}
.rp-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
/* 头部图标按钮：与会话顶栏工具按钮同风格 */
.rp-icon-btn {
  padding: 4px;
  border: none;
  background: none;
  border-radius: var(--radius);
  color: var(--muted-foreground);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  transition: color 0.15s, background-color 0.15s;
}
.rp-icon-btn:hover { color: var(--foreground); background: var(--muted); }
.rp-icon-btn.active {
  color: var(--primary);
  background: color-mix(in oklch, var(--primary) 10%, transparent);
}
.rp-icon-new:hover { color: var(--primary); }

/* 空态 */
.empty-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: var(--muted-foreground);
  font-size: 12px;
  text-align: center;
  padding: 0 32px;
}
.empty-hint {
  font-size: 10.5px;
  opacity: 0.75;
  line-height: 1.8;
  white-space: pre-line;
}
</style>
