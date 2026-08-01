<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { usePerfMonitor, startPerfMonitor, stopPerfMonitor } from '@/composables/usePerfMonitor'
import { anyStreaming } from '@/composables/useStreaming'
import { getBootBreakdown } from '@/utils/bootTrace'

const emit = defineEmits<{ close: [] }>()
const { t } = useI18n()
const {
  fps,
  baselineFps,
  jankCount,
  maxBlockMs,
  longFrames,
  clickLatencyLast,
  clickLatencyP95,
  domNodes,
  memStats,
  projEvents,
  streamRates,
  fpsHistory,
} = usePerfMonitor()

onMounted(() => startPerfMonitor())
onUnmounted(() => stopPerfMonitor())

const webTotal = computed(() =>
  memStats.value?.webkit.reduce((s, p) => s + p.footprint_mb, 0) ?? 0,
)
const cliTotal = computed(() =>
  memStats.value?.cli.reduce((s, p) => s + p.footprint_mb, 0) ?? 0,
)

function mb(v: number): string {
  return v >= 1024 ? (v / 1024).toFixed(2) + ' GB' : Math.round(v) + ' MB'
}

/** FPS sparkline：60 点折线，y 轴 0-120 截断 */
const sparkPoints = computed(() => {
  const h = 20
  const pts = fpsHistory.value
  if (pts.length < 2) return ''
  return pts
    .map((v, i) => `${(i * 100) / 59},${h - (Math.min(v, 120) / 120) * h}`)
    .join(' ')
})

/** 分级着色：good/warn/bad */
function grade(v: number, warn: number, bad: number): string {
  return v >= bad ? 'text-destructive' : v >= warn ? 'text-amber-600 dark:text-amber-400' : ''
}

// 启动分解:marks 在启动期一次性写入,HUD 打开时读快照即可,无需响应式
const bootSegs = getBootBreakdown()
// 瀑布段(参与合计)与独立计时(overlay,与瀑布段重叠、不计入合计)分列展示
const bootWaterfall = bootSegs.filter(s => !s.overlay)
const bootOverlay = bootSegs.filter(s => s.overlay)
</script>

