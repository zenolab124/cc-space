import { ref, reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

/** Rust get_perf_stats 返回结构（src-tauri/src/perf.rs） */
export interface ProcMem {
  pid: number
  name: string
  footprint_mb: number
}
export interface PerfStats {
  main: ProcMem
  webkit: ProcMem[]
  cli: ProcMem[]
  total_mb: number
  streaming: {
    input_events: number
    emitted_batches: number
    emitted_events: number
    merged_deltas: number
  }
}

// ---- 采样状态（模块单例；仅 HUD 打开期间运行） ----

const fps = ref(0)
/** 观察到的刷新率基线（ProMotion 120Hz 机器上自动上调，分级阈值随之校准） */
const baselineFps = ref(60)
/** 近 10s 帧间隔超过 2.5 个基线帧周期的次数（丢帧） */
const jankCount = ref(0)
/** 近 10s 最长帧间隔（主线程最长阻塞的近似下界） */
const maxBlockMs = ref(0)
/** 最近一次 pointerdown → 下一帧的延迟（点击响应的直接度量） */
const clickLatencyLast = ref(0)
/** 近 100 次点击延迟的 p95 */
const clickLatencyP95 = ref(0)
const domNodes = ref(0)
const memStats = ref<PerfStats | null>(null)
/** projects-changed 事件计数（验证增量更新是否生效） */
const projEvents = reactive({ incremental: 0, full: 0 })
/** 流式入口事件率、IPC 批次率与合并率（相邻内存采样差分）。 */
const streamRates = reactive({ input: 0, batches: 0, emitted: 0, merged: 0 })
/** 每秒一个 FPS 采样点，最多 60 个（sparkline 数据） */
const fpsHistory = ref<number[]>([])
/** 长帧归因记录：帧总时长、其中 JS 埋点段合计、最耗时的埋点段 */
export interface LongFrame {
  total: number
  js: number
  top: string
}
const longFrames = ref<LongFrame[]>([])

let running = false
let rafId = 0
let lastFrameTs = 0
let framesThisSecond = 0
let secondStartTs = 0
// 10 个 1s 桶的滑动窗口
const jankBuckets: number[] = []
const blockBuckets: number[] = []
let bucketJank = 0
let bucketMaxBlock = 0
const clickSamples: number[] = []
let memTimer = 0
let domTimer = 0
let unlistenProj: UnlistenFn | null = null
let previousStreamSample: { at: number; counters: PerfStats['streaming'] } | null = null
// 代际计数：listen() 是异步的，start→stop→start 交错时旧 listen 的 resolve
// 会覆盖新监听造成泄漏，await 后校验代际未变才落地
let startGen = 0

function frameLoop(ts: number) {
  if (!running) return
  if (lastFrameTs > 0) {
    const interval = ts - lastFrameTs
    // 丢帧阈值随基线刷新率校准：60Hz→41.7ms、120Hz→20.8ms（均为连丢 2 帧）
    if (interval > 2500 / baselineFps.value) bucketJank++
    if (interval > bucketMaxBlock) bucketMaxBlock = interval
    // 长帧归因：>50ms 的帧，分解窗口内 performance.measure 埋点段——
    // JS 合计占比低说明大头在 layout/paint（渲染管线），高则看 top 段名定位脚本源
    if (interval > 50) {
      const measures = performance
        .getEntriesByType('measure')
        .filter(m => m.startTime + m.duration >= lastFrameTs)
      const js = measures.reduce((s, m) => s + m.duration, 0)
      const top = [...measures].sort((a, b) => b.duration - a.duration)[0]
      longFrames.value = [
        ...longFrames.value.slice(-4),
        {
          total: Math.round(interval),
          js: Math.round(js),
          top: top ? `${top.name} ${Math.round(top.duration)}ms` : '—',
        },
      ]
      performance.clearMeasures()
    }
    framesThisSecond++
    if (ts - secondStartTs >= 1000) {
      fps.value = framesThisSecond
      if (framesThisSecond > baselineFps.value) baselineFps.value = framesThisSecond
      // 每秒清一次 measure 缓冲，防条目无限积累（长帧路径已即时消费）
      performance.clearMeasures()
      fpsHistory.value = [...fpsHistory.value.slice(-59), framesThisSecond]
      jankBuckets.push(bucketJank)
      blockBuckets.push(bucketMaxBlock)
      if (jankBuckets.length > 10) jankBuckets.shift()
      if (blockBuckets.length > 10) blockBuckets.shift()
      jankCount.value = jankBuckets.reduce((a, b) => a + b, 0)
      maxBlockMs.value = Math.round(Math.max(...blockBuckets))
      framesThisSecond = 0
      secondStartTs = ts
      bucketJank = 0
      bucketMaxBlock = 0
    }
  } else {
    secondStartTs = ts
  }
  lastFrameTs = ts
  rafId = requestAnimationFrame(frameLoop)
}

function onPointerDown(e: PointerEvent) {
  const t0 = e.timeStamp
  requestAnimationFrame((ts) => {
    const latency = Math.max(0, ts - t0)
    clickLatencyLast.value = Math.round(latency)
    clickSamples.push(latency)
    if (clickSamples.length > 100) clickSamples.shift()
    const sorted = [...clickSamples].sort((a, b) => a - b)
    clickLatencyP95.value = Math.round(sorted[Math.floor(sorted.length * 0.95)] ?? latency)
  })
}

async function pollMem() {
  try {
    const next = await invoke<PerfStats>('get_perf_stats')
    const now = performance.now()
    if (previousStreamSample) {
      const seconds = Math.max(0.001, (now - previousStreamSample.at) / 1000)
      const prev = previousStreamSample.counters
      streamRates.input = Math.round((next.streaming.input_events - prev.input_events) / seconds)
      streamRates.batches = Math.round((next.streaming.emitted_batches - prev.emitted_batches) / seconds)
      streamRates.emitted = Math.round((next.streaming.emitted_events - prev.emitted_events) / seconds)
      streamRates.merged = Math.round((next.streaming.merged_deltas - prev.merged_deltas) / seconds)
    }
    previousStreamSample = { at: now, counters: next.streaming }
    memStats.value = next
  } catch (_) {
    // 采集失败保持旧值
  }
}

function countDom() {
  domNodes.value = document.getElementsByTagName('*').length
}

/** 启动采样（HUD 挂载时调用）。重复调用幂等 */
export async function startPerfMonitor() {
  if (running) return
  running = true
  const gen = ++startGen
  lastFrameTs = 0
  rafId = requestAnimationFrame(frameLoop)
  window.addEventListener('pointerdown', onPointerDown, { capture: true, passive: true })
  pollMem()
  countDom()
  memTimer = window.setInterval(pollMem, 2000)
  domTimer = window.setInterval(countDom, 5000)
  const un = await listen<{ full: boolean }>('projects-changed', (event) => {
    if (event.payload?.full) projEvents.full++
    else projEvents.incremental++
  })
  // await 期间发生过 stop 或新一轮 start：本次监听作废，立即释放
  if (gen !== startGen || !running) {
    un()
    return
  }
  unlistenProj = un
}

/** 停止采样并释放所有监听（HUD 卸载时调用） */
export function stopPerfMonitor() {
  running = false
  startGen++
  cancelAnimationFrame(rafId)
  window.removeEventListener('pointerdown', onPointerDown, { capture: true } as EventListenerOptions)
  clearInterval(memTimer)
  clearInterval(domTimer)
  unlistenProj?.()
  unlistenProj = null
  // 清滑窗，避免重开 HUD 显示陈旧丢帧数据
  jankBuckets.length = 0
  blockBuckets.length = 0
  bucketJank = 0
  bucketMaxBlock = 0
  framesThisSecond = 0
  previousStreamSample = null
  streamRates.input = 0
  streamRates.batches = 0
  streamRates.emitted = 0
  streamRates.merged = 0
}

export function usePerfMonitor() {
  return {
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
  }
}
