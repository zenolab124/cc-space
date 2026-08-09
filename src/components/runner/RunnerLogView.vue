<script setup lang="ts">
/**
 * Runner 日志查看器
 * 工具栏 + 虚拟化日志体 + 自动滚动页脚
 */
import { ref, shallowRef, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { parseAnsi, stripAnsi } from '@/utils/ansi'
import type { RunnerSnapshot, LogLine } from '@/types'
import { shouldCompensateVirtualItemSizeChange } from '@/lib/sessionScrollPolicy'

const props = defineProps<{
  runner: RunnerSnapshot | null
  lines: LogLine[]
  tailLinesDefault: number
}>()

const emit = defineEmits<{
  restart: [id: string]
  stop: [id: string]
  insertToInput: [text: string]
  changeTailLines: [n: number]
}>()

const { t } = useI18n()

// ---- 过滤 ----
const filterText = ref('')
const filteredLines = computed(() => {
  if (!filterText.value) return props.lines
  const q = filterText.value.toLowerCase()
  return props.lines.filter(l => l.text.toLowerCase().includes(q))
})

// ---- 虚拟化阈值：>500 行启用 ----
const VIRTUAL_THRESHOLD = 500
// 尾部直渲行数，保持流式写入平滑
const TAIL_DIRECT = 30
const shouldVirtualize = computed(() => filteredLines.value.length > VIRTUAL_THRESHOLD)
// 虚拟化段：去掉尾部直渲行
const virtualLines = computed(() => {
  if (!shouldVirtualize.value) return []
  return filteredLines.value.slice(0, -TAIL_DIRECT)
})
// 尾部直渲段
const tailLines = computed(() => {
  if (!shouldVirtualize.value) return filteredLines.value
  return filteredLines.value.slice(-TAIL_DIRECT)
})

// ---- 滚动容器 & 自动滚动 ----
const scrollEl = ref<HTMLElement | null>(null)
const atBottom = ref(true)
const followLogs = ref(true)
const BOTTOM_THRESHOLD = 30
const ACTIVE_SCROLL_INTENT_MS = 500
let scrollFollowGeneration = 0
let scrollRequestId = 0
let lastScrollTop = 0
let downwardIntentAt = Number.NEGATIVE_INFINITY
let upwardIntentAt = Number.NEGATIVE_INFINITY
let virtualAdjustmentUntil = Number.NEGATIVE_INFINITY

function distanceFromBottom(el: HTMLElement): number {
  return Math.max(0, el.scrollHeight - el.scrollTop - el.clientHeight)
}

function invalidateScrollRequests() {
  scrollFollowGeneration++
  scrollRequestId++
}

function stopFollowingLogs() {
  followLogs.value = false
  invalidateScrollRequests()
}

function resumeFollowingLogs() {
  if (!followLogs.value) invalidateScrollRequests()
  followLogs.value = true
}

function onLogWheel(event: WheelEvent) {
  const now = performance.now()
  if (event.deltaY < 0) {
    upwardIntentAt = now
    stopFollowingLogs()
    return
  }
  if (event.deltaY <= 0) return
  downwardIntentAt = now
  const el = scrollEl.value
  // 已经由滚动条/键盘到达底部后，向下滚轮可能不会再产生 scroll 事件。
  if (el && distanceFromBottom(el) <= BOTTOM_THRESHOLD) resumeFollowingLogs()
}

function onScroll() {
  if (!scrollEl.value) return
  const el = scrollEl.value
  const now = performance.now()
  const nextScrollTop = el.scrollTop
  const delta = nextScrollTop - lastScrollTop
  const reachedBottom = distanceFromBottom(el) <= BOTTOM_THRESHOLD
  lastScrollTop = nextScrollTop
  atBottom.value = reachedBottom

  // wheel 之外的上滚（滚动条、键盘）也必须同步取消已排队的滚底。
  if (delta < -0.5 && !reachedBottom) {
    upwardIntentAt = now
    if (followLogs.value) stopFollowingLogs()
    return
  }

  // 只认近期用户向下意图。虚拟项首次测量也会写 scrollTop，不能借此恢复跟随。
  if (
    !followLogs.value
    && reachedBottom
    && delta > 0.5
    && downwardIntentAt > upwardIntentAt
    && now - downwardIntentAt <= ACTIVE_SCROLL_INTENT_MS
    && now >= virtualAdjustmentUntil
  ) {
    resumeFollowingLogs()
  }
}

function requestScrollToEnd(allowLayoutReset = false) {
  if (!followLogs.value) return
  const generation = scrollFollowGeneration
  const requestId = ++scrollRequestId
  const scheduledElement = scrollEl.value
  const scheduledScrollTop = scheduledElement?.scrollTop ?? 0
  void nextTick(() => {
    requestAnimationFrame(() => {
      const el = scrollEl.value
      if (
        !followLogs.value
        || generation !== scrollFollowGeneration
        || requestId !== scrollRequestId
        || !el
        || (scheduledElement !== null && el !== scheduledElement)
      ) return
      // scroll 回调若尚未运行，位置本身的上移仍能使旧请求失效。
      if (
        !allowLayoutReset
        && el.scrollTop < scheduledScrollTop - 0.5
        && distanceFromBottom(el) > BOTTOM_THRESHOLD
      ) {
        upwardIntentAt = performance.now()
        stopFollowingLogs()
        return
      }
      el.scrollTop = el.scrollHeight
      lastScrollTop = el.scrollTop
      atBottom.value = true
    })
  })
}

// 切换 runner 时重置滚底状态并立即滚到底部
watch(() => props.runner?.id, () => {
  invalidateScrollRequests()
  followLogs.value = true
  atBottom.value = true
  downwardIntentAt = Number.NEGATIVE_INFINITY
  upwardIntentAt = Number.NEGATIVE_INFINITY
  lastScrollTop = scrollEl.value?.scrollTop ?? 0
  requestScrollToEnd(true)
}, { immediate: true })

// 新行到达时若在底部则自动滚
watch(() => filteredLines.value.length, () => {
  if (followLogs.value) requestScrollToEnd()
})

// ---- 虚拟化 ----
const virtualLineKeys = computed(() => {
  const runnerId = props.runner?.id ?? 'none'
  return virtualLines.value.map((line, index) => `${runnerId}:${line.seq ?? index}`)
})
const virtualLineKeySnapshot = shallowRef<readonly string[]>([])
const virtualLineKeyExtractor = shallowRef<(index: number) => string>(
  index => `none:${index}`,
)

watch(virtualLineKeys, (keys) => {
  const previous = virtualLineKeySnapshot.value
  if (keys.length === previous.length && keys.every((key, index) => key === previous[index])) return
  const snapshot = [...keys]
  const runnerId = props.runner?.id ?? 'none'
  virtualLineKeySnapshot.value = snapshot
  virtualLineKeyExtractor.value = index => snapshot[index] ?? `${runnerId}:${index}`
}, { immediate: true, flush: 'sync' })

const virtualizer = useVirtualizer(computed(() => ({
  count: virtualLines.value.length,
  getScrollElement: () => scrollEl.value ?? null,
  getItemKey: virtualLineKeyExtractor.value,
  estimateSize: () => 20,
  overscan: 20,
})))

virtualizer.value.shouldAdjustScrollPositionOnItemSizeChange = (item, delta, instance) => {
  const shouldAdjust = shouldCompensateVirtualItemSizeChange({
    scrollDirection: instance.scrollDirection,
    upwardGestureActive: performance.now() - upwardIntentAt < 220,
    itemStart: item.start,
    itemSize: item.size,
    scrollOffset: instance.scrollOffset ?? 0,
    delta,
  })
  if (shouldAdjust) virtualAdjustmentUntil = performance.now() + 80
  return shouldAdjust
}

// ---- 时长计时 ----
const now = ref(Date.now())
let ticker: ReturnType<typeof setInterval> | null = null

function startTicker() {
  stopTicker()
  ticker = setInterval(() => { now.value = Date.now() }, 1000)
}
function stopTicker() {
  if (ticker) { clearInterval(ticker); ticker = null }
}

watch(() => props.runner?.status, (s) => {
  if (s === 'running' || s === 'starting') startTicker()
  else stopTicker()
}, { immediate: true })

onMounted(() => {
  if (props.runner?.status === 'running' || props.runner?.status === 'starting') startTicker()
})
onUnmounted(stopTicker)

const duration = computed(() => {
  if (!props.runner) return '--:--:--'
  const end = (props.runner.status === 'running' || props.runner.status === 'starting')
    ? now.value
    : (props.runner.exitedAt ?? now.value)
  const ms = Math.max(0, end - props.runner.startedAt)
  const s = Math.floor(ms / 1000)
  const hh = String(Math.floor(s / 3600)).padStart(2, '0')
  const mm = String(Math.floor((s % 3600) / 60)).padStart(2, '0')
  const ss = String(s % 60).padStart(2, '0')
  return `${hh}:${mm}:${ss}`
})

// ---- 时间戳格式化 ----
function fmtTs(ts: number): string {
  const d = new Date(ts)
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map(n => String(n).padStart(2, '0')).join(':')
}

// ---- 状态点颜色 ----
const statusDotClass = computed(() => {
  if (!props.runner) return ''
  const m: Record<string, string> = {
    running: 'dot-running', starting: 'dot-running',
    exited: 'dot-exited', killed: 'dot-killed',
    crashed: 'dot-crashed', 'spawn-failed': 'dot-crashed',
  }
  return m[props.runner.status] ?? ''
})

// ---- 操作 ----
function onInsertToInput() {
  if (!props.runner) return
  const n = props.tailLinesDefault
  const tail = props.lines.slice(-n)
  const stripped = tail.map(l => stripAnsi(l.text)).join('\n')
  const alias = props.runner.alias || props.runner.cmd
  const header = `# runner: ${alias} · ${t('runner.lastNLines', { n: tail.length })} · ${new Date().toISOString()}`
  const block = `\n\n${header}\n\`\`\`log\n${stripped}\n\`\`\`\n`
  emit('insertToInput', block)
}

async function onCopyLogPath() {
  if (!props.runner) return
  try { await navigator.clipboard.writeText(props.runner.logPath) } catch (_) { /* 静默 */ }
}

function onRestart() {
  if (props.runner) emit('restart', props.runner.id)
}

function onStop() {
  if (props.runner) emit('stop', props.runner.id)
}
</script>

<template>
  <div class="log-area">
    <!-- 空态 -->
    <div v-if="!runner" class="empty-log">
      {{ t('runner.selectRunnerForLog') }}
    </div>

    <template v-else>
      <!-- 工具栏 -->
      <div class="log-toolbar">
        <div class="log-target">
          <span class="status-dot" :class="statusDotClass" />
          <span class="alias">{{ runner.alias || runner.cmd.slice(0, 32) }}</span>
          <span class="log-dur">{{ duration }}</span>
        </div>

        <input
          v-model="filterText"
          class="log-search"
          type="text"
          :placeholder="t('runner.filterLog')"
        />

        <!-- 尾行数选择 -->
        <select
          class="tail-select"
          :value="tailLinesDefault"
          @change="emit('changeTailLines', Number(($event.target as HTMLSelectElement).value))"
        >
          <option v-for="v in [30, 100, 300, 1000]" :key="v" :value="v">{{ v }}</option>
        </select>

        <button class="tb-btn" :title="t('runner.insertToInput')" @click="onInsertToInput">
          <div class="i-carbon-export tb-icon" />
        </button>
        <button
          v-if="runner.status === 'running' || runner.status === 'starting'"
          class="tb-btn" :title="t('runner.stop')" @click="onStop"
        >
          <div class="i-carbon-stop-filled tb-icon" />
        </button>
        <button v-else class="tb-btn" :title="t('runner.restart')" @click="onRestart">
          <div class="i-carbon-restart tb-icon" />
        </button>
        <button class="tb-btn" :title="t('runner.copyLogPath')" @click="onCopyLogPath">
          <div class="i-carbon-copy tb-icon" />
        </button>
      </div>

      <!-- 日志体 -->
      <div ref="scrollEl" class="log-body" @wheel.passive="onLogWheel" @scroll.passive="onScroll">
        <!-- 虚拟化模式 -->
        <template v-if="shouldVirtualize">
          <div :style="{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }">
            <div
              v-for="vRow in virtualizer.getVirtualItems()"
              :key="String(vRow.key)"
              :style="{
                position: 'absolute',
                top: `${vRow.start}px`,
                left: 0,
                right: 0,
              }"
              :ref="(el) => { if (el) virtualizer.measureElement(el as HTMLElement) }"
              :data-index="vRow.index"
              class="log-line"
              :class="{ stderr: virtualLines[vRow.index].stream === 'stderr' }"
            >
              <span class="log-ts">{{ fmtTs(virtualLines[vRow.index].ts) }}</span>
              <span
                v-for="(span, j) in parseAnsi(virtualLines[vRow.index].text)"
                :key="j"
                :class="span.colorClass"
              >{{ span.text }}</span>
            </div>
          </div>
        </template>

        <!-- 尾部直渲 / 非虚拟化模式 -->
        <div
          v-for="line in tailLines"
          :key="line.seq"
          class="log-line"
          :class="{ stderr: line.stream === 'stderr' }"
        >
          <span class="log-ts">{{ fmtTs(line.ts) }}</span>
          <span
            v-for="(span, j) in parseAnsi(line.text)"
            :key="j"
            :class="span.colorClass"
          >{{ span.text }}</span>
        </div>
      </div>

      <!-- 页脚 -->
      <div class="log-foot">
        <template v-if="followLogs && atBottom && (runner.status === 'running' || runner.status === 'starting')">
          <span class="pulse" />
          <span>{{ t('runner.following') }}</span>
        </template>
        <span v-else class="line-count">{{ t('runner.lineCount', { n: filteredLines.length }) }}</span>
      </div>
    </template>
  </div>