<template>
  <div
    class="fixed bottom-3 right-3 z-[200] w-[260px] rounded-md border border-border bg-card text-card-foreground shadow-paper text-xs select-none"
  >
    <div class="flex items-center justify-between px-2.5 py-1.5 border-b border-border">
      <span class="font-medium">{{ t('perf.title') }}</span>
      <div class="flex items-center gap-1.5">
        <span
          v-if="anyStreaming()"
          class="text-[10px] px-1 rounded bg-primary/10 text-primary"
        >{{ t('perf.streaming') }}</span>
        <button class="icon-btn" @click="emit('close')">
          <span class="i-carbon-close w-3 h-3" />
        </button>
      </div>
    </div>

    <div class="px-2.5 py-1.5 space-y-1 font-mono tabular-nums">
      <!-- 帧率 -->
      <div class="flex items-center justify-between">
        <span class="font-sans text-muted-foreground">{{ t('perf.fps') }}</span>
        <!-- 分级锚定观察到的刷新率基线（ProMotion 120Hz 机器不低报） -->
        <span :class="grade(baselineFps - fps, baselineFps * 0.25, baselineFps * 0.5)">{{ fps }}</span>
      </div>
      <svg v-if="sparkPoints" viewBox="0 0 100 20" preserveAspectRatio="none" class="w-full h-5 opacity-70">
        <polyline :points="sparkPoints" fill="none" stroke="currentColor" stroke-width="1" />
      </svg>
      <div class="flex items-center justify-between">
        <span class="font-sans text-muted-foreground">{{ t('perf.jank') }}</span>
        <span :class="grade(jankCount, 5, 20)">{{ jankCount }}</span>
      </div>
      <div class="flex items-center justify-between">
        <span class="font-sans text-muted-foreground">{{ t('perf.maxBlock') }}</span>
        <span :class="grade(maxBlockMs, 50, 200)">{{ maxBlockMs }} ms</span>
      </div>

      <!-- 长帧归因:JS 占比低=渲染管线(layout/paint),高则看 top 段名 -->
      <template v-if="longFrames.length">
        <div class="font-sans text-muted-foreground border-t border-border pt-1">{{ t('perf.longFrames') }}</div>
        <div
          v-for="(lf, i) in [...longFrames].reverse()"
          :key="i"
          class="text-[10px] flex items-center justify-between"
        >
          <span :class="grade(lf.total, 50, 100)">{{ lf.total }}ms</span>
          <span class="text-muted-foreground">js {{ lf.js }}ms · {{ lf.top }}</span>
        </div>
      </template>

      <!-- 点击延迟 -->
      <div class="flex items-center justify-between border-t border-border pt-1">
        <span class="font-sans text-muted-foreground">{{ t('perf.click') }}</span>
        <span>
          <span :class="grade(clickLatencyLast, 50, 100)">{{ clickLatencyLast }}</span>
          <span class="text-muted-foreground"> / p95 </span>
          <span :class="grade(clickLatencyP95, 50, 100)">{{ clickLatencyP95 }}</span>
          <span class="text-muted-foreground"> ms</span>
        </span>
      </div>

      <!-- DOM -->
      <div class="flex items-center justify-between">
        <span class="font-sans text-muted-foreground">{{ t('perf.dom') }}</span>
        <span :class="grade(domNodes, 30000, 80000)">{{ domNodes.toLocaleString() }}</span>
      </div>

      <!-- 内存 -->
      <template v-if="memStats">
        <div class="flex items-center justify-between border-t border-border pt-1">
          <span class="font-sans text-muted-foreground">{{ t('perf.memMain') }}</span>
          <span>{{ mb(memStats.main.footprint_mb) }}</span>
        </div>
        <div class="flex items-center justify-between">
          <span class="font-sans text-muted-foreground">{{ t('perf.memWeb') }}</span>
          <span :class="grade(webTotal, 500, 900)">{{ mb(webTotal) }}</span>
        </div>
        <div class="flex items-center justify-between">
          <span class="font-sans text-muted-foreground">{{ t('perf.memCli', { count: memStats.cli.length }) }}</span>
          <span>{{ mb(cliTotal) }}</span>
        </div>
        <div class="flex items-center justify-between">
          <span class="font-sans text-muted-foreground">{{ t('perf.memTotal') }}</span>
          <span class="font-medium">{{ mb(memStats.total_mb) }}</span>
        </div>
      </template>

      <!-- 启动分解:瀑布段(block=主线程阻塞等待, paint=首帧真实绘制)按序加和, total = 导航→首帧上屏 -->
      <template v-if="bootSegs.length">
        <div class="font-sans text-muted-foreground border-t border-border pt-1">{{ t('perf.boot') }}</div>
        <div
          v-for="s in bootWaterfall"
          :key="s.label"
          class="flex items-center justify-between text-[10px]"
        >
          <span class="font-sans text-muted-foreground">{{ t('perf.boot_' + s.label) }}</span>
          <span :class="s.label === 'total' ? 'font-medium ' + grade(s.ms, 1000, 2500) : ''">{{ Math.round(s.ms) }} ms</span>
        </div>
        <!-- 独立计时:与瀑布段时间重叠、不计入合计,更淡色 + └ 前缀区分 -->
        <div
          v-for="s in bootOverlay"
          :key="s.label"
          class="flex items-center justify-between text-[10px] opacity-55"
        >
          <span class="font-sans text-muted-foreground">└ {{ t('perf.boot_' + s.label) }}</span>
          <span class="text-muted-foreground">{{ Math.round(s.ms) }} ms</span>
        </div>
      </template>

      <!-- 事件计数 -->
      <div class="flex items-center justify-between border-t border-border pt-1">
        <span class="font-sans text-muted-foreground">{{ t('perf.streamEvents') }}</span>
        <span>
          <span>{{ streamRates.input }}</span>
          <span class="text-muted-foreground"> → </span>
          <span :class="streamRates.batches > 60 ? 'text-destructive' : ''">{{ streamRates.batches }}</span>
          <span class="text-muted-foreground"> /s</span>
        </span>
      </div>
      <div class="flex items-center justify-between">
        <span class="font-sans text-muted-foreground">{{ t('perf.streamMerged') }}</span>
        <span>{{ streamRates.merged }} <span class="text-muted-foreground">/s</span></span>
      </div>
      <div class="flex items-center justify-between border-t border-border pt-1">
        <span class="font-sans text-muted-foreground">{{ t('perf.projEvents') }}</span>
        <span>
          <span>{{ projEvents.incremental }}</span>
          <span class="text-muted-foreground"> {{ t('perf.incremental') }} · </span>
          <span :class="projEvents.full > 3 ? 'text-destructive' : ''">{{ projEvents.full }}</span>
          <span class="text-muted-foreground"> {{ t('perf.full') }}</span>
        </span>
      </div>
    </div>
  </div>
</template>