</template>

<style scoped>
.log-area { flex: 1; min-height: 0; display: flex; flex-direction: column; border-top: 1px solid var(--border); }
.log-toolbar { display: flex; align-items: center; gap: 6px; padding: 6px 12px; border-bottom: 1px solid var(--border); flex-shrink: 0; }
.log-target { font-size: 11px; font-weight: 600; display: flex; align-items: center; gap: 6px; white-space: nowrap; }
.log-dur { font-weight: 400; color: var(--muted-foreground); font-variant-numeric: tabular-nums; }
.log-search { flex: 1; min-width: 60px; font-size: 11px; padding: 3px 8px; border-radius: var(--radius); border: 1px solid var(--input); background: var(--card); color: var(--foreground); outline: none; }
.log-search:focus { border-color: var(--ring); }

.tail-select {
  font-size: 10px; padding: 2px 4px; border-radius: var(--radius);
  border: 1px solid var(--input); background: var(--card); color: var(--foreground);
  cursor: pointer; outline: none;
}

.tb-btn {
  display: inline-flex; align-items: center; justify-content: center;
  width: 24px; height: 24px; border-radius: var(--radius);
  border: none; background: transparent; color: var(--muted-foreground);
  cursor: pointer; transition: color 0.15s, background 0.15s;
}
.tb-btn:hover { color: var(--foreground); background: var(--accent); }
.tb-icon { width: 13px; height: 13px; }

.log-body {
  flex: 1; min-height: 0; overflow-y: auto; padding: 8px 12px;
  font-family: var(--font-mono); font-size: 11px; line-height: 1.65;
  background: color-mix(in oklch, var(--background) 55%, var(--card));
}
.log-line { white-space: pre-wrap; word-break: break-all; }
.log-ts { color: var(--muted-foreground); opacity: 0.55; font-size: 9.5px; margin-right: 8px; }
.log-line.stderr .log-ts { color: var(--ansi-red); opacity: 0.7; }
/* stderr 行淡红底：无 ANSI 的输出也能一眼分辨错误流 */
.log-line.stderr { background: color-mix(in oklch, var(--ansi-red) 7%, transparent); }

.log-foot {
  display: flex; align-items: center; gap: 6px; padding: 4px 12px;
  flex-shrink: 0; border-top: 1px solid var(--border);
  font-size: 10px; color: var(--muted-foreground);
}
.pulse {
  width: 6px; height: 6px; border-radius: 50%;
  background: var(--run-running);
  animation: pulse-anim 1.6s ease-in-out infinite;
}
@keyframes pulse-anim { 0%, 100% { opacity: 1 } 50% { opacity: 0.35 } }
.line-count { font-variant-numeric: tabular-nums; }

.empty-log {
  flex: 1; display: flex; align-items: center; justify-content: center;
  color: var(--muted-foreground); font-size: 12px;
}

.status-dot { width: 6px; height: 6px; border-radius: 50%; flex-shrink: 0; }
.dot-running { background: var(--run-running); }
.dot-exited { background: var(--run-exited); }
.dot-killed { background: var(--muted-foreground); }
.dot-crashed { background: var(--ansi-red); }

/* ANSI 色映射 */
.ansi-green { color: var(--ansi-green); }
.ansi-red { color: var(--ansi-red); }
.ansi-yellow { color: var(--ansi-yellow); }
.ansi-blue { color: var(--ansi-blue); }
.ansi-magenta { color: var(--ansi-magenta); }
</style>
